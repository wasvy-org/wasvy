#[test]
fn module_authoring_contract_rejects_unsupported_params() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/trybuild/module_authoring/fail_*.rs");
}
