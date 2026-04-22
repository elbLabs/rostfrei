use domain::{BoundedContext, Command};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(Command)]
struct Generic<T>(T);

#[derive(Command)]
#[domain(id = "not-struct", label = "Not struct", owner = Catalog)]
enum NotAStruct {
    Value,
}

#[derive(Command)]
struct MissingMetadata;

#[derive(Command)]
#[domain(id = "missing-label", owner = Catalog)]
struct MissingLabel;

#[derive(Command)]
#[domain(id = "missing-owner", label = "Missing owner")]
struct MissingOwner;

#[derive(Command)]
#[domain(id = "bad_id", label = "Bad ID", owner = Catalog)]
struct InvalidId;

#[derive(Command)]
#[domain(id = "invalid-owner", label = "Invalid owner", owner = Catalog)]
struct InvalidOwner;

struct Child;

#[derive(Command)]
#[domain(id = "contains-entity", label = "Contains entity", owner = Catalog)]
struct ContainsEntity {
    #[domain(entity)]
    child: Child,
}

fn main() {}
