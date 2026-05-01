use rostfrei_registry::{DomainRegistry, QueryDefinition, RegistrationError};

struct FindProduct;

impl QueryDefinition for FindProduct {
    type Response = String;

    const BOUNDED_CONTEXT: &'static str = "catalog";
    const QUERY_NAME: &'static str = "find-product";
    const SCHEMA_VERSION: u32 = 1;
}

#[test]
fn directly_registers_query_metadata() {
    let mut registry = DomainRegistry::new();
    registry.register_query::<FindProduct>().unwrap();

    let query = registry.query("catalog", "find-product", 1).unwrap();
    assert_eq!(query.bounded_context, "catalog");
    assert_eq!(query.query_name, "find-product");
    assert_eq!(query.schema_version, 1);
    assert!(query.rust_request_type.ends_with("FindProduct"));
    assert_eq!(query.rust_response_type, "alloc::string::String");
    assert!(query.modeled_query().is_none());
    assert_eq!(registry.queries_for_context("catalog").count(), 1);
}

#[test]
fn rejects_duplicate_query_metadata_without_mutation() {
    let mut registry = DomainRegistry::new();
    registry.register_query::<FindProduct>().unwrap();

    assert_eq!(
        registry.register_query::<FindProduct>(),
        Err(RegistrationError::DuplicateQueryIdentityAcrossModules {
            query_name: "find-product",
            schema_version: 1,
            existing_module_name: "<direct query registration>",
            attempted_module_name: "<direct query registration>",
        })
    );
    assert_eq!(registry.queries().count(), 1);
}
