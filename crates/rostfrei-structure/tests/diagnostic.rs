use rostfrei_structure::DiagnosticCode;

#[test]
fn diagnostic_codes_keep_their_stable_external_representation() {
    let cases = [
        (DiagnosticCode::SourceParse, "RF000"),
        (DiagnosticCode::InvalidStructure, "RF001"),
        (DiagnosticCode::ImpureModule, "RF002"),
        (DiagnosticCode::WrongPlacement, "RF003"),
        (DiagnosticCode::InvalidCardinality, "RF004"),
        (DiagnosticCode::TestPlacement, "RF005"),
        (DiagnosticCode::ModuleTopology, "RF006"),
        (DiagnosticCode::UnexpectedRoleContent, "RF007"),
        (DiagnosticCode::InvalidTestMirror, "RF008"),
        (DiagnosticCode::MissingDomainCheckTarget, "RF009"),
        (DiagnosticCode::CompiledDomainCheckFailed, "RF010"),
    ];

    for (code, expected) in cases {
        assert_eq!(code.as_str(), expected);
        assert_eq!(code.to_string(), expected);
    }
}
