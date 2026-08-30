#[test]
fn credentials_and_transport_modes_are_type_isolated() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/platform_rejects_codex_credential.rs");
}

