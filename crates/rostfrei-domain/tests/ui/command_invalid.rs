use domain::{BoundedContext, Command};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(Command)]
struct Generic<T>(T);

#[derive(Command)]
#[domain(id = "not-struct", label = "Not struct")]
enum NotAStruct {
    Value,
}

#[derive(Command)]
struct MissingMetadata;

#[derive(Command)]
#[domain(id = "missing-label")]
struct MissingLabel;

#[derive(Command)]
#[domain(id = "bad_id", label = "Bad ID")]
struct InvalidId;

#[derive(Command)]
#[domain(id = "invalid-owner", label = "Invalid owner", owner = Catalog)]
struct UnsupportedOwner;

struct Child;

#[derive(Command)]
#[domain(id = "contains-entity", label = "Contains entity")]
struct ContainsEntity {
    #[domain(entity)]
    child: Child,
}

fn main() {}
