#[test]
fn credentials_and_transport_modes_are_type_isolated() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/platform_rejects_codex_credential.rs");
    tests.compile_fail("tests/ui/platform_rejects_admin_credential.rs");
    tests.compile_fail("tests/ui/admin_rejects_platform_credential.rs");
    tests.compile_fail("tests/ui/x509_has_no_realtime.rs");
}
