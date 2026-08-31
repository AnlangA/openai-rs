//! Compile-level regression checks for the facade crate's client re-exports.
//!
//! `RetrieveResponseParams`, `RetrieveResponseStreamParams`, `BodyPreview`,
//! `RateLimitMetadata`, and the `sse` module used to be reachable only by
//! depending on `openai-rs-client` directly (issue 1-29). These tests keep the
//! facade paths nameable and constructible.

#![cfg(feature = "client")]

use openai_rs::{
    BodyPreview, RateLimitMetadata, RetrieveResponseParams, RetrieveResponseStreamParams,
};

#[test]
fn retrieve_params_are_nameable_through_the_facade() {
    use openai_rs::responses::ResponseIncludable;

    let include = ResponseIncludable::ReasoningEncryptedContent;
    assert_eq!(include.as_str(), "reasoning.encrypted_content");

    let params = RetrieveResponseParams::new().include(include);
    assert!(format!("{params:?}").contains("ReasoningEncryptedContent"));
    assert_eq!(
        RetrieveResponseParams::new(),
        RetrieveResponseParams::default()
    );

    let stream = RetrieveResponseStreamParams::new()
        .include(ResponseIncludable::InputImageUrl)
        .starting_after(42)
        .include_obfuscation(true);
    assert!(format!("{stream:?}").contains("InputImageUrl"));
    assert_ne!(stream, RetrieveResponseStreamParams::default());
}

#[test]
fn rate_limit_metadata_is_nameable_through_the_facade() {
    let default = RateLimitMetadata::default();
    assert!(default.remaining_requests.is_none());

    let metadata = RateLimitMetadata {
        remaining_requests: Some("99".into()),
        ..default
    };
    assert_eq!(metadata.remaining_requests.as_deref(), Some("99"));
}

#[test]
fn body_preview_is_nameable_through_the_facade() {
    // `BodyPreview` is only constructed inside the client crate; coercing its
    // method paths through the facade keeps the re-export compile-checked.
    let _: fn(&BodyPreview) -> &str = BodyPreview::as_str;
    let _: fn(&BodyPreview) -> bool = BodyPreview::is_truncated;
}

#[test]
fn sse_module_is_nameable_through_the_facade() {
    use openai_rs::sse::{
        DEFAULT_MAX_SSE_LINE_BYTES, SseDispatch, SseEndpointPolicy, SseLimits, SseStreamDecoder,
    };

    let limits = SseLimits::default();
    assert_eq!(limits.max_line_bytes(), DEFAULT_MAX_SSE_LINE_BYTES);

    let mut decoder = SseStreamDecoder::with_default_limits(SseEndpointPolicy::responses());
    match decoder.push(b"event: response.completed\ndata: {}\n\n") {
        Ok(dispatches) => {
            assert_eq!(dispatches.len(), 1);
            assert!(dispatches[0].is_terminal());
            assert!(matches!(dispatches[0], SseDispatch::Terminal(_)));
        }
        Err(error) => panic!("unexpected SSE decode error: {error:?}"),
    }
}

/// `RealtimeConnectTarget` used to be reachable only by depending on
/// `openai-rs-client` directly (issue 2-09): the facade's realtime gate kept
/// the `?call_id=` sideband connection path unnameable. This test keeps all
/// three connection targets constructible through the facade.
#[cfg(feature = "realtime")]
#[test]
fn realtime_connect_target_is_nameable_through_the_facade() {
    use openai_rs::RealtimeConnectTarget;
    use openai_rs::types::ModelId;

    let model = RealtimeConnectTarget::model("gpt-realtime");
    let intent = RealtimeConnectTarget::TranscriptionIntent;
    let call_id = RealtimeConnectTarget::call_id("call_123");

    assert_eq!(format!("{model:?}"), r#"Model(ModelId("gpt-realtime"))"#);
    assert_eq!(format!("{intent:?}"), "TranscriptionIntent");
    assert_eq!(format!("{call_id:?}"), r#"CallId("call_123")"#);

    assert_ne!(model, intent);
    assert_ne!(intent, call_id);
    assert_eq!(
        RealtimeConnectTarget::from(ModelId::new("gpt-a")),
        RealtimeConnectTarget::model("gpt-a")
    );
}

/// `AdminCheckpointPermissions` used to be reachable only by depending on
/// `openai-rs-client` directly (issue 2-34). The handle itself is constructed
/// from an `AdminClient`, so the test keeps both the accessor and the method
/// paths compile-checked through the facade.
#[cfg(all(feature = "admin", any(feature = "rustls-tls", feature = "native-tls")))]
#[test]
fn admin_checkpoint_permissions_is_nameable_through_the_facade() {
    use openai_rs::admin::{AdminApiKey, AdminCheckpointPermissions, AdminClient};

    // Async methods cannot coerce to `fn` pointers; referencing the paths is
    // enough to keep the facade re-export type-checked.
    let _list = AdminCheckpointPermissions::list;
    let _create = AdminCheckpointPermissions::create;
    let _delete = AdminCheckpointPermissions::delete;

    let api_key = AdminApiKey::new("admin-facade-reexport-check").expect("valid admin key");
    let client = AdminClient::new(api_key).expect("admin client with secure defaults");
    let permissions = client.checkpoint_permissions();
    assert!(format!("{permissions:?}").contains("AdminCheckpointPermissions"));
}
