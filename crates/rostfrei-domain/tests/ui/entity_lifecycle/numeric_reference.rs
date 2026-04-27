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
    #[action(id = "2fa-start", label = "Start 2FA")]
    fn start(&mut self);
}

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Pending)]
enum Lifecycle {
    #[domain(id = "pending", label = "Pending")]
    #[transition(action = Actions::_2FA_START, to = Active)]
    Pending,
    #[domain(id = "active", label = "Active")]
    Active,
}

#[derive(DomainIdentity)]
#[domain(owner = Todo)]
struct TodoId(u8);

#[derive(Entity)]
#[domain(id = "todo", label = "Todo", owner = Owner, actions = [Actions], lifecycle = Lifecycle)]
struct Todo {
    #[domain(identity)]
    id: TodoId,
}

impl Actions for Todo {
    fn start(&mut self) {}
}

fn main() {}
