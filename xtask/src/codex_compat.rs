use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};

const MANIFEST_PATH: &str = "spec/contracts/codex-compatibility.toml";
const SCHEMA_PATH: &str = "spec/contracts/codex-compatibility.schema.json";

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RuntimeIdentity {
    version: String,
    target: String,
    executable_sha256: String,
    app_server_schema_sha256: String,
}

pub fn verify(repository_root: &Path) -> Result<()> {
    let manifest_path = repository_root.join(MANIFEST_PATH);
    let input = fs::read_to_string(&manifest_path)
        .map_err(|source| Error::io("read Codex compatibility manifest", &manifest_path, source))?;
    let runtimes = parse_manifest(&input, &manifest_path)?;

    let schema_path = repository_root.join(SCHEMA_PATH);
    let schema_bytes = fs::read(&schema_path)
        .map_err(|source| Error::io("read Codex compatibility schema", &schema_path, source))?;
    let schema: Value = serde_json::from_slice(&schema_bytes).map_err(|source| Error::Json {
        path: schema_path.clone(),
        source,
    })?;
    if schema.pointer("/properties/schema_version/const") != Some(&Value::from(1))
        || schema.pointer("/properties/runtime/type") != Some(&Value::from("array"))
    {
        return Err(Error::message(format!(
            "{} must define schema_version const 1 and a runtime array",
            schema_path.display()
        )));
    }

    println!(
        "verified Codex runtime compatibility manifest ({} audited runtimes; unknown and 0.0.0 runtimes remain unsupported)",
        runtimes.len()
    );
    Ok(())
}

fn parse_manifest(input: &str, path: &Path) -> Result<Vec<RuntimeIdentity>> {
    let mut schema_version = None;
    let mut declared_empty = false;
    let mut current_runtime: Option<BTreeMap<String, String>> = None;
    let mut runtime_values = Vec::new();

    for (line_index, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[runtime]]" {
            if declared_empty {
                return Err(Error::message(format!(
                    "{} cannot combine `runtime = []` with [[runtime]] entries",
                    path.display()
                )));
            }
            if let Some(runtime) = current_runtime.take() {
                runtime_values.push(runtime);
            }
            current_runtime = Some(BTreeMap::new());
            continue;
        }
        if line.starts_with('[') {
            return Err(Error::message(format!(
                "unsupported section in {} at line {}: {line}",
                path.display(),
                line_index + 1
            )));
        }

        let (key, value) = line.split_once('=').ok_or_else(|| {
            Error::message(format!(
                "invalid assignment in {} at line {}",
                path.display(),
                line_index + 1
            ))
        })?;
        let key = key.trim();
        let value = value.trim();
        if let Some(runtime) = current_runtime.as_mut() {
            if runtime.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(Error::message(format!(
                    "duplicate runtime key `{key}` in {}",
                    path.display()
                )));
            }
        } else {
            match key {
                "schema_version" if schema_version.is_none() => {
                    schema_version = Some(parse_u64(value, key, path)?);
                }
                "runtime" if value == "[]" && !declared_empty => declared_empty = true,
                "schema_version" | "runtime" => {
                    return Err(Error::message(format!(
                        "duplicate or invalid top-level key `{key}` in {}",
                        path.display()
                    )));
                }
                _ => {
                    return Err(Error::message(format!(
                        "unknown top-level key `{key}` in {}",
                        path.display()
                    )));
                }
            }
        }
    }

    if let Some(runtime) = current_runtime {
        runtime_values.push(runtime);
    }
    if schema_version != Some(1) {
        return Err(Error::message(format!(
            "{} must declare `schema_version = 1`",
            path.display()
        )));
    }
    if !declared_empty && runtime_values.is_empty() {
        return Err(Error::message(format!(
            "{} must declare `runtime = []` or one or more [[runtime]] entries",
            path.display()
        )));
    }

    let runtimes = runtime_values
        .into_iter()
        .map(|values| parse_runtime(values, path))
        .collect::<Result<Vec<_>>>()?;
    let mut unique = BTreeSet::new();
    for runtime in &runtimes {
        let key = (
            runtime.version.as_str(),
            runtime.target.as_str(),
            runtime.executable_sha256.as_str(),
        );
        if !unique.insert(key) {
            return Err(Error::message(format!(
                "duplicate Codex runtime mapping for version {}, target {}, executable {} in {}",
                runtime.version,
                runtime.target,
                runtime.executable_sha256,
                path.display()
            )));
        }
    }
    Ok(runtimes)
}

fn parse_runtime(mut values: BTreeMap<String, String>, path: &Path) -> Result<RuntimeIdentity> {
    let version = take_string(&mut values, "version", path)?;
    let target = take_string(&mut values, "target", path)?;
    let executable_sha256 = take_string(&mut values, "executable_sha256", path)?;
    let app_server_schema_sha256 = take_string(&mut values, "app_server_schema_sha256", path)?;

    if !values.is_empty() {
        return Err(Error::message(format!(
            "unknown Codex runtime keys in {}: {}",
            path.display(),
            values.keys().cloned().collect::<Vec<_>>().join(", ")
        )));
    }
    if version == "0.0.0" || !is_exact_release_version(&version) {
        return Err(Error::message(format!(
            "Codex runtime version `{version}` in {} is unsupported; use an exact released x.y.z version and never 0.0.0",
            path.display()
        )));
    }
    if !is_exact_target(&target) {
        return Err(Error::message(format!(
            "Codex runtime target `{target}` in {} must be an exact target triple without wildcards",
            path.display()
        )));
    }
    for (label, hash) in [
        ("executable_sha256", executable_sha256.as_str()),
        (
            "app_server_schema_sha256",
            app_server_schema_sha256.as_str(),
        ),
    ] {
        if !is_sha256(hash) {
            return Err(Error::message(format!(
                "Codex runtime {label} in {} must be 64 lowercase hexadecimal characters",
                path.display()
            )));
        }
    }

    Ok(RuntimeIdentity {
        version,
        target,
        executable_sha256,
        app_server_schema_sha256,
    })
}

fn is_exact_release_version(version: &str) -> bool {
    let components = version.split('.').collect::<Vec<_>>();
    components.len() == 3
        && components.iter().all(|component| {
            !component.is_empty()
                && component.bytes().all(|byte| byte.is_ascii_digit())
                && (component == &"0" || !component.starts_with('0'))
        })
}

fn is_exact_target(target: &str) -> bool {
    !target.is_empty()
        && !target.contains('*')
        && !target.chars().any(char::is_whitespace)
        && target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn is_sha256(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn take_string(values: &mut BTreeMap<String, String>, key: &str, path: &Path) -> Result<String> {
    let raw = values.remove(key).ok_or_else(|| {
        Error::message(format!(
            "missing Codex runtime key `{key}` in {}",
            path.display()
        ))
    })?;
    serde_json::from_str::<String>(&raw).map_err(|source| {
        Error::message(format!(
            "Codex runtime key `{key}` in {} must be a TOML basic string compatible with JSON quoting: {source}",
            path.display()
        ))
    })
}

fn parse_u64(raw: &str, key: &str, path: &Path) -> Result<u64> {
    raw.parse::<u64>().map_err(|source| {
        Error::message(format!(
            "Codex compatibility key `{key}` in {} must be a non-negative integer: {source}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::parse_manifest;

    #[test]
    fn accepts_explicitly_empty_manifest() -> Result<(), Box<dyn std::error::Error>> {
        let runtimes = parse_manifest(
            "schema_version = 1\nruntime = []\n",
            Path::new("codex-compatibility.toml"),
        )?;
        assert!(runtimes.is_empty());
        Ok(())
    }

    #[test]
    fn accepts_exact_audited_mapping_shape() -> Result<(), Box<dyn std::error::Error>> {
        let input = format!(
            "schema_version = 1\n[[runtime]]\nversion = \"1.2.3\"\ntarget = \"aarch64-apple-darwin\"\nexecutable_sha256 = \"{}\"\napp_server_schema_sha256 = \"{}\"\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        assert_eq!(
            parse_manifest(&input, Path::new("codex-compatibility.toml"))?.len(),
            1
        );
        Ok(())
    }

    #[test]
    fn rejects_zero_version_and_wildcards() {
        let input = format!(
            "schema_version = 1\n[[runtime]]\nversion = \"0.0.0\"\ntarget = \"*-apple-darwin\"\nexecutable_sha256 = \"{}\"\napp_server_schema_sha256 = \"{}\"\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        assert!(parse_manifest(&input, Path::new("codex-compatibility.toml")).is_err());
    }
}
