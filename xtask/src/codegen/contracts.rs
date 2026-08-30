use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::RenderedArtifact;
use crate::error::{Error, Result};
use crate::spec::{PINNED_REVISION, PINNED_SHA256, SNAPSHOT_PATH};

const OPERATIONS_PATH: &str = "spec/contracts/operations.json";
const DISCRIMINATORS_PATH: &str = "spec/contracts/discriminators.json";
const NULLABILITY_PATH: &str = "spec/contracts/nullability.json";
const SCHEMA_IR_PATH: &str = "spec/contracts/schema-ir.json";
const NON_REST_PATH: &str = "spec/contracts/non-rest-implementation.json";
const IMPLEMENTATION_PATH: &str = "spec/contracts/implementation.toml";
const EXPECTED_CLIENT_OPERATIONS: usize = 288;
const EXPECTED_WEBHOOK_OPERATIONS: usize = 18;
const HTTP_METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];
const COMPLEX_SCHEMAS: [&str; 13] = [
    "InputItem",
    "Item",
    "OutputItem",
    "Tool",
    "ToolChoiceParam",
    "ResponseStreamEvent",
    "RealtimeServerEvent",
    "RealtimeClientEvent",
    "Response",
    "CreateChatCompletionRequest",
    "ChatCompletionRequestMessage",
    "CreateFineTuningJobRequest",
    "CreateThreadAndRunRequest",
];

#[derive(Serialize)]
struct SourceIdentity {
    revision: &'static str,
    sha256: &'static str,
}

#[derive(Serialize)]
struct OperationsArtifact {
    schema_version: u32,
    source: SourceIdentity,
    counts: OperationCounts,
    client_operations: Vec<OperationContract>,
    webhook_operations: Vec<OperationContract>,
}

#[derive(Serialize)]
struct NonRestArtifact {
    schema_version: u32,
    source: SourceIdentity,
    count: usize,
    implementation_statuses: BTreeMap<String, usize>,
    verified_units: usize,
    units: Vec<NonRestImplementation>,
}

#[derive(Serialize)]
struct OperationCounts {
    client: usize,
    webhook: usize,
    total: usize,
    implementation_statuses: BTreeMap<String, usize>,
    webhook_implementation_statuses: BTreeMap<String, usize>,
    verified_units: usize,
}

#[derive(Serialize)]
struct OperationContract {
    method: String,
    path: String,
    operation_id: Option<String>,
    request: RequestContract,
    response: ResponseContract,
    lifecycle: String,
    feature: String,
    implementation: ImplementationStatus,
    manual_overrides: Vec<&'static str>,
}

#[derive(Serialize)]
struct RequestContract {
    parameters: Vec<ParameterContract>,
    body: Option<BodyContract>,
    mode: String,
}

#[derive(Serialize)]
struct ParameterContract {
    name: Option<String>,
    location: Option<String>,
    required: bool,
    style: Option<String>,
    explode: Option<bool>,
    allow_reserved: Option<bool>,
    reference: Option<String>,
    schema_refs: Vec<String>,
}

#[derive(Serialize)]
struct BodyContract {
    required: bool,
    content_types: Vec<String>,
    schema_refs: Vec<String>,
}

#[derive(Serialize)]
struct ResponseContract {
    success_statuses: Vec<String>,
    content_types: Vec<String>,
    schema_refs: Vec<String>,
    mode: String,
}

#[derive(Clone, Serialize)]
struct ImplementationStatus {
    status: String,
    milestone: String,
    units: Vec<String>,
    tests: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry_group: Option<String>,
}

#[derive(Clone, Serialize)]
struct NonRestImplementation {
    id: String,
    #[serde(flatten)]
    implementation: ImplementationStatus,
}

struct ImplementationRegistry {
    operations: BTreeMap<String, ImplementationStatus>,
    non_rest: Vec<NonRestImplementation>,
}

#[derive(Serialize)]
struct DiscriminatorsArtifact {
    schema_version: u32,
    source: SourceIdentity,
    count: usize,
    entries: Vec<DiscriminatorEntry>,
}

#[derive(Serialize)]
struct DiscriminatorEntry {
    schema: String,
    pointer: String,
    property_name: String,
    mapping: BTreeMap<String, String>,
    one_of_branches: usize,
    any_of_branches: usize,
    branch_refs: Vec<String>,
}

#[derive(Serialize)]
struct NullabilityArtifact {
    schema_version: u32,
    source: SourceIdentity,
    count: usize,
    counts_by_encoding: BTreeMap<String, usize>,
    entries: Vec<NullabilityEntry>,
}

#[derive(Serialize)]
struct NullabilityEntry {
    schema: String,
    pointer: String,
    encodings: Vec<String>,
}

#[derive(Serialize)]
struct SchemaIrArtifact {
    schema_version: u32,
    source: SourceIdentity,
    selection: &'static str,
    count: usize,
    schemas: Vec<LoweredSchema>,
}

#[derive(Serialize)]
struct LoweredSchema {
    name: String,
    pointer: String,
    metrics: SchemaMetrics,
    node: LoweredNode,
}

#[derive(Default, Serialize)]
struct SchemaMetrics {
    node_count: usize,
    reference_count: usize,
    nullable_node_count: usize,
    one_of_branch_count: usize,
    any_of_branch_count: usize,
    all_of_branch_count: usize,
    max_inline_depth: usize,
}

#[derive(Serialize)]
struct LoweredNode {
    kind: String,
    types: Vec<String>,
    reference: Option<String>,
    nullable: bool,
    const_value: Option<Value>,
    enum_values: Vec<Value>,
    required: Vec<String>,
    properties: BTreeMap<String, LoweredNode>,
    items: Option<Box<LoweredNode>>,
    one_of: Vec<LoweredNode>,
    any_of: Vec<LoweredNode>,
    all_of: Vec<LoweredNode>,
    discriminator: Option<LoweredDiscriminator>,
    constraints: BTreeMap<String, Value>,
    unmodeled_keywords: Vec<String>,
}

#[derive(Serialize)]
struct LoweredDiscriminator {
    property_name: String,
    mapping: BTreeMap<String, String>,
}

pub(super) fn render(repository_root: &Path) -> Result<Vec<RenderedArtifact>> {
    let snapshot_path = repository_root.join(SNAPSHOT_PATH);
    let bytes = fs::read(&snapshot_path)
        .map_err(|source| Error::io("read pinned OpenAPI for codegen", &snapshot_path, source))?;
    let document: Value = serde_json::from_slice(&bytes).map_err(|source| Error::Json {
        path: snapshot_path,
        source,
    })?;

    let implementation_registry = load_implementation_registry(repository_root)?;
    let operations = build_operations(&document, &implementation_registry.operations)?;
    let non_rest = build_non_rest(&implementation_registry.non_rest);
    let discriminators = build_discriminators(&document)?;
    let nullability = build_nullability(&document)?;
    let schema_ir = build_schema_ir(&document)?;

    Ok(vec![
        artifact(OPERATIONS_PATH, &operations)?,
        artifact(NON_REST_PATH, &non_rest)?,
        artifact(DISCRIMINATORS_PATH, &discriminators)?,
        artifact(NULLABILITY_PATH, &nullability)?,
        artifact(SCHEMA_IR_PATH, &schema_ir)?,
    ])
}

fn build_non_rest(units: &[NonRestImplementation]) -> NonRestArtifact {
    let mut implementation_statuses = BTreeMap::new();
    let mut verified_units = BTreeSet::new();
    for unit in units {
        *implementation_statuses
            .entry(unit.implementation.status.clone())
            .or_insert(0) += 1;
        if unit.implementation.status == "verified" {
            verified_units.extend(unit.implementation.units.iter().cloned());
        }
    }
    NonRestArtifact {
        schema_version: 1,
        source: source_identity(),
        count: units.len(),
        implementation_statuses,
        verified_units: verified_units.len(),
        units: units.to_vec(),
    }
}

fn source_identity() -> SourceIdentity {
    SourceIdentity {
        revision: PINNED_REVISION,
        sha256: PINNED_SHA256,
    }
}

fn artifact(path: &str, value: &impl Serialize) -> Result<RenderedArtifact> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| {
        Error::message(format!("serialize generated artifact {path}: {source}"))
    })?;
    bytes.push(b'\n');
    Ok(RenderedArtifact {
        relative_path: PathBuf::from(path),
        bytes,
    })
}

fn build_operations(
    document: &Value,
    implementation_registry: &BTreeMap<String, ImplementationStatus>,
) -> Result<OperationsArtifact> {
    let client_operations = collect_operations(document, "/paths", false, implementation_registry)?;
    let webhook_operations =
        collect_operations(document, "/webhooks", true, implementation_registry)?;
    if client_operations.len() != EXPECTED_CLIENT_OPERATIONS
        || webhook_operations.len() != EXPECTED_WEBHOOK_OPERATIONS
    {
        return Err(Error::message(format!(
            "operation lowering count mismatch: expected {EXPECTED_CLIENT_OPERATIONS}+{EXPECTED_WEBHOOK_OPERATIONS}, found {}+{}",
            client_operations.len(),
            webhook_operations.len()
        )));
    }

    let known_labels = client_operations
        .iter()
        .filter_map(|operation| operation.operation_id.clone())
        .chain(
            webhook_operations
                .iter()
                .map(|operation| webhook_registry_label(&operation.path, &operation.method)),
        )
        .collect::<BTreeSet<_>>();
    let orphaned = implementation_registry
        .keys()
        .filter(|operation_id| !known_labels.contains(*operation_id))
        .cloned()
        .collect::<Vec<_>>();
    if !orphaned.is_empty() {
        return Err(Error::message(format!(
            "implementation registry contains unknown operation ids: {}",
            orphaned.join(", ")
        )));
    }
    let mut implementation_statuses = BTreeMap::new();
    let mut webhook_implementation_statuses = BTreeMap::new();
    let mut verified_units = BTreeSet::new();
    for operation in &client_operations {
        *implementation_statuses
            .entry(operation.implementation.status.clone())
            .or_insert(0) += 1;
        if operation.implementation.status == "verified" {
            verified_units.extend(operation.implementation.units.iter().cloned());
        }
    }
    for operation in &webhook_operations {
        *webhook_implementation_statuses
            .entry(operation.implementation.status.clone())
            .or_insert(0) += 1;
        if operation.implementation.status == "verified" {
            verified_units.extend(operation.implementation.units.iter().cloned());
        }
    }

    Ok(OperationsArtifact {
        schema_version: 1,
        source: source_identity(),
        counts: OperationCounts {
            client: client_operations.len(),
            webhook: webhook_operations.len(),
            total: client_operations.len() + webhook_operations.len(),
            implementation_statuses,
            webhook_implementation_statuses,
            verified_units: verified_units.len(),
        },
        client_operations,
        webhook_operations,
    })
}

fn collect_operations(
    document: &Value,
    collection_pointer: &str,
    webhook: bool,
    implementation_registry: &BTreeMap<String, ImplementationStatus>,
) -> Result<Vec<OperationContract>> {
    let collection = object_at(document, collection_pointer)?;
    let mut operations = Vec::new();

    for (path, path_item) in collection {
        let path_item = path_item.as_object().ok_or_else(|| {
            Error::message(format!("{collection_pointer}/{path} must be an object"))
        })?;
        for method in HTTP_METHODS {
            let Some(operation) = path_item.get(method) else {
                continue;
            };
            let operation = operation.as_object().ok_or_else(|| {
                Error::message(format!(
                    "{collection_pointer}/{path}/{method} must be an object"
                ))
            })?;
            let operation_id = operation
                .get("operationId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if operation_id.is_none() && !webhook {
                return Err(Error::message(format!(
                    "{collection_pointer}/{path}/{method} is missing operationId"
                )));
            }
            let operation_label = operation_id
                .as_deref()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("webhook:{path}:{method}"));
            if operation_label.is_empty() {
                return Err(Error::message(format!(
                    "{collection_pointer}/{path}/{method} has an empty operationId"
                )));
            }
            let lifecycle = initial_lifecycle(path, operation).to_owned();
            let feature = initial_feature(path, webhook, &lifecycle).to_owned();

            operations.push(OperationContract {
                method: method.to_ascii_uppercase(),
                path: path.clone(),
                operation_id: operation_id.clone(),
                request: request_contract(document, path_item, operation)?,
                response: response_contract(document, operation, &operation_label)?,
                lifecycle,
                feature,
                implementation: implementation_registry
                    .get(&operation_label)
                    .cloned()
                    .unwrap_or_else(planned_implementation),
                manual_overrides: operation_id
                    .as_deref()
                    .map(operation_override_ids)
                    .unwrap_or_default(),
            });
        }
    }

    operations.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| method_rank(&left.method).cmp(&method_rank(&right.method)))
            .then_with(|| left.operation_id.cmp(&right.operation_id))
    });
    Ok(operations)
}

fn webhook_registry_label(path: &str, method: &str) -> String {
    format!("webhook:{path}:{}", method.to_ascii_lowercase())
}

fn planned_implementation() -> ImplementationStatus {
    ImplementationStatus {
        status: "planned".to_owned(),
        milestone: "unassigned".to_owned(),
        units: Vec::new(),
        tests: Vec::new(),
        registry_group: None,
    }
}

#[derive(Deserialize)]
struct ImplementationToml {
    schema_version: u32,
    #[serde(default)]
    operations: Vec<OperationToml>,
    #[serde(default)]
    groups: Vec<GroupToml>,
    #[serde(default)]
    non_rest: Vec<NonRestToml>,
}

#[derive(Deserialize)]
struct OperationToml {
    operation_id: String,
    status: String,
    milestone: String,
    units: Vec<String>,
    tests: Vec<String>,
}

#[derive(Deserialize)]
struct GroupToml {
    name: String,
    operation_ids: Vec<String>,
    status: String,
    milestone: String,
    units: Vec<String>,
    tests: Vec<String>,
}

#[derive(Deserialize)]
struct NonRestToml {
    id: String,
    status: String,
    milestone: String,
    units: Vec<String>,
    tests: Vec<String>,
}

fn load_implementation_registry(repository_root: &Path) -> Result<ImplementationRegistry> {
    let path = repository_root.join(IMPLEMENTATION_PATH);
    let input = fs::read_to_string(&path)
        .map_err(|source| Error::io("read implementation registry", &path, source))?;
    let toml_data: ImplementationToml = toml::from_str(&input).map_err(|source| {
        Error::message(format!(
            "invalid TOML in {}: {source}",
            path.display()
        ))
    })?;

    if toml_data.schema_version != 1 {
        return Err(Error::message(format!(
            "{} must declare schema_version = 1",
            path.display()
        )));
    }

    let mut operations = BTreeMap::new();
    let mut non_rest = Vec::new();
    let mut non_rest_ids = BTreeSet::new();

    for op in toml_data.operations {
        let status = validate_implementation_fields(
            op.status,
            op.milestone,
            op.units,
            op.tests,
            &format!("operation {}", op.operation_id),
            None,
        )?;
        insert_operation(&mut operations, op.operation_id, status)?;
    }

    for group in toml_data.groups {
        if group.operation_ids.is_empty() {
            return Err(Error::message(format!(
                "implementation group `{}` must list at least one operation id",
                group.name
            )));
        }
        let status = validate_implementation_fields(
            group.status,
            group.milestone,
            group.units,
            group.tests,
            &format!("group {}", group.name),
            Some(group.name.clone()),
        )?;
        for operation_id in group.operation_ids {
            let mut expanded = status.clone();
            expanded.units = expanded
                .units
                .iter()
                .map(|unit| unit.replace("{operation_id}", &operation_id))
                .collect();
            insert_operation(&mut operations, operation_id, expanded)?;
        }
    }

    for nr in toml_data.non_rest {
        let status = validate_implementation_fields(
            nr.status,
            nr.milestone,
            nr.units,
            nr.tests,
            &format!("non-REST unit {}", nr.id),
            None,
        )?;
        if !non_rest_ids.insert(nr.id.clone()) {
            return Err(Error::message(format!(
                "duplicate non-REST implementation id `{}`",
                nr.id
            )));
        }
        non_rest.push(NonRestImplementation {
            id: nr.id,
            implementation: status,
        });
    }

    non_rest.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(ImplementationRegistry {
        operations,
        non_rest,
    })
}

fn validate_implementation_fields(
    status: String,
    milestone: String,
    units: Vec<String>,
    tests: Vec<String>,
    label: &str,
    registry_group: Option<String>,
) -> Result<ImplementationStatus> {
    if !is_valid_implementation_status(&status) {
        return Err(Error::message(format!(
            "implementation status for {label} must be planned, partial, implemented, verified, historical, quarantined, or omitted"
        )));
    }
    if milestone.is_empty() || units.is_empty() || tests.is_empty() {
        return Err(Error::message(format!(
            "implementation registry {label} requires non-empty milestone, units, and tests"
        )));
    }
    Ok(ImplementationStatus {
        status,
        milestone,
        units,
        tests,
        registry_group,
    })
}

fn is_valid_implementation_status(status: &str) -> bool {
    matches!(
        status,
        "planned"
            | "partial"
            | "implemented"
            | "verified"
            | "historical"
            | "quarantined"
            | "omitted"
    )
}

fn insert_operation(
    registry: &mut BTreeMap<String, ImplementationStatus>,
    operation_id: String,
    status: ImplementationStatus,
) -> Result<()> {
    if operation_id.is_empty() {
        return Err(Error::message(
            "implementation registry operation id must not be empty",
        ));
    }
    if registry.insert(operation_id.clone(), status).is_some() {
        return Err(Error::message(format!(
            "duplicate implementation registry operation `{operation_id}`"
        )));
    }
    Ok(())
}

fn request_contract(
    document: &Value,
    path_item: &Map<String, Value>,
    operation: &Map<String, Value>,
) -> Result<RequestContract> {
    let mut parameters = Vec::new();
    for parameter in path_item
        .get("parameters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            operation
                .get("parameters")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
    {
        parameters.push(parameter_contract(document, parameter)?);
    }
    parameters.sort_by(|left, right| {
        left.location
            .cmp(&right.location)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.reference.cmp(&right.reference))
    });

    let body = operation
        .get("requestBody")
        .map(|body| body_contract(document, body))
        .transpose()?;
    let mode = body
        .as_ref()
        .map(|body| request_mode(&body.content_types))
        .unwrap_or("none")
        .to_owned();

    Ok(RequestContract {
        parameters,
        body,
        mode,
    })
}

fn parameter_contract(document: &Value, parameter: &Value) -> Result<ParameterContract> {
    let reference = parameter
        .get("$ref")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let resolved = resolve_local_ref(document, parameter)?;
    let object = resolved
        .as_object()
        .ok_or_else(|| Error::message("operation parameter must resolve to an object"))?;
    let mut schema_refs = BTreeSet::new();
    if let Some(schema) = object.get("schema") {
        collect_refs(schema, &mut schema_refs);
    }

    Ok(ParameterContract {
        name: object
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        location: object.get("in").and_then(Value::as_str).map(str::to_owned),
        required: object
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        style: object
            .get("style")
            .and_then(Value::as_str)
            .map(str::to_owned),
        explode: object.get("explode").and_then(Value::as_bool),
        allow_reserved: object.get("allowReserved").and_then(Value::as_bool),
        reference,
        schema_refs: schema_refs.into_iter().collect(),
    })
}

fn body_contract(document: &Value, body: &Value) -> Result<BodyContract> {
    let resolved = resolve_local_ref(document, body)?;
    let object = resolved
        .as_object()
        .ok_or_else(|| Error::message("requestBody must resolve to an object"))?;
    let content = object
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::message("requestBody must contain a content object"))?;
    let content_types = content.keys().cloned().collect::<Vec<_>>();
    let mut schema_refs = BTreeSet::new();
    for media in content.values() {
        if let Some(schema) = media.get("schema") {
            collect_refs(schema, &mut schema_refs);
        }
    }

    Ok(BodyContract {
        required: object
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        content_types,
        schema_refs: schema_refs.into_iter().collect(),
    })
}

fn response_contract(
    document: &Value,
    operation: &Map<String, Value>,
    operation_id: &str,
) -> Result<ResponseContract> {
    let responses = operation
        .get("responses")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::message(format!("operation {operation_id} is missing responses")))?;
    let mut statuses = BTreeSet::new();
    let mut content_types = BTreeSet::new();
    let mut schema_refs = BTreeSet::new();
    let mut has_empty_success = false;

    for (status, response) in responses {
        if !is_success_status(status) {
            continue;
        }
        statuses.insert(status.clone());
        let resolved = resolve_local_ref(document, response)?;
        let object = resolved.as_object().ok_or_else(|| {
            Error::message(format!(
                "success response {status} for {operation_id} must be an object"
            ))
        })?;
        let Some(content) = object.get("content").and_then(Value::as_object) else {
            has_empty_success = true;
            continue;
        };
        if content.is_empty() {
            has_empty_success = true;
        }
        for (content_type, media) in content {
            content_types.insert(content_type.clone());
            if let Some(schema) = media.get("schema") {
                collect_refs(schema, &mut schema_refs);
            }
        }
    }
    if statuses.is_empty() {
        return Err(Error::message(format!(
            "operation {operation_id} has no declared success response"
        )));
    }

    let mut content_types = content_types.into_iter().collect::<Vec<_>>();
    let mut mode = response_mode(&content_types, has_empty_success).to_owned();
    if operation_id == "downloadFile" {
        content_types = vec!["application/octet-stream".to_owned()];
        mode = "raw".to_owned();
    }

    Ok(ResponseContract {
        success_statuses: statuses.into_iter().collect(),
        content_types,
        schema_refs: schema_refs.into_iter().collect(),
        mode,
    })
}

fn initial_feature(path: &str, webhook: bool, lifecycle: &str) -> &'static str {
    if webhook {
        "webhooks"
    } else if path.starts_with("/chatkit/") {
        "beta-chatkit"
    } else if path.contains("?beta=true") {
        "beta-responses"
    } else if path.starts_with("/fine_tuning/alpha/") {
        "alpha-graders"
    } else if path == "/completions" {
        "legacy-completions"
    } else if matches!(
        path,
        "/realtime/sessions" | "/realtime/transcription_sessions"
    ) {
        "legacy-realtime"
    } else if path.starts_with("/audio/voice_consents") || path == "/audio/voices" {
        "custom-voice"
    } else if path == "/images/variations" {
        "quarantine"
    } else if path == "/assistants"
        || path.starts_with("/assistants/")
        || path == "/threads"
        || path.starts_with("/threads/")
    {
        "legacy-assistants"
    } else if path == "/videos" || path.starts_with("/videos/") {
        "legacy-video"
    } else if path == "/organization"
        || path.starts_with("/organization/")
        || path.starts_with("/projects/")
        || (path.starts_with("/fine_tuning/checkpoints/") && path.contains("/permissions"))
    {
        "admin"
    } else if path.starts_with("/realtime") {
        "realtime"
    } else if lifecycle == "deprecated" || lifecycle == "legacy" {
        "legacy"
    } else {
        "default"
    }
}

fn initial_lifecycle(path: &str, operation: &Map<String, Value>) -> &'static str {
    if path == "/threads" || path.starts_with("/threads/") {
        "sunset"
    } else if operation
        .get("deprecated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "deprecated"
    } else if path.starts_with("/chatkit/") || path.contains("?beta=true") {
        "beta"
    } else if path.starts_with("/fine_tuning/alpha/") {
        "alpha"
    } else if path == "/completions"
        || matches!(
            path,
            "/realtime/sessions" | "/realtime/transcription_sessions"
        )
    {
        "legacy"
    } else if path.starts_with("/audio/voice_consents") || path == "/audio/voices" {
        "access-controlled"
    } else if path == "/images/variations" {
        "quarantined"
    } else {
        "active"
    }
}

fn operation_override_ids(operation_id: &str) -> Vec<&'static str> {
    match operation_id {
        "downloadFile" => vec!["OVR-0001"],
        "createFile" => vec!["OVR-0002"],
        "createUpload" => vec!["OVR-0004"],
        "listFiles" => vec!["OVR-0006"],
        _ => Vec::new(),
    }
}

fn request_mode(content_types: &[String]) -> &'static str {
    if content_types.len() != 1 {
        return "mixed";
    }
    match content_types[0].as_str() {
        "application/json" => "json",
        "multipart/form-data" => "multipart",
        "application/x-www-form-urlencoded" => "form",
        _ => "raw",
    }
}

fn response_mode(content_types: &[String], has_empty: bool) -> &'static str {
    if content_types.is_empty() {
        return "empty";
    }
    if content_types
        .iter()
        .any(|content| content == "text/event-stream")
    {
        return "sse";
    }
    let all_json = content_types.iter().all(|content| {
        content == "application/json"
            || content.ends_with("+json")
            || content == "application/problem+json"
    });
    if all_json {
        if has_empty { "empty_or_json" } else { "json" }
    } else if content_types.len() == 1 {
        "raw"
    } else {
        "dynamic"
    }
}

fn is_success_status(status: &str) -> bool {
    status.len() == 3
        && status.starts_with('2')
        && status
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_digit() || byte == b'X')
}

fn method_rank(method: &str) -> usize {
    HTTP_METHODS
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(method))
        .unwrap_or(HTTP_METHODS.len())
}

fn build_discriminators(document: &Value) -> Result<DiscriminatorsArtifact> {
    let schemas = object_at(document, "/components/schemas")?;
    let mut entries = Vec::new();
    for (schema_name, schema) in schemas {
        let pointer = format!("#/components/schemas/{}", escape_json_pointer(schema_name));
        walk_schema_nodes(schema, &pointer, &mut |node, node_pointer| {
            let Some(discriminator) = node.get("discriminator").and_then(Value::as_object) else {
                return;
            };
            let Some(property_name) = discriminator.get("propertyName").and_then(Value::as_str)
            else {
                return;
            };
            let mapping = discriminator
                .get("mapping")
                .and_then(Value::as_object)
                .map(|mapping| {
                    mapping
                        .iter()
                        .filter_map(|(key, value)| {
                            value.as_str().map(|value| (key.clone(), value.to_owned()))
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default();
            let mut branch_refs = BTreeSet::new();
            for branch in node
                .get("oneOf")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .chain(
                    node.get("anyOf")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten(),
                )
            {
                if let Some(reference) = branch.get("$ref").and_then(Value::as_str) {
                    branch_refs.insert(reference.to_owned());
                }
            }
            entries.push(DiscriminatorEntry {
                schema: schema_name.clone(),
                pointer: node_pointer.to_owned(),
                property_name: property_name.to_owned(),
                mapping,
                one_of_branches: node
                    .get("oneOf")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0),
                any_of_branches: node
                    .get("anyOf")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0),
                branch_refs: branch_refs.into_iter().collect(),
            });
        });
    }
    entries.sort_by(|left, right| left.pointer.cmp(&right.pointer));

    Ok(DiscriminatorsArtifact {
        schema_version: 1,
        source: source_identity(),
        count: entries.len(),
        entries,
    })
}

fn build_nullability(document: &Value) -> Result<NullabilityArtifact> {
    let schemas = object_at(document, "/components/schemas")?;
    let mut entries = Vec::new();
    let mut counts = BTreeMap::new();

    for (schema_name, schema) in schemas {
        let pointer = format!("#/components/schemas/{}", escape_json_pointer(schema_name));
        walk_schema_nodes(schema, &pointer, &mut |node, node_pointer| {
            let encodings = nullability_encodings(node);
            if encodings.is_empty() {
                return;
            }
            for encoding in &encodings {
                *counts.entry(encoding.clone()).or_insert(0) += 1;
            }
            entries.push(NullabilityEntry {
                schema: schema_name.clone(),
                pointer: node_pointer.to_owned(),
                encodings,
            });
        });
    }
    entries.sort_by(|left, right| left.pointer.cmp(&right.pointer));

    Ok(NullabilityArtifact {
        schema_version: 1,
        source: source_identity(),
        count: entries.len(),
        counts_by_encoding: counts,
        entries,
    })
}

fn nullability_encodings(node: &Value) -> Vec<String> {
    let mut encodings = BTreeSet::new();
    if node.get("nullable").and_then(Value::as_bool) == Some(true) {
        encodings.insert("legacy_nullable".to_owned());
    }
    match node.get("type") {
        Some(Value::String(value)) if value == "null" => {
            encodings.insert("type_null".to_owned());
        }
        Some(Value::Array(values)) if values.iter().any(|value| value == "null") => {
            encodings.insert("type_union_null".to_owned());
        }
        _ => {}
    }
    if node
        .get("oneOf")
        .and_then(Value::as_array)
        .is_some_and(|branches| branches.iter().any(is_null_schema))
    {
        encodings.insert("one_of_null".to_owned());
    }
    if node
        .get("anyOf")
        .and_then(Value::as_array)
        .is_some_and(|branches| branches.iter().any(is_null_schema))
    {
        encodings.insert("any_of_null".to_owned());
    }
    encodings.into_iter().collect()
}

fn is_null_schema(node: &Value) -> bool {
    node.get("type").and_then(Value::as_str) == Some("null")
        || node.get("const") == Some(&Value::Null)
        || node
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|values| values.len() == 1 && values[0].is_null())
}

fn build_schema_ir(document: &Value) -> Result<SchemaIrArtifact> {
    let schemas = object_at(document, "/components/schemas")?;
    let mut lowered = Vec::new();
    let mut selected_names = BTreeSet::new();

    for name in COMPLEX_SCHEMAS {
        selected_names.insert(name.to_string());
    }

    for (schema_name, schema) in schemas {
        if is_schema_ir_target(schema_name, schema) {
            selected_names.insert(schema_name.clone());
        }
    }

    for schema_name in selected_names {
        let schema = schemas.get(&schema_name).ok_or_else(|| {
            Error::message(format!(
                "selected schema `{schema_name}` is missing"
            ))
        })?;
        let pointer = format!("#/components/schemas/{}", escape_json_pointer(&schema_name));
        let mut metrics = SchemaMetrics::default();
        collect_metrics(schema, 0, &mut metrics);
        lowered.push(LoweredSchema {
            name: schema_name,
            pointer,
            metrics,
            node: lower_node(schema)?,
        });
    }

    Ok(SchemaIrArtifact {
        schema_version: 1,
        source: source_identity(),
        selection: "tagged unions, event streams, and high-complexity proof schemas; references remain local and unresolved",
        count: lowered.len(),
        schemas: lowered,
    })
}

fn is_schema_ir_target(name: &str, schema: &Value) -> bool {
    if COMPLEX_SCHEMAS.contains(&name) {
        return true;
    }
    let mut has_discriminator = false;
    let pointer = format!("#/components/schemas/{}", escape_json_pointer(name));
    walk_schema_nodes(schema, &pointer, &mut |node, _| {
        if node.get("discriminator").and_then(Value::as_object).is_some() {
            has_discriminator = true;
        }
    });
    if has_discriminator {
        return true;
    }

    if (name.starts_with("Response")
        && (name.contains("Event") || name.contains("Item") || name.contains("Stream")))
        || (name.starts_with("Realtime")
            && (name.contains("Event") || name.contains("Item") || name.contains("Session")))
        || (name.starts_with("ChatCompletion")
            && (name.contains("Chunk")
                || name.contains("Delta")
                || name.contains("Message")
                || name.contains("Stream")))
    {
        return true;
    }

    false
}

fn lower_node(node: &Value) -> Result<LoweredNode> {
    let object = node
        .as_object()
        .ok_or_else(|| Error::message("schema node must be an object"))?;
    let types = schema_types(node);
    let reference = object
        .get("$ref")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let one_of = lower_array(object.get("oneOf"))?;
    let any_of = lower_array(object.get("anyOf"))?;
    let all_of = lower_array(object.get("allOf"))?;
    let properties = object
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(name, value)| Ok((name.clone(), lower_node(value)?)))
                .collect::<Result<BTreeMap<_, _>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let items = object
        .get("items")
        .filter(|items| items.is_object())
        .map(lower_node)
        .transpose()?
        .map(Box::new);
    let required = object
        .get("required")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let discriminator = object
        .get("discriminator")
        .and_then(Value::as_object)
        .and_then(|discriminator| {
            let property_name = discriminator.get("propertyName")?.as_str()?.to_owned();
            let mapping = discriminator
                .get("mapping")
                .and_then(Value::as_object)
                .map(|mapping| {
                    mapping
                        .iter()
                        .filter_map(|(key, value)| {
                            value.as_str().map(|value| (key.clone(), value.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(LoweredDiscriminator {
                property_name,
                mapping,
            })
        });
    let constraints = object
        .iter()
        .filter(|(key, _)| is_constraint_keyword(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let mut unmodeled_keywords = object
        .keys()
        .filter(|key| !is_modeled_keyword(key) && !key.starts_with("x-"))
        .cloned()
        .collect::<Vec<_>>();
    unmodeled_keywords.sort();

    Ok(LoweredNode {
        kind: schema_kind(object).to_owned(),
        types,
        reference,
        nullable: !nullability_encodings(node).is_empty(),
        const_value: object.get("const").cloned(),
        enum_values: object
            .get("enum")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        required,
        properties,
        items,
        one_of,
        any_of,
        all_of,
        discriminator,
        constraints,
        unmodeled_keywords,
    })
}

fn lower_array(value: Option<&Value>) -> Result<Vec<LoweredNode>> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(lower_node)
        .collect()
}

fn schema_types(node: &Value) -> Vec<String> {
    match node.get("type") {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn schema_kind(object: &Map<String, Value>) -> &'static str {
    if object.contains_key("oneOf") || object.contains_key("anyOf") {
        "union"
    } else if object.contains_key("allOf") {
        "intersection"
    } else if object.contains_key("properties")
        || object.get("type").and_then(Value::as_str) == Some("object")
    {
        "object"
    } else if object.contains_key("items")
        || object.get("type").and_then(Value::as_str) == Some("array")
    {
        "array"
    } else if object.contains_key("$ref") {
        "reference"
    } else if object.contains_key("type") || object.contains_key("enum") {
        "scalar"
    } else {
        "unknown"
    }
}

fn is_constraint_keyword(key: &str) -> bool {
    matches!(
        key,
        "additionalProperties"
            | "minProperties"
            | "maxProperties"
            | "minItems"
            | "maxItems"
            | "uniqueItems"
            | "minLength"
            | "maxLength"
            | "pattern"
            | "minimum"
            | "maximum"
            | "exclusiveMinimum"
            | "exclusiveMaximum"
            | "multipleOf"
            | "propertyNames"
            | "not"
    )
}

fn is_modeled_keyword(key: &str) -> bool {
    matches!(
        key,
        "$ref"
            | "type"
            | "nullable"
            | "const"
            | "enum"
            | "required"
            | "properties"
            | "items"
            | "oneOf"
            | "anyOf"
            | "allOf"
            | "discriminator"
            | "title"
            | "description"
            | "format"
            | "default"
            | "example"
            | "examples"
            | "deprecated"
            | "readOnly"
            | "writeOnly"
    ) || is_constraint_keyword(key)
}

fn collect_metrics(node: &Value, depth: usize, metrics: &mut SchemaMetrics) {
    metrics.node_count += 1;
    metrics.max_inline_depth = metrics.max_inline_depth.max(depth);
    if node.get("$ref").and_then(Value::as_str).is_some() {
        metrics.reference_count += 1;
    }
    if !nullability_encodings(node).is_empty() {
        metrics.nullable_node_count += 1;
    }
    metrics.one_of_branch_count += node
        .get("oneOf")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    metrics.any_of_branch_count += node
        .get("anyOf")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    metrics.all_of_branch_count += node
        .get("allOf")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);

    walk_direct_schema_children(node, &mut |child, _| {
        collect_metrics(child, depth + 1, metrics);
    });
}

fn walk_schema_nodes(node: &Value, pointer: &str, visit: &mut impl FnMut(&Value, &str)) {
    visit(node, pointer);
    walk_direct_schema_children(node, &mut |child, suffix| {
        let child_pointer = format!("{pointer}{suffix}");
        walk_schema_nodes(child, &child_pointer, visit);
    });
}

fn walk_direct_schema_children(node: &Value, visit: &mut impl FnMut(&Value, &str)) {
    let Some(object) = node.as_object() else {
        return;
    };
    for keyword in [
        "properties",
        "$defs",
        "definitions",
        "patternProperties",
        "dependentSchemas",
    ] {
        if let Some(children) = object.get(keyword).and_then(Value::as_object) {
            for (name, child) in children {
                visit(child, &format!("/{keyword}/{}", escape_json_pointer(name)));
            }
        }
    }
    for keyword in ["oneOf", "anyOf", "allOf", "prefixItems"] {
        if let Some(children) = object.get(keyword).and_then(Value::as_array) {
            for (index, child) in children.iter().enumerate() {
                visit(child, &format!("/{keyword}/{index}"));
            }
        }
    }
    for keyword in [
        "items",
        "additionalProperties",
        "contains",
        "propertyNames",
        "not",
        "if",
        "then",
        "else",
    ] {
        if let Some(child) = object.get(keyword).filter(|child| child.is_object()) {
            visit(child, &format!("/{keyword}"));
        }
    }
}

fn collect_refs(value: &Value, refs: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                refs.insert(reference.to_owned());
            }
            for child in object.values() {
                collect_refs(child, refs);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_refs(value, refs);
            }
        }
        _ => {}
    }
}

fn resolve_local_ref<'a>(document: &'a Value, value: &'a Value) -> Result<&'a Value> {
    let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
        return Ok(value);
    };
    let pointer = reference.strip_prefix('#').ok_or_else(|| {
        Error::message(format!(
            "external reference is forbidden during codegen: {reference}"
        ))
    })?;
    document
        .pointer(pointer)
        .ok_or_else(|| Error::message(format!("unresolved local reference: {reference}")))
}

fn object_at<'a>(document: &'a Value, pointer: &str) -> Result<&'a Map<String, Value>> {
    document
        .pointer(pointer)
        .and_then(Value::as_object)
        .ok_or_else(|| Error::message(format!("missing object at JSON pointer {pointer}")))
}

fn escape_json_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        initial_feature, initial_lifecycle, is_success_status, is_valid_implementation_status,
        nullability_encodings, response_mode, schema_kind,
    };

    #[test]
    fn classifies_success_statuses() {
        assert!(is_success_status("200"));
        assert!(is_success_status("2XX"));
        assert!(!is_success_status("default"));
        assert!(!is_success_status("302"));
    }

    #[test]
    fn classifies_nullable_union() {
        let node = json!({"anyOf": [{"type": "string"}, {"type": "null"}]});
        assert_eq!(nullability_encodings(&node), vec!["any_of_null"]);
    }

    #[test]
    fn classifies_response_modes() {
        assert_eq!(
            response_mode(&["application/json".to_owned()], false),
            "json"
        );
        assert_eq!(
            response_mode(&["application/json".to_owned()], true),
            "empty_or_json"
        );
        assert_eq!(
            response_mode(&["text/event-stream".to_owned()], false),
            "sse"
        );
    }

    #[test]
    fn preserves_union_before_reference_kind() -> Result<(), Box<dyn std::error::Error>> {
        let value = json!({"oneOf": [{"$ref": "#/components/schemas/A"}]});
        let object = value.as_object().ok_or("fixture must be an object")?;
        assert_eq!(schema_kind(object), "union");
        Ok(())
    }

    #[test]
    fn classifies_beta_alpha_legacy_and_access_controlled_surfaces()
    -> Result<(), Box<dyn std::error::Error>> {
        let operation = json!({});
        let operation = operation.as_object().ok_or("fixture must be an object")?;
        for (path, lifecycle, feature) in [
            ("/chatkit/sessions", "beta", "beta-chatkit"),
            ("/responses?beta=true", "beta", "beta-responses"),
            ("/fine_tuning/alpha/graders/run", "alpha", "alpha-graders"),
            ("/completions", "legacy", "legacy-completions"),
            ("/realtime/sessions", "legacy", "legacy-realtime"),
            ("/audio/voices", "access-controlled", "custom-voice"),
            ("/images/variations", "quarantined", "quarantine"),
            ("/threads/thread_1/runs", "sunset", "legacy-assistants"),
        ] {
            assert_eq!(initial_lifecycle(path, operation), lifecycle);
            assert_eq!(initial_feature(path, false, lifecycle), feature);
        }
        Ok(())
    }

    #[test]
    fn webhook_registry_label_uses_lowercase_method() {
        assert_eq!(
            super::webhook_registry_label("batch_cancelled", "POST"),
            "webhook:batch_cancelled:post"
        );
    }

    #[test]
    fn accepts_every_documented_implementation_status_and_rejects_unknown_values() {
        for status in [
            "planned",
            "partial",
            "implemented",
            "verified",
            "historical",
            "quarantined",
            "omitted",
        ] {
            assert!(is_valid_implementation_status(status), "rejected {status}");
        }
        assert!(!is_valid_implementation_status("complete"));
        assert!(!is_valid_implementation_status("deprecated"));
    }
}
