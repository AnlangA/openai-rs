use std::{fmt::Debug, future::Future};

use openai_rs_codex::{ManagedAppServerCredential, RuntimeCompatibility};
use openai_rs_rmcp::{CatalogPolicy, ToolCatalog};
use openai_rs_types::{
    CreateEmbeddingRequest, CreateEmbeddingResponse, CreateModerationRequest,
    CreateModerationResponse, ExtraFields, FileObject, Model, ModelId, Nullable, Omittable, Upload,
    chat::{
        ChatCompletion, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionStreamRequest,
    },
    responses::{
        CompactResponseRequest, CompactedResponse, CountInputTokensRequest, CreateResponseRequest,
        CreateStreamingResponseRequest, DeletedResponse, InputTokenCountResponse, Response,
        ResponseInputItemList, ResponseStreamEvent,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn assert_wire<T>()
where
    T: Serialize + DeserializeOwned + Send + Sync + 'static,
{
}

fn assert_owned_runtime<T>()
where
    T: Send + Sync + 'static,
{
}

#[test]
fn public_json_dtos_are_bidirectional_owned_types() {
    assert_wire::<CreateResponseRequest>();
    assert_wire::<CreateStreamingResponseRequest>();
    assert_wire::<Response>();
    assert_wire::<DeletedResponse>();
    assert_wire::<CompactResponseRequest>();
    assert_wire::<CompactedResponse>();
    assert_wire::<CountInputTokensRequest>();
    assert_wire::<InputTokenCountResponse>();
    assert_wire::<ResponseInputItemList>();
    assert_wire::<ResponseStreamEvent>();
    assert_wire::<ChatCompletionRequest>();
    assert_wire::<ChatCompletionStreamRequest>();
    assert_wire::<ChatCompletion>();
    assert_wire::<ChatCompletionChunk>();
    assert_wire::<CreateEmbeddingRequest>();
    assert_wire::<CreateEmbeddingResponse>();
    assert_wire::<CreateModerationRequest>();
    assert_wire::<CreateModerationResponse>();
    assert_wire::<Model>();
    assert_wire::<FileObject>();
    assert_wire::<Upload>();

    assert_owned_runtime::<ManagedAppServerCredential>();
    assert_owned_runtime::<RuntimeCompatibility>();
    assert_owned_runtime::<ToolCatalog>();
    assert_owned_runtime::<CatalogPolicy>();
}

#[test]
fn optional_nullable_keeps_all_three_wire_states() {
    #[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
    struct Fixture {
        #[serde(default, skip_serializing_if = "Omittable::is_omitted")]
        field: Omittable<Nullable<String>>,
    }

    let missing: Fixture = serde_json::from_value(json!({})).expect("missing is valid");
    let null: Fixture = serde_json::from_value(json!({"field": null})).expect("null is valid");
    let value: Fixture = serde_json::from_value(json!({"field": "set"})).expect("value is valid");

    assert!(matches!(missing.field, Omittable::Omitted));
    assert!(matches!(null.field, Omittable::Value(Nullable::Null)));
    assert!(matches!(value.field, Omittable::Value(Nullable::Value(_))));
    assert_eq!(serde_json::to_value(missing).expect("encode"), json!({}));
    assert_eq!(
        serde_json::to_value(null).expect("encode"),
        json!({"field": null})
    );
    assert_eq!(
        serde_json::to_value(value).expect("encode"),
        json!({"field": "set"})
    );
}

#[test]
fn unknown_response_event_is_semantically_lossless_and_redacted_in_debug() {
    let fixture = json!({
        "type": "response.future.delta",
        "sequence_number": 9,
        "payload": {"secret_marker": "must-not-appear-in-debug"}
    });
    let event: ResponseStreamEvent =
        serde_json::from_value(fixture.clone()).expect("unknown event must decode");
    let debug = format!("{event:?}");
    assert!(!debug.contains("must-not-appear-in-debug"));
    assert_eq!(serde_json::to_value(event).expect("re-encode"), fixture);
}

#[test]
fn known_response_event_with_bad_payload_never_becomes_unknown() {
    let malformed = json!({
        "type": "response.output_text.delta",
        "sequence_number": 1,
        "delta": 42
    });
    assert!(serde_json::from_value::<ResponseStreamEvent>(malformed).is_err());
}

#[test]
fn request_builders_do_not_require_formatted_json() {
    let response = CreateResponseRequest::new("gpt-test", "hello");
    assert_eq!(
        serde_json::to_value(response).expect("encode"),
        json!({"model": "gpt-test", "input": "hello"})
    );

    let embedding = CreateEmbeddingRequest::new(ModelId::new("embedding-test"), "hello");
    assert_eq!(
        serde_json::to_value(embedding).expect("encode"),
        json!({"model": "embedding-test", "input": "hello"})
    );
}

#[test]
fn extra_fields_are_read_only_and_hide_payloads_in_debug() {
    let fields = serde_json::from_value::<ExtraFields>(json!({
        "future": {"private": "payload"}
    }))
    .expect("extra fields");
    assert!(fields.contains_key("future"));
    assert!(!format!("{fields:?}").contains("payload"));
    assert_eq!(
        serde_json::to_value(fields).expect("encode"),
        json!({"future": {"private": "payload"}})
    );
}

#[allow(dead_code)]
fn assert_async_result<F, T, E>(future: F)
where
    F: Future<Output = Result<T, E>> + Send,
    T: Send,
    E: Debug + Send,
{
    drop(future);
}

#[allow(dead_code)]
fn semantic_json(value: Value) -> Value {
    value
}
