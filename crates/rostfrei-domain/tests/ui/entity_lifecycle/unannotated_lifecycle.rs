#![allow(dead_code)]

use domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

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

enum Lifecycle {
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
