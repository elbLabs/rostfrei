#![allow(dead_code)]

use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, EntityLifecycle};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct RootId(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Owner)]
struct Root {
    #[domain(identity)]
    id: RootId,
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

#[derive(DomainIdentity)]
#[domain(owner = Other)]
struct OtherId(u8);

#[derive(Entity)]
#[domain(id = "other", label = "Other", owner = Owner)]
struct Other {
    #[domain(identity)]
    id: OtherId,
}

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Other, initial = Draft)]
enum Lifecycle {
    #[domain(id = "draft", label = "Draft")]
    Draft,
}

#[derive(DomainIdentity)]
#[domain(owner = Todo)]
struct TodoId(u8);

#[derive(Entity)]
#[domain(id = "todo", label = "Todo", owner = Owner, lifecycle = Lifecycle)]
struct Todo {
    #[domain(identity)]
    id: TodoId,
}

fn main() {}
