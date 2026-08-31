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
