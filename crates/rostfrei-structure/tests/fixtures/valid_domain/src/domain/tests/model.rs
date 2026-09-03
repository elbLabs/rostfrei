#[test]
fn compiles_the_domain_model() {
    let model = super::super::model::compiled_model();
    assert_eq!(model.contexts().len(), 1);
}
