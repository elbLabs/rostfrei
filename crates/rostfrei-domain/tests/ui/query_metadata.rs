use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, domain_queries,
};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct Id(u64);

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    #[domain(identity)]
    id: Id,
}

impl domain::EntityDefinition for Root {
    type Owner = Model;
    type Identity = Id;
}

#[derive(Aggregate)]
#[domain(id = "model", label = "Model")]
struct Model;

impl domain::AggregateDefinition for Model {
    type Context = Catalog;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

#[domain_queries(group = MetadataQueries)]
impl Model {
    #[query(id = "duplicate", label = "One")]
    pub fn one(root: &Root) -> bool {
        true
    }

    #[query(id = "duplicate", label = "Two")]
    pub fn two(root: &Root) -> bool {
        true
    }

    #[query(id = "Bad_Id", label = "Invalid")]
    pub fn invalid_id(root: &Root) -> bool {
        true
    }

    #[query(id = "blank", label = " ")]
    pub fn blank_label(root: &Root) -> bool {
        true
    }

    #[query(id = "async", label = "Async")]
    pub async fn asynchronous(root: &Root) -> bool {
        true
    }

    #[query(id = "generic", label = "Generic")]
    pub fn generic<T>(root: &Root) -> bool {
        true
    }
}

#[domain_queries(group = One, group = Two)]
impl Model {
    #[query(id = "another", label = "Another")]
    pub fn another(root: &Root) -> bool {
        true
    }
}

fn main() {}
