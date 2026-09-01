#[test]
fn credentials_and_transport_modes_are_type_isolated() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/platform_rejects_codex_credential.rs");
    tests.compile_fail("tests/ui/platform_rejects_admin_credential.rs");
    tests.compile_fail("tests/ui/admin_rejects_platform_credential.rs");
    tests.compile_fail("tests/ui/x509_has_no_realtime.rs");
    tests.compile_fail("tests/ui/default_client_has_no_evals.rs");
}

/// 8-13: pin the remaining feature-gated surfaces (webhook-verification,
/// beta-chatkit, legacy-completions, custom-voice) as compile failures on the
/// contract-test feature set, which deliberately enables none of them. The
/// negative cases fall out of the gates themselves: without the feature the
/// facade method (or type re-export) does not exist, so a caller cannot even
/// name the surface.
#[test]
fn optional_feature_surfaces_are_compile_gated() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/default_client_has_no_webhooks.rs");
    tests.compile_fail("tests/ui/default_client_has_no_chatkit.rs");
    tests.compile_fail("tests/ui/default_client_has_no_legacy_completions.rs");
    tests.compile_fail("tests/ui/default_client_has_no_custom_voice.rs");
}
