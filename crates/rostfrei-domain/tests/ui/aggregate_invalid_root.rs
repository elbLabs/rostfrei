use domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

#[derive(Aggregate)]
#[domain(id = "missing-root", label = "Missing Root", context = Inbox)]
struct MissingRoot;

struct PlainRoot;

#[derive(Aggregate)]
#[domain(id = "plain-root", label = "Plain Root", context = Inbox, root = PlainRoot)]
struct PlainRootAggregate;

#[derive(DomainIdentity)]
#[domain(owner = OtherRoot)]
struct Id(u64);

#[derive(Entity)]
#[domain(id = "other-root", label = "Other", owner = OtherAggregate)]
struct OtherRoot {
    #[domain(identity)]
    id: Id,
}

#[derive(Aggregate)]
#[domain(id = "wrong-owner", label = "Wrong Owner", context = Inbox, root = OtherRoot)]
struct WrongOwner;

#[derive(Aggregate)]
#[domain(id = "other", label = "Other", context = Inbox, root = OtherRoot)]
struct OtherAggregate;

fn main() {}
