use domain::{BoundedContext, DomainCommand};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainCommand)]
struct Generic<T>(T);

#[derive(DomainCommand)]
#[domain(id = "not-struct", label = "Not struct", owner = Catalog)]
enum NotAStruct {
    Value,
}

#[derive(DomainCommand)]
struct MissingMetadata;

#[derive(DomainCommand)]
#[domain(id = "missing-label", owner = Catalog)]
struct MissingLabel;

#[derive(DomainCommand)]
#[domain(id = "missing-owner", label = "Missing owner")]
struct MissingOwner;

#[derive(DomainCommand)]
#[domain(id = "bad_id", label = "Bad ID", owner = Catalog)]
struct InvalidId;

#[derive(DomainCommand)]
#[domain(id = "invalid-owner", label = "Invalid owner", owner = Catalog)]
struct InvalidOwner;

struct Child;

#[derive(DomainCommand)]
#[domain(id = "contains-entity", label = "Contains entity", owner = Catalog)]
struct ContainsEntity {
    #[domain(entity)]
    child: Child,
}

fn main() {}
