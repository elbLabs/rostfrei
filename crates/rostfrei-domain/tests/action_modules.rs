mod domain {
    use domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

    #[derive(BoundedContext)]
    #[domain(id = "catalog", label = "Catalog")]
    pub struct Catalog;

    #[derive(DomainIdentity)]
    #[domain(owner = TaxonomyRoot)]
    pub struct TaxonomyId(pub(crate) u64);

    #[derive(Entity)]
    #[domain(id = "taxonomy-root", label = "Taxonomy")]
    pub struct TaxonomyRoot {
        #[domain(identity)]
        pub(crate) id: TaxonomyId,
        pub(crate) published: bool,
        pub(crate) deprecated: bool,
    }

    impl domain::EntityDefinition for TaxonomyRoot {
        type Owner = ServiceTaxonomy;
        type Identity = TaxonomyId;
    }

    #[derive(Aggregate)]
    #[domain(id = "service-taxonomy", label = "Service taxonomy")]
    pub struct ServiceTaxonomy;

    impl domain::AggregateDefinition for ServiceTaxonomy {
        type Context = Catalog;
        type Root = TaxonomyRoot;
        type Event = domain::NoDomainEvents;
    }

    pub mod publication {
        use domain::domain_actions;

        #[domain_actions(aggregate)]
        pub trait CategoryPublicationActions {
            #[action(id = "publish-category", label = "Publish category")]
            fn publish_category(root: &mut super::TaxonomyRoot);
        }
    }

    pub mod deprecation {
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
    use domain::{DomainModelError, domain_model};

    use super::domain::{Catalog, ServiceTaxonomy, TaxonomyId, TaxonomyRoot};

    pub fn registered_owner() -> Result<serde_json::Value, DomainModelError> {
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
fn aggregate_contracts_remain_callable_without_implicit_model_registration() {
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

    let model = model::registered_owner().expect("registered owner model should be valid");
    let actions = model["actions"].as_array().unwrap();
    assert!(actions.is_empty());
}
