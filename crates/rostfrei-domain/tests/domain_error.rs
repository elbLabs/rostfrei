use domain::{DomainError, JsonErrorPayload};
use serde_json::json;

#[derive(DomainError)]
#[domain(
    id = "subject-blank",
    label = "Subject blank",
    code = "SUBJECT_BLANK",
    message = "The subject must not be blank."
)]
struct SubjectBlank {
    supplied: String,
}

#[test]
fn generated_json_errors_preserve_canonical_code_message_and_application_fields() {
    assert_eq!(
        SubjectBlank {
            supplied: " ".to_owned()
        }
        .encode_json()
        .unwrap(),
        json!({"code": "SUBJECT_BLANK", "message": "The subject must not be blank.", "supplied": " "}),
    );
}

rostfrei_domain_macros::__install_test_macro_support!();
