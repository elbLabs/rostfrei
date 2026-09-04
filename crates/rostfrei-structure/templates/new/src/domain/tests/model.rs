use super::super::domain_model;

#[test]
fn empty_domain_model_is_valid() {
    assert!(domain_model().is_ok());
}
