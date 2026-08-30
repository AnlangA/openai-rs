mod contracts;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};

const DECISIONS_PATH: &str = "spec/contracts/decisions.md";
const OVERRIDES_PATH: &str = "spec/contracts/manual-overrides.toml";
const OVERRIDES_SCHEMA_PATH: &str = "spec/contracts/manual-overrides.schema.json";

pub(super) struct RenderedArtifact {
    pub(super) relative_path: PathBuf,
    pub(super) bytes: Vec<u8>,
}

pub fn run(repository_root: &Path, check: bool) -> Result<()> {
    validate_contract_inputs(repository_root)?;
    let artifacts = render_artifacts(repository_root)?;

    if check {
        check_artifacts(repository_root, &artifacts)?;
        println!(
            "codegen check passed ({} generators registered)",
            artifacts.len()
        );
    } else {
        write_artifacts(repository_root, &artifacts)?;
        println!(
            "codegen completed ({} generators registered)",
            artifacts.len()
        );
    }
    Ok(())
}

fn render_artifacts(repository_root: &Path) -> Result<Vec<RenderedArtifact>> {
    contracts::render(repository_root)
}

fn validate_contract_inputs(repository_root: &Path) -> Result<()> {
    let decisions_path = repository_root.join(DECISIONS_PATH);
    let decisions = read_text(&decisions_path, "read decision ledger")?;
    if !decisions.starts_with("# Contract Decision Ledger") {
        return Err(Error::message(format!(
            "{} must start with `# Contract Decision Ledger`",
            decisions_path.display()
        )));
    }

    let overrides_path = repository_root.join(OVERRIDES_PATH);
    let overrides = read_text(&overrides_path, "read manual overrides")?;
    validate_override_manifest(&overrides, &overrides_path)?;

    let schema_path = repository_root.join(OVERRIDES_SCHEMA_PATH);
    let schema_bytes = fs::read(&schema_path)
        .map_err(|source| Error::io("read manual override schema", &schema_path, source))?;
    let schema: Value = serde_json::from_slice(&schema_bytes).map_err(|source| Error::Json {
        path: schema_path.clone(),
        source,
    })?;
    if schema.pointer("/properties/schema_version/const") != Some(&Value::from(1))
        || schema.pointer("/properties/overrides/type") != Some(&Value::from("array"))
    {
        return Err(Error::message(format!(
            "{} must define schema_version const 1 and an overrides array",
            schema_path.display()
        )));
    }
    Ok(())
}

fn validate_override_manifest(input: &str, path: &Path) -> Result<()> {
    let has_schema_version = input
        .lines()
        .any(|line| line.trim() == "schema_version = 1");
    let declares_empty = input
        .lines()
        .any(|line| line.trim() == "overrides = []");
    let mut blocks = Vec::new();
    let mut current: Option<BTreeSet<String>> = None;
    let mut ids = BTreeSet::new();

    for raw_line in input.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[overrides]]" {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            current = Some(BTreeSet::new());
            continue;
        }
        let Some(block) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            return Err(Error::message(format!(
                "invalid manual override assignment in {}: {line}",
                path.display()
            )));
        };
        let key = key.trim().to_owned();
        if !block.insert(key.clone()) {
            return Err(Error::message(format!(
                "duplicate manual override key `{key}` in {}",
                path.display()
            )));
        }
        if key == "id" {
            let id = serde_json::from_str::<String>(value.trim()).map_err(|source| {
                Error::message(format!(
                    "manual override id in {} must be a TOML basic string compatible with JSON quoting: {source}",
                    path.display()
                ))
            })?;
            if !ids.insert(id.clone()) {
                return Err(Error::message(format!(
                    "duplicate manual override id `{id}` in {}",
                    path.display()
                )));
            }
        }
    }
    if let Some(block) = current {
        blocks.push(block);
    }

    if !has_schema_version || (declares_empty == !blocks.is_empty()) {
        return Err(Error::message(format!(
            "{} must declare schema_version = 1 and exactly one of `overrides = []` or [[overrides]] entries",
            path.display()
        )));
    }
    let required = [
        "id",
        "target",
        "action",
        "sources",
        "reviewed_at",
        "reason",
        "impact",
        "tests",
    ];
    for (index, block) in blocks.iter().enumerate() {
        for key in required {
            if !block.contains(key) {
                return Err(Error::message(format!(
                    "manual override {} in {} is missing `{key}`",
                    index + 1,
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn check_artifacts(repository_root: &Path, artifacts: &[RenderedArtifact]) -> Result<()> {
    let mut problems = Vec::new();
    for artifact in artifacts {
        let path = repository_root.join(&artifact.relative_path);
        match fs::read(&path) {
            Ok(existing) if existing == artifact.bytes => {}
            Ok(_) => problems.push(format!("generated file differs: {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                problems.push(format!("generated file is missing: {}", path.display()));
            }
            Err(source) => return Err(Error::io("read generated file", path, source)),
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(Error::verification(&problems))
    }
}

fn write_artifacts(repository_root: &Path, artifacts: &[RenderedArtifact]) -> Result<()> {
    for artifact in artifacts {
        let path = repository_root.join(&artifact.relative_path);
        let parent = path.parent().ok_or_else(|| {
            Error::message(format!("generated path has no parent: {}", path.display()))
        })?;
        fs::create_dir_all(parent)
            .map_err(|source| Error::io("create generated directory", parent, source))?;
        fs::write(&path, &artifact.bytes)
            .map_err(|source| Error::io("write generated file", path, source))?;
    }
    Ok(())
}

fn read_text(path: &Path, action: &'static str) -> Result<String> {
    fs::read_to_string(path).map_err(|source| Error::io(action, path, source))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{RenderedArtifact, check_artifacts, validate_override_manifest};

    #[test]
    fn empty_artifact_set_is_a_zero_diff() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir();
        let artifacts: Vec<RenderedArtifact> = Vec::new();
        check_artifacts(&root, &artifacts)?;
        Ok(())
    }

    #[test]
    fn accepts_nonempty_override_manifest() -> Result<(), Box<dyn std::error::Error>> {
        let input = "schema_version = 1\n[[overrides]]\nid = \"OVR-0001\"\ntarget = \"/paths\"\naction = \"replace\"\nsources = []\nreviewed_at = \"2026-08-30\"\nreason = \"proof\"\nimpact = [\"proof\"]\ntests = [\"proof\"]\n";
        validate_override_manifest(input, Path::new("manual-overrides.toml"))?;
        Ok(())
    }
}
