#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, EntityLifecycle, domain_actions,
};

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
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

#[domain_actions(entity)]
trait Actions {
    #[action(id = "activate", label = "Activate")]
    fn activate(&mut self);
}

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Draft)]
enum Lifecycle {
    #[domain(id = "draft", label = "Draft")]
    #[transition(action = Actions::ACTIVATE, to = Active)]
    Draft,
    #[domain(id = "active", label = "Active")]
    Active,
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
