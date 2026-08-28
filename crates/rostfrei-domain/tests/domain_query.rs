#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, DomainIdentityType, Entity, QueryGroupType,
    QueryInputDescriptor, QueryOutputDescriptor, ScalarType, ValueObject, ValueObjectType,
    domain_model, domain_queries,
};
use serde_json::json;

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainIdentity, Clone)]
#[domain(owner = CatalogRoot)]
struct CatalogId(u64);

#[derive(Entity)]
#[domain(id = "catalog-root", label = "Catalog", owner = CatalogAggregate)]
struct CatalogRoot {
    #[domain(identity)]
    id: CatalogId,
    count: usize,
}

#[derive(Aggregate)]
#[domain(id = "catalog", label = "Catalog", context = Catalog, root = CatalogRoot)]
struct CatalogAggregate;

#[derive(ValueObject, Clone)]
#[domain(id = "filter", label = "Filter", owner = CatalogAggregate)]
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
    assert_eq!(queries[0].input, None);
    assert_eq!(
        queries[0].output,
        QueryOutputDescriptor::Scalar(ScalarType::Usize)
    );
    assert_eq!(
        queries[1].input,
        Some(QueryInputDescriptor::Scalar(ScalarType::U64))
    );
    assert_eq!(
        queries[2].input,
        Some(QueryInputDescriptor::ValueObject(Filter::DESCRIPTOR.id))
    );
    assert_eq!(
        queries[3].input,
        Some(QueryInputDescriptor::DomainIdentity(
            CatalogId::DESCRIPTOR.id
        ))
    );
    assert!(matches!(queries[2].output, QueryOutputDescriptor::List(_)));
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
        identities: [CatalogId],
        value_objects: [Filter],
        services: [],
        commands: [],
        errors: [],

        query_groups: [CatalogQueries],
    };
    let aggregate = json!({ "context": "catalog", "local": "catalog" });
    let entity = json!({ "aggregate": aggregate, "local": "catalog-root" });
    let identity = json!({ "owner": entity });
    assert_eq!(
        model["domainIdentities"],
        json!([{ "id": identity, "scalar": "u64" }])
    );
    assert_eq!(
        model["entities"][0]["identity"],
        json!({ "field": "id", "id": identity })
    );
    assert_eq!(
        model["queries"][0],
        json!({
            "id": { "aggregate": aggregate, "local": "count" },
            "label": "Count",
            "input": null,
            "output": { "kind": "scalar", "scalar": "usize" },
        })
    );
    assert_eq!(model["queries"][2]["output"]["kind"], "list");
    assert_eq!(model["queries"][2]["output"]["element"]["kind"], "optional");
    assert_eq!(
        model["queries"][2]["output"]["element"]["value"]["kind"],
        "domainIdentity"
    );
}

struct DuplicateQueries;

impl QueryGroupType for DuplicateQueries {
    type Owner = CatalogAggregate;
    const QUERIES: &'static [domain::QueryDescriptor] = CatalogQueries::QUERIES;
}

#[test]
#[should_panic(expected = "duplicate QueryId")]
fn rejects_duplicate_query_ids_across_groups() {
    let _ = domain_model! {
        contexts: [], aggregates: [], entities: [], identities: [], value_objects: [],
        services: [], commands: [], errors: [],
        query_groups: [CatalogQueries, DuplicateQueries],
    };
}

#[test]
#[should_panic(expected = "duplicate DomainIdentityId")]
fn rejects_duplicate_identity_ids() {
    let _ = domain_model! {
        contexts: [], aggregates: [], entities: [], identities: [CatalogId, CatalogId],
        value_objects: [], services: [], commands: [], errors: [],
        query_groups: [],
    };
}
