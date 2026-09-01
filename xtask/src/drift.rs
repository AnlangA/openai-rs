use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cli::DriftArguments;
use crate::error::{Error, Result};
use crate::spec::SNAPSHOT_PATH;

const HTTP_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

#[derive(Debug, Clone)]
struct OperationMeta {
    method: String,
    path: String,
    deprecated: bool,
    required_params: BTreeSet<String>,
    optional_params: BTreeSet<String>,
    // name -> `in` (location), e.g. "query" / "header" / "path" / "cookie"
    param_locations: BTreeMap<String, String>,
    // Success status codes declared on responses, e.g. "200", "201"
    success_statuses: BTreeSet<String>,
}

#[derive(Debug, Default)]
pub struct DriftReport {
    pub from_source: String,
    pub to_source: String,
    pub from_operations_count: usize,
    pub to_operations_count: usize,
    pub from_schemas_count: usize,
    pub to_schemas_count: usize,
    pub lifecycle_changes: Vec<String>,
    pub breaking_changes: Vec<String>,
    pub additive_changes: Vec<String>,
}

impl DriftReport {
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.lifecycle_changes.is_empty()
            && self.breaking_changes.is_empty()
            && self.additive_changes.is_empty()
    }

    pub fn print_summary(&self) {
        println!("=== OpenAPI Drift Report ===");
        println!("From: {}", self.from_source);
        println!("To:   {}", self.to_source);
        println!(
            "Operations: {} -> {} | Schemas: {} -> {}",
            self.from_operations_count,
            self.to_operations_count,
            self.from_schemas_count,
            self.to_schemas_count
        );
        println!();

        println!(
            "--- Lifecycle Changes (D0013 / Deprecations / Sunsets: {}) ---",
            self.lifecycle_changes.len()
        );
        if self.lifecycle_changes.is_empty() {
            println!("  (none)");
        } else {
            for change in &self.lifecycle_changes {
                println!("  * {change}");
            }
        }
        println!();

        println!("--- Breaking Changes ({}) ---", self.breaking_changes.len());
        if self.breaking_changes.is_empty() {
            println!("  (none)");
        } else {
            for change in &self.breaking_changes {
                println!("  * {change}");
            }
        }
        println!();

        println!(
            "--- Additive / Unknown Changes ({}) ---",
            self.additive_changes.len()
        );
        if self.additive_changes.is_empty() {
            println!("  (none)");
        } else {
            for change in &self.additive_changes {
                println!("  * {change}");
            }
        }
    }
}

pub fn run(repository_root: &Path, arguments: &DriftArguments) -> Result<()> {
    let from_path = match &arguments.from {
        Some(path) => resolve_path(repository_root, path),
        None => repository_root.join(SNAPSHOT_PATH),
    };
    let to_path = resolve_path(repository_root, &arguments.to);

    let report = compute_drift(&from_path, &to_path)?;
    report.print_summary();
    Ok(())
}

fn resolve_path(repository_root: &Path, path_str: &str) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() || p.exists() {
        p.to_path_buf()
    } else {
        repository_root.join(p)
    }
}

pub fn compute_drift(from_path: &Path, to_path: &Path) -> Result<DriftReport> {
    let from_bytes = fs::read(from_path)
        .map_err(|source| Error::io("read from-spec for drift", from_path, source))?;
    let to_bytes =
        fs::read(to_path).map_err(|source| Error::io("read to-spec for drift", to_path, source))?;

    let from_doc: Value = serde_json::from_slice(&from_bytes).map_err(|source| Error::Json {
        path: from_path.to_path_buf(),
        source,
    })?;
    let to_doc: Value = serde_json::from_slice(&to_bytes).map_err(|source| Error::Json {
        path: to_path.to_path_buf(),
        source,
    })?;

    let from_ops = extract_operations(&from_doc)?;
    let to_ops = extract_operations(&to_doc)?;

    let from_schemas = extract_schema_names(&from_doc);
    let to_schemas = extract_schema_names(&to_doc);

    let mut report = DriftReport {
        from_source: from_path.display().to_string(),
        to_source: to_path.display().to_string(),
        from_operations_count: from_ops.len(),
        to_operations_count: to_ops.len(),
        from_schemas_count: from_schemas.len(),
        to_schemas_count: to_schemas.len(),
        lifecycle_changes: Vec::new(),
        breaking_changes: Vec::new(),
        additive_changes: Vec::new(),
    };

    // Analyze operations
    for (op_id, from_op) in &from_ops {
        match to_ops.get(op_id) {
            Some(to_op) => {
                // Check method/path changes
                if from_op.method != to_op.method || from_op.path != to_op.path {
                    report.breaking_changes.push(format!(
                        "operation `{op_id}` route changed: {} {} -> {} {}",
                        from_op.method.to_uppercase(),
                        from_op.path,
                        to_op.method.to_uppercase(),
                        to_op.path
                    ));
                }

                // Check deprecation / lifecycle
                if !from_op.deprecated && to_op.deprecated {
                    report.lifecycle_changes.push(format!(
                        "operation `{op_id}` ({}) was marked deprecated in OpenAPI",
                        to_op.path
                    ));
                }

                // Check success-status changes (200 <-> 201 flip etc.)
                if from_op.success_statuses != to_op.success_statuses {
                    report.breaking_changes.push(format!(
                        "operation `{op_id}` success status codes changed: [{}] -> [{}]",
                        from_op
                            .success_statuses
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", "),
                        to_op
                            .success_statuses
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }

                // Check parameter location (in) changes, e.g. query -> header
                for (param, to_loc) in &to_op.param_locations {
                    match from_op.param_locations.get(param) {
                        Some(from_loc) => {
                            if from_loc != to_loc {
                                report.breaking_changes.push(format!(
                                    "operation `{op_id}` parameter `{param}` moved from `{from_loc}` to `{to_loc}`"
                                ));
                            }
                        }
                        None => {
                            report.additive_changes.push(format!(
                                "operation `{op_id}` parameter `{param}` now declares location `{to_loc}`"
                            ));
                        }
                    }
                }

                // Check added required parameters (breaking)
                for req in &to_op.required_params {
                    if !from_op.required_params.contains(req) {
                        report.breaking_changes.push(format!(
                            "operation `{op_id}` added new required parameter `{req}`"
                        ));
                    }
                }

                // Check added optional parameters (additive)
                for opt in &to_op.optional_params {
                    if !from_op.optional_params.contains(opt)
                        && !from_op.required_params.contains(opt)
                    {
                        report.additive_changes.push(format!(
                            "operation `{op_id}` added optional parameter `{opt}`"
                        ));
                    }
                }
            }
            None => {
                // Operation was removed
                if is_sunset_or_omitted_family(&from_op.path) || from_op.deprecated {
                    report.lifecycle_changes.push(format!(
                        "operation `{op_id}` ({} {}) removed under lifecycle/sunset policy",
                        from_op.method.to_uppercase(),
                        from_op.path
                    ));
                } else {
                    report.breaking_changes.push(format!(
                        "operation `{op_id}` ({} {}) was removed",
                        from_op.method.to_uppercase(),
                        from_op.path
                    ));
                }
            }
        }
    }

    for (op_id, to_op) in &to_ops {
        if !from_ops.contains_key(op_id) {
            if is_sunset_or_omitted_family(&to_op.path) {
                report.lifecycle_changes.push(format!(
                    "new operation `{op_id}` in sunset/omitted family ({}) - keep omitted per D0013",
                    to_op.path
                ));
            } else {
                report.additive_changes.push(format!(
                    "new operation `{op_id}` ({} {})",
                    to_op.method.to_uppercase(),
                    to_op.path
                ));
            }
        }
    }

    // Analyze schemas
    for schema_name in &to_schemas {
        if !from_schemas.contains(schema_name) {
            report
                .additive_changes
                .push(format!("new schema `{schema_name}`"));
        }
    }

    for schema_name in &from_schemas {
        if !to_schemas.contains(schema_name) {
            if is_sunset_schema_family(schema_name) {
                report.lifecycle_changes.push(format!(
                    "schema `{schema_name}` removed under sunset policy"
                ));
            } else {
                report
                    .breaking_changes
                    .push(format!("schema `{schema_name}` was removed"));
            }
        }
    }

    Ok(report)
}

fn is_sunset_or_omitted_family(path: &str) -> bool {
    path.starts_with("/assistants")
        || path.starts_with("/threads")
        || path.starts_with("/videos")
        || path.starts_with("/prompts")
}

fn is_sunset_schema_family(name: &str) -> bool {
    name.starts_with("Assistant")
        || name.starts_with("Thread")
        || name.starts_with("Run")
        || name.starts_with("Video")
        || name.starts_with("Prompt")
}

fn extract_operations(doc: &Value) -> Result<BTreeMap<String, OperationMeta>> {
    let mut operations = BTreeMap::new();
    let Some(paths) = doc.get("paths").and_then(Value::as_object) else {
        return Ok(operations);
    };

    for (path, path_item) in paths {
        let Some(path_obj) = path_item.as_object() else {
            continue;
        };
        for &method in &HTTP_METHODS {
            let Some(op) = path_obj.get(method).and_then(Value::as_object) else {
                continue;
            };
            let Some(op_id) = op.get("operationId").and_then(Value::as_str) else {
                continue;
            };
            let deprecated = op
                .get("deprecated")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            let mut required_params = BTreeSet::new();
            let mut optional_params = BTreeSet::new();
            let mut param_locations = BTreeMap::new();

            // Operation-level parameters plus shared path-item parameters
            // (OpenAPI 3: path-item `parameters` apply to every operation).
            let mut all_params: Vec<&Value> = Vec::new();
            if let Some(params) = op.get("parameters").and_then(Value::as_array) {
                all_params.extend(params.iter());
            }
            if let Some(params) = path_obj.get("parameters").and_then(Value::as_array) {
                all_params.extend(params.iter());
            }
            for p in all_params {
                if let Some(p_name) = p.get("name").and_then(Value::as_str) {
                    let is_req = p.get("required").and_then(Value::as_bool).unwrap_or(false);
                    if is_req {
                        required_params.insert(p_name.to_owned());
                    } else {
                        optional_params.insert(p_name.to_owned());
                    }
                    if let Some(location) = p.get("in").and_then(Value::as_str) {
                        param_locations.insert(p_name.to_owned(), location.to_owned());
                    }
                }
            }

            // Success responses: 2xx status keys under `responses`.
            let mut success_statuses = BTreeSet::new();
            if let Some(responses) = op.get("responses").and_then(Value::as_object) {
                for status in responses.keys() {
                    if status.starts_with("2") {
                        success_statuses.insert(status.clone());
                    }
                }
            }

            operations.insert(
                op_id.to_owned(),
                OperationMeta {
                    method: method.to_owned(),
                    path: path.clone(),
                    deprecated,
                    required_params,
                    optional_params,
                    param_locations,
                    success_statuses,
                },
            );
        }
    }
    Ok(operations)
}

fn extract_schema_names(doc: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Some(schemas) = doc
        .pointer("/components/schemas")
        .and_then(Value::as_object)
    {
        for name in schemas.keys() {
            names.insert(name.clone());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_same_spec_is_zero_diff() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root");
        let snapshot = repo_root.join(SNAPSHOT_PATH);
        let report = compute_drift(&snapshot, &snapshot).expect("compute drift");
        assert!(report.is_empty());
        assert_eq!(report.from_operations_count, report.to_operations_count);
        assert_eq!(report.from_schemas_count, report.to_schemas_count);
    }

    #[test]
    fn implemented_eval_and_fine_tuning_families_are_not_lifecycle_only() {
        assert!(!is_sunset_or_omitted_family("/evals"));
        assert!(!is_sunset_or_omitted_family("/fine_tuning/jobs"));
        assert!(is_sunset_or_omitted_family("/assistants"));
        assert!(is_sunset_or_omitted_family("/videos"));
        assert!(is_sunset_or_omitted_family("/prompts"));
        assert!(!is_sunset_schema_family("Eval"));
        assert!(!is_sunset_schema_family("FineTuningJob"));
        assert!(is_sunset_schema_family("Assistant"));
    }

    fn spec_with_operation(
        status_from: &str,
        status_to: &str,
        in_from: &str,
        in_to: &str,
    ) -> (String, String) {
        let from = serde_json::json!({
            "paths": {
                "/files/{file_id}": {
                    "get": {
                        "operationId": "retrieveFile",
                        "parameters": [
                            {"name": "file_id", "in": "path", "required": true},
                            {"name": "purpose", "in": in_from, "required": false}
                        ],
                        "responses": {status_from: {"description": "ok"}}
                    }
                }
            }
        })
        .to_string();
        let to = serde_json::json!({
            "paths": {
                "/files/{file_id}": {
                    "get": {
                        "operationId": "retrieveFile",
                        "parameters": [
                            {"name": "file_id", "in": "path", "required": true},
                            {"name": "purpose", "in": in_to, "required": false}
                        ],
                        "responses": {status_to: {"description": "ok"}}
                    }
                }
            }
        })
        .to_string();
        (from, to)
    }

    #[test]
    fn status_code_and_param_location_changes_are_reported() {
        let (from, to) = spec_with_operation("200", "201", "query", "header");
        let from_path = std::env::temp_dir().join("drift_from.json");
        let to_path = std::env::temp_dir().join("drift_to.json");
        fs::write(&from_path, &from).expect("write from spec");
        fs::write(&to_path, &to).expect("write to spec");

        let report = compute_drift(&from_path, &to_path).expect("compute drift");

        assert!(
            report
                .breaking_changes
                .iter()
                .any(|c| c.contains("retrieveFile") && c.contains("200") && c.contains("201")),
            "expected success-status change in breaking_changes, got {:?}",
            report.breaking_changes
        );
        assert!(
            report
                .breaking_changes
                .iter()
                .any(|c| c.contains("purpose") && c.contains("query") && c.contains("header")),
            "expected parameter location change in breaking_changes, got {:?}",
            report.breaking_changes
        );
    }

    #[test]
    fn identical_status_and_location_produce_no_changes() {
        let (from, to) = spec_with_operation("200", "200", "query", "query");
        let from_path = std::env::temp_dir().join("drift_from_same.json");
        let to_path = std::env::temp_dir().join("drift_to_same.json");
        fs::write(&from_path, &from).expect("write from spec");
        fs::write(&to_path, &to).expect("write to spec");

        let report = compute_drift(&from_path, &to_path).expect("compute drift");
        assert!(
            !report
                .breaking_changes
                .iter()
                .any(|c| c.contains("retrieveFile") || c.contains("purpose")),
            "unexpected changes: {:?}",
            report.breaking_changes
        );
    }
}
