#[test]
fn wasvy_methods_ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/wasvy_methods/*.rs");
}
