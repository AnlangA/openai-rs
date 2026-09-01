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

/// The four round-8 spend resource clients (org and project scopes for both
/// alerts and limits) were missing from the facade's admin module (issue
/// 10-02, same family as 2-34): facade-only users could not name the handles
/// returned by `AdminClient::spend_alerts`/`spend_limits`. This test keeps
/// all four types nameable and their accessor/method paths compile-checked.
#[cfg(all(feature = "admin", any(feature = "rustls-tls", feature = "native-tls")))]
#[test]
fn admin_spend_resources_are_nameable_through_the_facade() {
    use openai_rs::admin::{
        AdminApiKey, AdminClient, AdminProjectSpendAlerts, AdminProjectSpendLimits,
        AdminSpendAlerts, AdminSpendLimits,
    };

    // Async methods cannot coerce to `fn` pointers; referencing the paths is
    // enough to keep the facade re-export type-checked.
    let _list = AdminSpendAlerts::list;
    let _create = AdminSpendAlerts::create;
    let _get = AdminSpendLimits::get;
    let _delete = AdminSpendLimits::delete;
    let _project_list = AdminProjectSpendAlerts::list;
    let _project_update = AdminProjectSpendLimits::update;

    let api_key = AdminApiKey::new("admin-facade-reexport-check").expect("valid admin key");
    let client = AdminClient::new(api_key).expect("admin client with secure defaults");
    let alerts = client.spend_alerts();
    assert!(format!("{alerts:?}").contains("AdminSpendAlerts"));
    let project_alerts = alerts.project("proj_1");
    assert!(format!("{project_alerts:?}").contains("AdminProjectSpendAlerts"));
    let limits = client.spend_limits();
    assert!(format!("{limits:?}").contains("AdminSpendLimits"));
    let project_limits = limits.project("proj_1");
    assert!(format!("{project_limits:?}").contains("AdminProjectSpendLimits"));
}

/// The admin operation machinery (`AdminOperation`/`AdminQuery` trait pair,
/// their supporting enums, `AdminClientOperationContract`, and both manifest
/// constants) used to be reachable only by depending on `openai-rs-client`
/// directly (issue 3-25): helper code parameterized over the traits was
/// unnameable through the facade. This test keeps the traits usable as
/// generic bounds and the remaining items compile-checked via reference
/// paths, mirroring the client crate's root export list.
#[cfg(feature = "admin")]
#[test]
fn admin_operation_machinery_is_nameable_through_the_facade() {
    use openai_rs::admin::operations::OpAdminApiKeysList;
    use openai_rs::admin::{
        ADMIN_CHECKPOINT_PERMISSION_OPERATION_MANIFEST, ADMIN_CLIENT_OPERATION_MANIFEST,
        AdminAuthScope, AdminClientOperationContract, AdminListParams, AdminOperation, AdminQuery,
        AdminRequestEncoding, AdminResponseMode,
    };

    fn assert_operation_contract<O: AdminOperation>() {
        assert_eq!(O::AUTH, AdminAuthScope::Admin);
        assert!(matches!(
            O::REQUEST_ENCODING,
            AdminRequestEncoding::None | AdminRequestEncoding::Json
        ));
        assert!(matches!(O::RESPONSE_MODE, AdminResponseMode::Json));
    }

    fn assert_query<Q: AdminQuery>() {
        fn is_serializable<T: serde::Serialize>() {}
        is_serializable::<Q>();
    }

    assert_operation_contract::<OpAdminApiKeysList>();
    assert_query::<AdminListParams>();

    let client_manifest: &'static [AdminClientOperationContract] = ADMIN_CLIENT_OPERATION_MANIFEST;
    let checkpoint_manifest: &'static [AdminClientOperationContract] =
        ADMIN_CHECKPOINT_PERMISSION_OPERATION_MANIFEST;
    let contract = OpAdminApiKeysList::CONTRACT;
    assert_eq!(contract.operation_id, "admin-api-keys-list");
    assert!(client_manifest.contains(&contract));
    assert!(!checkpoint_manifest.is_empty());
    assert!(client_manifest.iter().any(|entry| entry == &contract));
}

/// The three content-provenance discriminator DTOs re-exported by the client
/// crate's root were missing from the facade's flat client-gate list (issue
/// 3-25). This test keeps them nameable and constructible through the facade.
#[test]
fn content_provenance_discriminators_are_nameable_through_the_facade() {
    use openai_rs::{C2paValidationState, ContentProvenanceObjectType, ProvenanceDetectionOutcome};

    assert_eq!(
        ContentProvenanceObjectType::ContentProvenanceCheck.as_str(),
        "content_provenance_check"
    );
    assert_eq!(
        ContentProvenanceObjectType::from_raw("future_check").unknown_value(),
        Some("future_check")
    );

    assert_eq!(ProvenanceDetectionOutcome::Detected.as_str(), "detected");
    assert_eq!(
        ProvenanceDetectionOutcome::NotDetected.as_str(),
        "not_detected"
    );

    assert_eq!(C2paValidationState::Trusted.as_str(), "trusted");
    assert_eq!(C2paValidationState::Valid.as_str(), "valid");
    assert_eq!(C2paValidationState::Invalid.as_str(), "invalid");
    assert_eq!(C2paValidationState::NotPresent.as_str(), "not_present");
    assert!(C2paValidationState::from_raw("trusted").is_known());
}
