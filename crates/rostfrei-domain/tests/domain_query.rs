#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, DomainModelError, Entity, QueryGroupType,
    domain_model, domain_queries,
};
use serde_json::json;

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainIdentity, Clone)]
struct CatalogId(u64);

#[derive(Entity)]
#[domain(id = "catalog-root", label = "Catalog")]
struct CatalogRoot {
    #[domain(identity)]
    id: CatalogId,
    count: usize,
}

impl domain::EntityDefinition for CatalogRoot {
    type Owner = CatalogAggregate;
    type Identity = CatalogId;
}

#[derive(Aggregate)]
#[domain(id = "catalog", label = "Catalog")]
struct CatalogAggregate;

impl domain::AggregateDefinition for CatalogAggregate {
    type Context = Catalog;
    type Root = CatalogRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(Clone)]
struct Filter(String);

#[domain_queries(group = CatalogQueries)]
impl CatalogAggregate {
    #[query(id = "count", label = "Count")]
    pub const fn count(root: &CatalogRoot) -> usize {
        root.count
    }

    #[query(id = "contains", label = "Contains")]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub const fn contains(root: &CatalogRoot, input: &u64) -> bool {
        #[cfg(target_pointer_width = "16")]
        let count = {
            let [first, second] = root.count.to_be_bytes();
            u64::from_be_bytes([0, 0, 0, 0, 0, 0, first, second])
        };
        #[cfg(target_pointer_width = "32")]
        let count = {
            let [first, second, third, fourth] = root.count.to_be_bytes();
            u64::from_be_bytes([0, 0, 0, 0, first, second, third, fourth])
        };
        #[cfg(target_pointer_width = "64")]
        let count = u64::from_be_bytes(root.count.to_be_bytes());

        count == *input
    }

    #[query(id = "search", label = "Search")]
    pub fn search(root: &CatalogRoot, input: &Filter) -> Vec<Option<CatalogId>> {
        if input.0.is_empty() || root.count == 0 {
            Vec::new()
        } else {
            vec![Some(root.id.clone())]
        }
    }

    #[query(id = "lookup", label = "Lookup")]
    pub fn lookup(root: &CatalogRoot, input: &CatalogId) -> Option<Filter> {
        (root.id.0 == input.0).then(|| Filter("found".to_owned()))
    }
}

#[test]
fn derives_query_descriptors_and_keeps_functions_callable() {
    let queries = CatalogQueries::QUERIES;
    assert_eq!(queries.len(), 4);
    assert_eq!(queries[0].id.local, "count");
    assert_eq!(queries[1].id.local, "contains");
    assert_eq!(queries[2].id.local, "search");
    assert_eq!(queries[3].id.local, "lookup");
    let root = CatalogRoot {
        id: CatalogId(7),
        count: 2,
    };
    assert_eq!(CatalogAggregate::count(&root), 2);
    assert!(CatalogAggregate::contains(&root, &2));
    assert_eq!(
        CatalogAggregate::search(&root, &Filter("all".to_owned())).len(),
        1
    );
}

#[test]
fn projects_queries_and_identities_to_exact_json() {
    let model = domain_model! {
        contexts: [Catalog],
        aggregates: [CatalogAggregate],
        entities: [CatalogRoot],
        value_objects: [],
        services: [],
        errors: [],

        query_groups: [CatalogQueries],
    }
    .expect("query domain model should be valid");
    let aggregate = json!({ "context": "catalog", "local": "catalog" });
    let entity = json!({ "aggregate": aggregate, "local": "catalog-root" });
    let identity = json!({ "owner": entity });
    assert_eq!(model["domainIdentities"], json!([{ "id": identity }]));
    assert_eq!(
        model["entities"][0]["identity"],
        json!({ "field": "id", "id": identity })
    );
    assert_eq!(
        model["queries"][0],
        json!({
            "id": { "aggregate": aggregate, "local": "count" },
            "label": "Count",
        })
    );
}

struct DuplicateQueries;

impl QueryGroupType for DuplicateQueries {
    type Owner = CatalogAggregate;
    const QUERIES: &'static [domain::QueryDescriptor] = CatalogQueries::QUERIES;
}

#[test]
fn rejects_duplicate_query_ids_across_groups() {
    let error = domain_model! {
        contexts: [], aggregates: [], entities: [],value_objects: [],
        services: [], errors: [],
        query_groups: [CatalogQueries, DuplicateQueries],
    }
    .expect_err("duplicate query IDs should be rejected");
    let id = CatalogQueries::QUERIES[0].id;

    assert_eq!(
        error,
        DomainModelError::DuplicateQueryId { id: Box::new(id) }
    );
    assert_eq!(error.to_string(), format!("duplicate QueryId: {id:?}"));
}
