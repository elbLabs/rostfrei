mod domain {
    use domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

    #[derive(BoundedContext)]
    #[domain(id = "catalog", label = "Catalog")]
    pub(crate) struct Catalog;

    #[derive(DomainIdentity)]
    #[domain(owner = TaxonomyRoot)]
    pub(crate) struct TaxonomyId(pub(crate) u64);

    #[derive(Entity)]
    #[domain(id = "taxonomy-root", label = "Taxonomy", owner = ServiceTaxonomy)]
    pub(crate) struct TaxonomyRoot {
        #[domain(identity)]
        pub(crate) id: TaxonomyId,
        pub(crate) published: bool,
        pub(crate) deprecated: bool,
    }

    #[derive(Aggregate)]
    #[domain(
        id = "service-taxonomy",
        label = "Service taxonomy",
        context = Catalog,
        root = TaxonomyRoot,
        actions = [
            publication::CategoryPublicationActions,
            deprecation::CategoryDeprecationActions
        ]
    )]
    pub(crate) struct ServiceTaxonomy;

    pub(crate) mod publication {
        use domain::domain_actions;

        #[domain_actions(aggregate)]
        pub trait CategoryPublicationActions {
            #[action(id = "publish-category", label = "Publish category")]
            fn publish_category(root: &mut super::TaxonomyRoot);
        }
    }

    pub(crate) mod deprecation {
        use domain::domain_actions;

        #[domain_actions(aggregate)]
        pub trait CategoryDeprecationActions {
            #[action(id = "deprecate-category", label = "Deprecate category")]
            fn deprecate_category(root: &mut super::TaxonomyRoot);
        }
    }

    impl publication::CategoryPublicationActions for ServiceTaxonomy {
        fn publish_category(root: &mut TaxonomyRoot) {
            root.published = true;
        }
    }

    impl deprecation::CategoryDeprecationActions for ServiceTaxonomy {
        fn deprecate_category(root: &mut TaxonomyRoot) {
            root.deprecated = true;
        }
    }
}

mod model {
    use domain::domain_model;

    use super::domain::{Catalog, ServiceTaxonomy, TaxonomyId, TaxonomyRoot};

    pub(crate) fn registered_owner() -> serde_json::Value {
        domain_model! {
            contexts: [Catalog],
            aggregates: [ServiceTaxonomy],
            entities: [TaxonomyRoot],
            identities: [TaxonomyId],
            value_objects: [],
            services: [],
            commands: [],
            errors: [],
            action_extensions: [],
            query_groups: [],
        }
    }
}

#[test]
fn registering_owner_projects_complete_contract_and_supports_runtime_calls() {
    use domain::deprecation::CategoryDeprecationActions;
    use domain::publication::CategoryPublicationActions;

    let mut root = domain::TaxonomyRoot {
        id: domain::TaxonomyId(1),
        published: false,
        deprecated: false,
    };
    domain::ServiceTaxonomy::publish_category(&mut root);
    domain::ServiceTaxonomy::deprecate_category(&mut root);
    assert!(root.published);
    assert!(root.deprecated);
    assert_eq!(root.id.0, 1);

    let model = model::registered_owner();
    let actions = model["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["id"]["local"], "publish-category");
    assert_eq!(actions[1]["id"]["local"], "deprecate-category");
    assert_eq!(actions[0]["id"]["owner"], actions[1]["id"]["owner"]);
}
