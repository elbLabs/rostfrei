#[test]
fn typed_event_codec_success() {
    let tests = trybuild::TestCases::new();
    tests.pass("tests/ui/event_codec/pass/*.rs");
}

#[test]
fn typed_event_codec_diagnostics() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/event_codec/fail/*.rs");
}
