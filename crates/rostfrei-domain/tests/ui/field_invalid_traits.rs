use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, ValueObject};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct Id(u64);

#[derive(DomainIdentity)]
#[domain(owner = OtherRoot)]
struct OtherId(u64);

#[derive(DomainIdentity)]
#[domain(owner = Wrong)]
struct WrongId(u64);

struct Plain;

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = First)]
struct Root {
    #[domain(identity)]
    id: Id,
}

#[derive(Aggregate)]
#[domain(id = "first", label = "First", context = Context, root = Root)]
struct First;

#[derive(Entity)]
#[domain(id = "other-root", label = "Other root", owner = Second)]
struct OtherRoot {
    #[domain(identity)]
    id: OtherId,
}

#[derive(Aggregate)]
#[domain(id = "second", label = "Second", context = Context, root = OtherRoot)]
struct Second;

#[derive(Entity)]
#[domain(id = "wrong", label = "Wrong", owner = First)]
struct Wrong {
    #[domain(identity)]
    id: WrongId,
    #[domain(entity)]
    other: OtherRoot,
    #[domain(value_object)]
    plain_value: Plain,
    #[domain(aggregate_ref = Plain)]
    reference: Plain,
}

#[derive(ValueObject)]
#[domain(id = "wrong-value", label = "Wrong value", owner = Context)]
struct WrongValue(#[domain(value_object)] Plain);

fn main() {}
