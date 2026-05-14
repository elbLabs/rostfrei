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
#[domain(id = "missing-context", label = "Missing context")]
struct MissingContext;

#[derive(Command)]
#[domain(context = Catalog, id = "missing-label")]
struct MissingLabel;

#[derive(Command)]
#[domain(context = Catalog, id = "bad_id", label = "Bad ID")]
struct InvalidId;

#[derive(Command)]
#[domain(context = Catalog, id = "invalid-owner", label = "Invalid owner", owner = Catalog)]
struct UnsupportedOwner;

struct Child;

#[derive(Command)]
#[domain(context = Catalog, id = "contains-entity", label = "Contains entity")]
struct ContainsEntity {
    #[domain(entity)]
    child: Child,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
