//! Regression coverage for reviewed additions to the pinned API contract.

use openai_rs_types::{Nullable, Omittable, responses::*};
use serde_json::{Value, json};

fn response_fixture() -> Value {
    json!({"id":"resp_test","object":"response","created_at":1,"model":"gpt-6-astra",
        "error":null,"incomplete_details":null,"instructions":null,"metadata":{},"output":[],
        "parallel_tool_calls":true,"temperature":null,"top_p":null,"tool_choice":"auto","tools":[],
        "prompt_cache_options":{"mode":"implicit","ttl":"30m","comparison_response_id":null},
        "prompt_cache_diagnostics":{"type":"cache_hit"}})
}

#[test]
fn prompt_cache_comparison_preserves_omitted_null_and_value() {
    for input in [
        json!({}),
        json!({"comparison_response_id":null}),
        json!({"comparison_response_id":"resp_previous"}),
    ] {
        let options: PromptCacheOptionsParam =
            serde_json::from_value(input.clone()).expect("valid test fixture");
        assert_eq!(
            serde_json::to_value(&options).expect("valid test fixture"),
            input
        );
    }
    assert_eq!(
        serde_json::to_value(
            PromptCacheOptionsParam::new().comparison_response_id("resp_previous")
        )
        .expect("valid test fixture"),
        json!({"comparison_response_id":"resp_previous"})
    );
    assert!(matches!(
        PromptCacheOptionsParam::new()
            .comparison_response_id_null()
            .comparison_response_id_presence(),
        Omittable::Value(Nullable::Null)
    ));
    let value = response_fixture();
    let response: Response = serde_json::from_value(value.clone()).expect("valid test fixture");
    assert!(matches!(
        response.prompt_cache_diagnostics(),
        Some(PromptCacheDiagnostics::CacheHit(_))
    ));
    assert_eq!(
        serde_json::to_value(response).expect("valid test fixture"),
        value
    );
}

#[test]
fn function_tool_async_and_future_fields_survive_complete_requests() {
    for flag in [None, Some(false), Some(true)] {
        let mut tool = json!({"type":"function","name":"lookup","parameters":{"type":"object","properties":{}},"future":{"nested":true}});
        if let Some(flag) = flag {
            tool["async"] = json!(flag);
        }
        let decoded: FunctionTool =
            serde_json::from_value(tool.clone()).expect("valid test fixture");
        assert_eq!(decoded.is_async(), flag);
        assert_eq!(
            serde_json::to_value(&decoded).expect("valid test fixture"),
            tool
        );
        let request = CreateResponseRequest::new("gpt-6-astra", "test").with_tool(decoded.clone());
        assert_eq!(
            serde_json::to_value(request).expect("valid test fixture")["tools"][0],
            tool
        );
        let request = CreateStreamingResponseRequest::new("gpt-6-astra", "test").with_tool(decoded);
        let encoded = serde_json::to_value(request).expect("valid test fixture");
        assert_eq!(encoded["tools"][0], tool);
        assert_eq!(encoded["stream"], true);
    }
    assert_eq!(
        FunctionTool::new("lookup").with_async(true).is_async(),
        Some(true)
    );
    for flag in [false, true] {
        let legacy = FunctionTool::new("lookup")
            .asynchronous(!flag)
            .with_async(flag);
        let current = FunctionTool::new("lookup")
            .with_async(!flag)
            .asynchronous(flag);
        assert_eq!(legacy.is_async(), Some(flag));
        assert_eq!(current.is_async(), Some(flag));
        assert_eq!(
            serde_json::to_value(legacy).expect("serialize compatible builder"),
            serde_json::to_value(current).expect("serialize current builder")
        );
    }
    assert!(
        serde_json::from_value::<FunctionTool>(
            json!({"type":"function","name":"lookup","async":null})
        )
        .is_err()
    );
    let custom: CustomTool =
        serde_json::from_value(json!({"type":"custom","name":"custom","async":false}))
            .expect("valid test fixture");
    assert_eq!(custom.is_async(), Some(false));
}

#[test]
fn response_error_exposes_typed_safety_details() {
    let mut value = response_fixture();
    value["error"] = json!({"code":"server_error","message":"blocked","misalignment":{"error_type":"other","steer":{"message":"review"}}});
    let response: Response = serde_json::from_value(value.clone()).expect("valid test fixture");
    assert_eq!(
        serde_json::to_value(&response).expect("valid test fixture"),
        value
    );
    let error: ResponseError =
        serde_json::from_value(value["error"].clone()).expect("valid test fixture");
    assert_eq!(
        error
            .misalignment()
            .expect("valid test fixture")
            .steer()
            .expect("valid test fixture")
            .message(),
        "review"
    );
}

#[cfg(feature = "beta-responses-multi-agent")]
#[test]
fn beta_uses_the_same_diagnostics_and_comparison_semantics() {
    use openai_rs_types::beta_responses::*;
    let value = response_fixture();
    let response: BetaResponse = serde_json::from_value(value.clone()).expect("valid test fixture");
    assert!(matches!(
        response.prompt_cache_diagnostics(),
        Some(PromptCacheDiagnostics::CacheHit(_))
    ));
    assert_eq!(
        serde_json::to_value(response).expect("valid test fixture"),
        value
    );
    let options = BetaPromptCacheOptionsParam::new().comparison_response_id_null();
    assert_eq!(
        serde_json::to_value(options).expect("valid test fixture"),
        json!({"comparison_response_id":null})
    );
}

#[cfg(feature = "admin")]
#[test]
fn service_account_key_expiry_preserves_three_states_and_checks_limits() {
    use openai_rs_types::admin::*;
    for value in [
        json!({}),
        json!({"expires_in_seconds":null}),
        json!({"expires_in_seconds":60}),
    ] {
        let request: CreateProjectServiceAccountApiKeyBody =
            serde_json::from_value(value.clone()).expect("valid test fixture");
        request.validate().expect("valid test fixture");
        assert_eq!(
            serde_json::to_value(request).expect("valid test fixture"),
            value
        );
    }
    for seconds in [1, 31536000] {
        CreateProjectServiceAccountApiKeyBody::default()
            .expires_in_seconds(seconds)
            .validate()
            .expect("valid test fixture");
    }
    for seconds in [0, 31536001] {
        assert!(
            CreateProjectServiceAccountApiKeyBody::default()
                .expires_in_seconds(seconds)
                .validate()
                .is_err()
        );
    }
    let response = json!({"id":"key_test","object":"organization.project.service_account.api_key","created_at":1,"name":"test","value":"test-key","expires_at":null});
    let key: ServiceAccountApiKeyBody =
        serde_json::from_value(response.clone()).expect("valid test fixture");
    assert!(matches!(key.expires_at, Omittable::Value(Nullable::Null)));
    assert_eq!(
        serde_json::to_value(key).expect("valid test fixture"),
        response
    );
}

#[test]
fn async_call_flags_survive_response_and_conversation_item_types() {
    let function = json!({"type":"function_call","id":"item_test","call_id":"call_test","name":"lookup","arguments":"{}","status":"completed","async":true});
    let call: FunctionCall = serde_json::from_value(function.clone()).expect("function call");
    assert_eq!(call.is_async(), Some(true));
    assert_eq!(serde_json::to_value(call).expect("serialize"), function);
    let item: openai_rs_types::conversations::ConversationFunctionCall =
        serde_json::from_value(function.clone()).expect("stored function call");
    assert_eq!(item.is_async(), Some(true));
    assert_eq!(serde_json::to_value(item).expect("serialize"), function);
    let custom = json!({"type":"custom_tool_call","call_id":"call_test","name":"custom","input":"text","async":false});
    let call: CustomToolCall = serde_json::from_value(custom.clone()).expect("custom call");
    assert_eq!(call.is_async(), Some(false));
    assert_eq!(serde_json::to_value(call).expect("serialize"), custom);

    let mut value = response_fixture();
    value["output"] = json!([function.clone(), custom.clone()]);
    let response: Response = serde_json::from_value(value).expect("response with calls");
    let replay = response.to_input_items();
    assert_eq!(
        serde_json::to_value(replay).expect("serialize response replay"),
        json!([function.clone(), custom.clone()])
    );
    for value in [function, custom] {
        let item: openai_rs_types::conversations::ConversationItem =
            serde_json::from_value(value.clone()).expect("stored call");
        let replay = item.to_response_input_item().expect("conversation replay");
        assert_eq!(
            serde_json::to_value(replay).expect("serialize conversation replay"),
            value
        );
    }
}

#[test]
fn response_diagnostics_preserve_future_variants_and_reject_malformed_known_tags() {
    for diagnostics in [
        json!({"type":"future_diagnostic","nested":{"tokens":12}}),
        json!({"type":"cache_miss","cache_missed_tokens":4,"reason":"future_reason"}),
    ] {
        let mut value = response_fixture();
        value["prompt_cache_diagnostics"] = diagnostics;
        let response: Response = serde_json::from_value(value.clone()).expect("future diagnostics");
        assert_eq!(serde_json::to_value(response).expect("serialize"), value);
        #[cfg(feature = "beta-responses-multi-agent")]
        {
            let response: openai_rs_types::beta_responses::BetaResponse =
                serde_json::from_value(value.clone()).expect("future beta diagnostics");
            assert_eq!(serde_json::to_value(response).expect("serialize"), value);
        }
    }
    for diagnostics in [
        json!({"type":"cache_miss","cache_missed_tokens":4}),
        json!({"type":"cache_miss","cache_missed_tokens":"4","reason":"model_changed"}),
        json!({"type":null}),
    ] {
        let mut value = response_fixture();
        value["prompt_cache_diagnostics"] = diagnostics;
        assert!(serde_json::from_value::<Response>(value.clone()).is_err());
        #[cfg(feature = "beta-responses-multi-agent")]
        assert!(
            serde_json::from_value::<openai_rs_types::beta_responses::BetaResponse>(value).is_err()
        );
    }
}
