use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::cli::FetchArguments;
use crate::error::{Error, Result};

pub const PINNED_REVISION: &str = "690521b1753dce0c6d6b275f583d22537679cff9";
pub const PINNED_SHA256: &str = "5be8cde8490bd8422e1b3502b80e858e7c162ec3e01b187b633577dab6d0c899";
pub const SNAPSHOT_PATH: &str = "spec/upstream/openapi-2026-08-29.json";
pub const SOURCES_PATH: &str = "spec/SOURCES.toml";

const SOURCE_ID: &str = "openai-openapi";
const SOURCE_REPOSITORY: &str = "openai/openai-openapi";
const SOURCE_LICENSE: &str = "MIT";
const PINNED_BYTE_LENGTH: usize = 3_405_126;
const PINNED_OPENAPI_VERSION: &str = "3.1.0";
const PINNED_TITLE: &str = "OpenAI API";
const PINNED_API_VERSION: &str = "2.3.0";
const PINNED_PATH_COUNT: usize = 182;
const PINNED_CLIENT_OPERATION_COUNT: usize = 288;
const PINNED_WEBHOOK_COUNT: usize = 18;
const PINNED_WEBHOOK_OPERATION_COUNT: usize = 18;
const PINNED_SCHEMA_COUNT: usize = 1_424;
const MAX_DOWNLOAD_BYTES: &str = "20000000";
const HTTP_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

#[derive(Debug, Clone, Eq, PartialEq)]
struct SnapshotSummary {
    byte_length: usize,
    sha256: String,
    openapi_version: String,
    title: String,
    api_version: String,
    path_count: usize,
    client_operation_count: usize,
    webhook_count: usize,
    webhook_operation_count: usize,
    schema_count: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct SourceManifest {
    schema_version: u64,
    id: String,
    kind: String,
    repository: String,
    revision: String,
    url: String,
    path: String,
    fetched_at: String,
    byte_length: usize,
    sha256: String,
    license: String,
    license_url: String,
    openapi_version: String,
    api_version: String,
    path_count: usize,
    client_operation_count: usize,
    webhook_count: usize,
    webhook_operation_count: usize,
    schema_count: usize,
}

pub fn fetch(repository_root: &Path, arguments: &FetchArguments) -> Result<()> {
    let url = resolve_source_url(arguments)?;
    println!("fetching immutable OpenAPI source {url}");
    let bytes = download(&url)?;
    let summary = inspect_snapshot(&bytes, Path::new("<download>"))?;
    verify_pinned_summary(&summary)?;

    let snapshot_path = repository_root.join(SNAPSHOT_PATH);
    let sources_path = repository_root.join(SOURCES_PATH);
    ensure_absent_or_equal(&snapshot_path, &bytes)?;

    let manifest = if sources_path.exists() {
        let manifest = load_manifest(&sources_path)?;
        verify_manifest(&manifest, &summary)?;
        None
    } else {
        Some(SourceManifest::new(now_utc_timestamp()?, &summary))
    };

    write_new_if_missing(&snapshot_path, &bytes)?;
    if let Some(manifest) = manifest {
        let rendered = render_manifest(&manifest);
        write_new_if_missing(&sources_path, rendered.as_bytes())?;
    }

    println!(
        "recorded {} bytes, sha256={}, paths={}, operations={}+{}, schemas={}",
        summary.byte_length,
        summary.sha256,
        summary.path_count,
        summary.client_operation_count,
        summary.webhook_operation_count,
        summary.schema_count
    );
    Ok(())
}

pub fn verify(repository_root: &Path) -> Result<()> {
    let snapshot_path = repository_root.join(SNAPSHOT_PATH);
    let bytes = fs::read(&snapshot_path)
        .map_err(|source| Error::io("read OpenAPI snapshot", &snapshot_path, source))?;
    let summary = inspect_snapshot(&bytes, &snapshot_path)?;
    verify_pinned_summary(&summary)?;

    let sources_path = repository_root.join(SOURCES_PATH);
    let manifest = load_manifest(&sources_path)?;
    verify_manifest(&manifest, &summary)?;
    crate::codex_compat::verify(repository_root)?;

    println!(
        "verified OpenAPI {} ({} bytes, {} paths, {} client operations, {} webhook operations, {} schemas)",
        PINNED_REVISION,
        summary.byte_length,
        summary.path_count,
        summary.client_operation_count,
        summary.webhook_operation_count,
        summary.schema_count
    );
    Ok(())
}

fn resolve_source_url(arguments: &FetchArguments) -> Result<String> {
    validate_revision(&arguments.revision)?;
    if arguments.revision != PINNED_REVISION {
        return Err(Error::message(format!(
            "revision {} is immutable but is not the audited baseline {}; update the pinned contract intentionally before fetching it",
            arguments.revision, PINNED_REVISION
        )));
    }

    let expected = official_source_url(&arguments.revision);
    match &arguments.url {
        Some(url) if url != &expected => Err(Error::message(format!(
            "explicit URL must exactly match the immutable official source `{expected}`"
        ))),
        Some(url) => Ok(url.clone()),
        None => Ok(expected),
    }
}

fn validate_revision(revision: &str) -> Result<()> {
    if revision.len() == 40
        && revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(Error::message(format!(
            "revision must be exactly 40 lowercase hexadecimal characters, got `{revision}`"
        )))
    }
}

fn official_source_url(revision: &str) -> String {
    format!("https://raw.githubusercontent.com/{SOURCE_REPOSITORY}/{revision}/openapi.json")
}

fn official_license_url(revision: &str) -> String {
    format!("https://raw.githubusercontent.com/{SOURCE_REPOSITORY}/{revision}/LICENSE")
}

fn download(url: &str) -> Result<Vec<u8>> {
    let output = Command::new("curl")
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "0",
            "--connect-timeout",
            "20",
            "--max-time",
            "120",
            "--max-filesize",
            MAX_DOWNLOAD_BYTES,
            url,
        ])
        .output()
        .map_err(|source| Error::io("start curl", PathBuf::from("curl"), source))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(Error::message(format!(
            "curl failed while fetching the official snapshot (status {}): {stderr}",
            output.status
        )));
    }
    Ok(output.stdout)
}

fn inspect_snapshot(bytes: &[u8], path: &Path) -> Result<SnapshotSummary> {
    let document: Value = serde_json::from_slice(bytes).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })?;

    let paths = object_at(&document, "/paths", path)?;
    let webhooks = object_at(&document, "/webhooks", path)?;
    let schemas = object_at(&document, "/components/schemas", path)?;

    Ok(SnapshotSummary {
        byte_length: bytes.len(),
        sha256: sha256_hex(bytes),
        openapi_version: string_at(&document, "/openapi", path)?,
        title: string_at(&document, "/info/title", path)?,
        api_version: string_at(&document, "/info/version", path)?,
        path_count: paths.len(),
        client_operation_count: count_operations(paths, "/paths", path)?,
        webhook_count: webhooks.len(),
        webhook_operation_count: count_operations(webhooks, "/webhooks", path)?,
        schema_count: schemas.len(),
    })
}

fn object_at<'a>(
    document: &'a Value,
    pointer: &str,
    path: &Path,
) -> Result<&'a Map<String, Value>> {
    document
        .pointer(pointer)
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Error::message(format!(
                "{} must contain an object at JSON pointer {pointer}",
                path.display()
            ))
        })
}

fn string_at(document: &Value, pointer: &str, path: &Path) -> Result<String> {
    document
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            Error::message(format!(
                "{} must contain a string at JSON pointer {pointer}",
                path.display()
            ))
        })
}

fn count_operations(collection: &Map<String, Value>, pointer: &str, path: &Path) -> Result<usize> {
    let mut count = 0;
    for (name, path_item) in collection {
        let path_item = path_item.as_object().ok_or_else(|| {
            Error::message(format!(
                "{} must contain an object at {pointer}/{}",
                path.display(),
                escape_json_pointer(name)
            ))
        })?;
        for method in HTTP_METHODS {
            if let Some(operation) = path_item.get(method) {
                if !operation.is_object() {
                    return Err(Error::message(format!(
                        "{} operation at {pointer}/{}/{method} must be an object",
                        path.display(),
                        escape_json_pointer(name)
                    )));
                }
                count += 1;
            }
        }
    }
    Ok(count)
}

fn escape_json_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn verify_pinned_summary(summary: &SnapshotSummary) -> Result<()> {
    let mut problems = Vec::new();
    compare(
        &mut problems,
        "byte length",
        PINNED_BYTE_LENGTH,
        summary.byte_length,
    );
    compare(
        &mut problems,
        "SHA-256",
        PINNED_SHA256,
        summary.sha256.as_str(),
    );
    compare(
        &mut problems,
        "OpenAPI version",
        PINNED_OPENAPI_VERSION,
        summary.openapi_version.as_str(),
    );
    compare(
        &mut problems,
        "API title",
        PINNED_TITLE,
        summary.title.as_str(),
    );
    compare(
        &mut problems,
        "API version",
        PINNED_API_VERSION,
        summary.api_version.as_str(),
    );
    compare(
        &mut problems,
        "path count",
        PINNED_PATH_COUNT,
        summary.path_count,
    );
    compare(
        &mut problems,
        "client operation count",
        PINNED_CLIENT_OPERATION_COUNT,
        summary.client_operation_count,
    );
    compare(
        &mut problems,
        "webhook count",
        PINNED_WEBHOOK_COUNT,
        summary.webhook_count,
    );
    compare(
        &mut problems,
        "webhook operation count",
        PINNED_WEBHOOK_OPERATION_COUNT,
        summary.webhook_operation_count,
    );
    compare(
        &mut problems,
        "schema count",
        PINNED_SCHEMA_COUNT,
        summary.schema_count,
    );
    verification_result(problems)
}

fn compare<T: std::fmt::Display + PartialEq>(
    problems: &mut Vec<String>,
    label: &str,
    expected: T,
    actual: T,
) {
    if expected != actual {
        problems.push(format!("{label}: expected {expected}, found {actual}"));
    }
}

fn verification_result(problems: Vec<String>) -> Result<()> {
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Error::verification(&problems))
    }
}

impl SourceManifest {
    fn new(fetched_at: String, summary: &SnapshotSummary) -> Self {
        Self {
            schema_version: 1,
            id: SOURCE_ID.to_owned(),
            kind: "openapi".to_owned(),
            repository: SOURCE_REPOSITORY.to_owned(),
            revision: PINNED_REVISION.to_owned(),
            url: official_source_url(PINNED_REVISION),
            path: SNAPSHOT_PATH.to_owned(),
            fetched_at,
            byte_length: summary.byte_length,
            sha256: summary.sha256.clone(),
            license: SOURCE_LICENSE.to_owned(),
            license_url: official_license_url(PINNED_REVISION),
            openapi_version: summary.openapi_version.clone(),
            api_version: summary.api_version.clone(),
            path_count: summary.path_count,
            client_operation_count: summary.client_operation_count,
            webhook_count: summary.webhook_count,
            webhook_operation_count: summary.webhook_operation_count,
            schema_count: summary.schema_count,
        }
    }
}

fn render_manifest(manifest: &SourceManifest) -> String {
    format!(
        "# Frozen upstream provenance. Regenerate only through `cargo xtask spec fetch`.\n\
# The URL and revision are immutable; normal builds and verification stay offline.\n\
\n\
schema_version = {}\n\
\n\
[[source]]\n\
id = {:?}\n\
kind = {:?}\n\
repository = {:?}\n\
revision = {:?}\n\
url = {:?}\n\
path = {:?}\n\
fetched_at = {:?}\n\
byte_length = {}\n\
sha256 = {:?}\n\
license = {:?}\n\
license_url = {:?}\n\
openapi_version = {:?}\n\
api_version = {:?}\n\
path_count = {}\n\
client_operation_count = {}\n\
webhook_count = {}\n\
webhook_operation_count = {}\n\
schema_count = {}\n",
        manifest.schema_version,
        manifest.id,
        manifest.kind,
        manifest.repository,
        manifest.revision,
        manifest.url,
        manifest.path,
        manifest.fetched_at,
        manifest.byte_length,
        manifest.sha256,
        manifest.license,
        manifest.license_url,
        manifest.openapi_version,
        manifest.api_version,
        manifest.path_count,
        manifest.client_operation_count,
        manifest.webhook_count,
        manifest.webhook_operation_count,
        manifest.schema_count,
    )
}

fn load_manifest(path: &Path) -> Result<SourceManifest> {
    let input = fs::read_to_string(path)
        .map_err(|source| Error::io("read source manifest", path, source))?;
    parse_manifest(&input, path)
}

fn parse_manifest(input: &str, path: &Path) -> Result<SourceManifest> {
    let mut top_level = BTreeMap::new();
    let mut sources = Vec::new();
    let mut current_source: Option<BTreeMap<String, String>> = None;

    for (line_index, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[source]]" {
            if let Some(source) = current_source.take() {
                sources.push(source);
            }
            current_source = Some(BTreeMap::new());
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
        let key = key.trim().to_owned();
        let value = value.trim().to_owned();
        let values = current_source.as_mut().unwrap_or(&mut top_level);
        if values.insert(key.clone(), value).is_some() {
            return Err(Error::message(format!(
                "duplicate manifest key `{key}` in {}",
                path.display()
            )));
        }
    }

    if let Some(source) = current_source {
        sources.push(source);
    }
    if sources.is_empty() {
        return Err(Error::message(format!(
            "{} must contain at least one [[source]] entry",
            path.display()
        )));
    }

    let schema_version = take_u64(&mut top_level, "schema_version", path)?;
    let mut matching_sources = sources
        .into_iter()
        .filter_map(|values| match peek_string(&values, "id", path) {
            Ok(id) if id == SOURCE_ID => Some(Ok(values)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>>>()?;
    if matching_sources.len() != 1 {
        return Err(Error::message(format!(
            "{} must contain exactly one [[source]] with id `{SOURCE_ID}`, found {}",
            path.display(),
            matching_sources.len()
        )));
    }
    let mut values = matching_sources.pop().ok_or_else(|| {
        Error::message(format!(
            "{} is missing source `{SOURCE_ID}` after validation",
            path.display()
        ))
    })?;

    let id = take_string(&mut values, "id", path)?;
    let kind = take_string(&mut values, "kind", path)?;
    let repository = take_string(&mut values, "repository", path)?;
    let revision = take_string(&mut values, "revision", path)?;
    let url = take_string(&mut values, "url", path)?;
    let source_path = take_string(&mut values, "path", path)?;
    let fetched_at = take_string(&mut values, "fetched_at", path)?;
    let byte_length = take_usize(&mut values, "byte_length", path)?;
    let sha256 = take_string(&mut values, "sha256", path)?;
    let license = take_string(&mut values, "license", path)?;
    let license_url = take_string(&mut values, "license_url", path)?;
    let openapi_version = take_string(&mut values, "openapi_version", path)?;
    let api_version = take_string(&mut values, "api_version", path)?;
    let path_count = take_usize(&mut values, "path_count", path)?;
    let client_operation_count = take_usize(&mut values, "client_operation_count", path)?;
    let webhook_count = take_usize(&mut values, "webhook_count", path)?;
    let webhook_operation_count = take_usize(&mut values, "webhook_operation_count", path)?;
    let schema_count = take_usize(&mut values, "schema_count", path)?;

    if !values.is_empty() {
        return Err(Error::message(format!(
            "unknown manifest keys in {}: {}",
            path.display(),
            values.keys().cloned().collect::<Vec<_>>().join(", ")
        )));
    }

    Ok(SourceManifest {
        schema_version,
        id,
        kind,
        repository,
        revision,
        url,
        path: source_path,
        fetched_at,
        byte_length,
        sha256,
        license,
        license_url,
        openapi_version,
        api_version,
        path_count,
        client_operation_count,
        webhook_count,
        webhook_operation_count,
        schema_count,
    })
}

fn peek_string(values: &BTreeMap<String, String>, key: &str, path: &Path) -> Result<String> {
    let raw = values.get(key).ok_or_else(|| {
        Error::message(format!(
            "missing manifest key `{key}` in one [[source]] entry in {}",
            path.display()
        ))
    })?;
    serde_json::from_str::<String>(raw).map_err(|source| {
        Error::message(format!(
            "manifest key `{key}` in {} must be a TOML basic string compatible with JSON quoting: {source}",
            path.display()
        ))
    })
}

fn take_string(values: &mut BTreeMap<String, String>, key: &str, path: &Path) -> Result<String> {
    let raw = values.remove(key).ok_or_else(|| {
        Error::message(format!(
            "missing manifest key `{key}` in {}",
            path.display()
        ))
    })?;
    serde_json::from_str::<String>(&raw).map_err(|source| {
        Error::message(format!(
            "manifest key `{key}` in {} must be a TOML basic string compatible with JSON quoting: {source}",
            path.display()
        ))
    })
}

fn take_u64(values: &mut BTreeMap<String, String>, key: &str, path: &Path) -> Result<u64> {
    let raw = values.remove(key).ok_or_else(|| {
        Error::message(format!(
            "missing manifest key `{key}` in {}",
            path.display()
        ))
    })?;
    raw.parse::<u64>().map_err(|source| {
        Error::message(format!(
            "manifest key `{key}` in {} must be a non-negative integer: {source}",
            path.display()
        ))
    })
}

fn take_usize(values: &mut BTreeMap<String, String>, key: &str, path: &Path) -> Result<usize> {
    let value = take_u64(values, key, path)?;
    usize::try_from(value).map_err(|source| {
        Error::message(format!(
            "manifest key `{key}` in {} does not fit usize: {source}",
            path.display()
        ))
    })
}

fn verify_manifest(manifest: &SourceManifest, summary: &SnapshotSummary) -> Result<()> {
    let mut problems = Vec::new();
    compare(
        &mut problems,
        "manifest schema_version",
        1,
        manifest.schema_version,
    );
    compare(
        &mut problems,
        "manifest id",
        SOURCE_ID,
        manifest.id.as_str(),
    );
    compare(
        &mut problems,
        "manifest kind",
        "openapi",
        manifest.kind.as_str(),
    );
    compare(
        &mut problems,
        "manifest repository",
        SOURCE_REPOSITORY,
        manifest.repository.as_str(),
    );
    compare(
        &mut problems,
        "manifest revision",
        PINNED_REVISION,
        manifest.revision.as_str(),
    );
    compare(
        &mut problems,
        "manifest URL",
        official_source_url(PINNED_REVISION).as_str(),
        manifest.url.as_str(),
    );
    compare(
        &mut problems,
        "manifest path",
        SNAPSHOT_PATH,
        manifest.path.as_str(),
    );
    compare(
        &mut problems,
        "manifest byte length",
        summary.byte_length,
        manifest.byte_length,
    );
    compare(
        &mut problems,
        "manifest SHA-256",
        summary.sha256.as_str(),
        manifest.sha256.as_str(),
    );
    compare(
        &mut problems,
        "manifest license",
        SOURCE_LICENSE,
        manifest.license.as_str(),
    );
    compare(
        &mut problems,
        "manifest license URL",
        official_license_url(PINNED_REVISION).as_str(),
        manifest.license_url.as_str(),
    );
    compare(
        &mut problems,
        "manifest OpenAPI version",
        summary.openapi_version.as_str(),
        manifest.openapi_version.as_str(),
    );
    compare(
        &mut problems,
        "manifest API version",
        summary.api_version.as_str(),
        manifest.api_version.as_str(),
    );
    compare(
        &mut problems,
        "manifest path count",
        summary.path_count,
        manifest.path_count,
    );
    compare(
        &mut problems,
        "manifest client operation count",
        summary.client_operation_count,
        manifest.client_operation_count,
    );
    compare(
        &mut problems,
        "manifest webhook count",
        summary.webhook_count,
        manifest.webhook_count,
    );
    compare(
        &mut problems,
        "manifest webhook operation count",
        summary.webhook_operation_count,
        manifest.webhook_operation_count,
    );
    compare(
        &mut problems,
        "manifest schema count",
        summary.schema_count,
        manifest.schema_count,
    );
    if manifest.fetched_at.len() != 20
        || !manifest.fetched_at.ends_with('Z')
        || !manifest.fetched_at.is_ascii()
    {
        problems.push(format!(
            "manifest fetched_at must use UTC YYYY-MM-DDTHH:MM:SSZ, found {}",
            manifest.fetched_at
        ));
    }
    verification_result(problems)
}

fn ensure_absent_or_equal(path: &Path, expected: &[u8]) -> Result<()> {
    match fs::read(path) {
        Ok(existing) if existing == expected => Ok(()),
        Ok(_) => Err(Error::message(format!(
            "refusing to overwrite existing audited artifact {} with different bytes",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::io("read existing artifact", path, source)),
    }
}

fn write_new_if_missing(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        return ensure_absent_or_equal(path, bytes);
    }

    let parent = path.parent().ok_or_else(|| {
        Error::message(format!("artifact path has no parent: {}", path.display()))
    })?;
    fs::create_dir_all(parent)
        .map_err(|source| Error::io("create artifact directory", parent, source))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::message(format!("invalid artifact file name: {}", path.display())))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|source| Error::message(format!("system clock predates Unix epoch: {source}")))?
        .as_nanos();
    let temporary_path = parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)
        .map_err(|source| Error::io("create temporary artifact", &temporary_path, source))?;
    file.write_all(bytes)
        .map_err(|source| Error::io("write temporary artifact", &temporary_path, source))?;
    file.sync_all()
        .map_err(|source| Error::io("sync temporary artifact", &temporary_path, source))?;
    fs::hard_link(&temporary_path, path)
        .map_err(|source| Error::io("install audited artifact without overwrite", path, source))?;
    fs::remove_file(&temporary_path).map_err(|source| {
        Error::io(
            "remove installed artifact temporary link",
            &temporary_path,
            source,
        )
    })?;
    Ok(())
}

fn now_utc_timestamp() -> Result<String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|source| Error::message(format!("system clock predates Unix epoch: {source}")))?
        .as_secs();
    Ok(format_utc_timestamp(seconds))
}

fn format_utc_timestamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;

    let shifted_days = days + 719_468;
    let era = shifted_days / 146_097;
    let day_of_era = shifted_days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::{
        PINNED_REVISION, SnapshotSummary, SourceManifest, count_operations, format_utc_timestamp,
        official_source_url, parse_manifest, render_manifest, resolve_source_url, sha256_hex,
    };
    use crate::cli::FetchArguments;

    #[test]
    fn hashes_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn accepts_only_exact_official_url() -> Result<(), Box<dyn std::error::Error>> {
        let expected = official_source_url(PINNED_REVISION);
        let arguments = FetchArguments {
            revision: PINNED_REVISION.to_owned(),
            url: Some(expected.clone()),
        };
        assert_eq!(resolve_source_url(&arguments)?, expected);

        let invalid = FetchArguments {
            revision: PINNED_REVISION.to_owned(),
            url: Some(format!(
                "https://example.com/{PINNED_REVISION}/openapi.json"
            )),
        };
        assert!(resolve_source_url(&invalid).is_err());
        Ok(())
    }

    #[test]
    fn counts_only_http_operation_members() -> Result<(), Box<dyn std::error::Error>> {
        let collection = json!({
            "/widgets": {
                "parameters": [],
                "get": {},
                "post": {},
                "x-extension": {}
            }
        });
        let object = collection.as_object().ok_or("fixture must be an object")?;
        assert_eq!(count_operations(object, "/paths", Path::new("fixture"))?, 2);
        Ok(())
    }

    #[test]
    fn manifest_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let summary = SnapshotSummary {
            byte_length: 3,
            sha256: "abc".to_owned(),
            openapi_version: "3.1.0".to_owned(),
            title: "OpenAI API".to_owned(),
            api_version: "2.3.0".to_owned(),
            path_count: 1,
            client_operation_count: 2,
            webhook_count: 3,
            webhook_operation_count: 4,
            schema_count: 5,
        };
        let manifest = SourceManifest::new("2026-08-30T00:00:00Z".to_owned(), &summary);
        let rendered = render_manifest(&manifest);
        assert_eq!(
            parse_manifest(&rendered, Path::new("SOURCES.toml"))?,
            manifest
        );

        let extended = format!(
            "{rendered}\n[[source]]\nid = \"future-sdk-source\"\nkind = \"sdk\"\ncustom_field = 42\n"
        );
        assert_eq!(
            parse_manifest(&extended, Path::new("SOURCES.toml"))?,
            manifest
        );
        Ok(())
    }

    #[test]
    fn formats_utc_timestamp_without_platform_date_tools() {
        assert_eq!(format_utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc_timestamp(86_400), "1970-01-02T00:00:00Z");
    }
}
