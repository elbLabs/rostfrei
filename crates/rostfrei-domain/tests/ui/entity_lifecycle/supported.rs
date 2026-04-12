#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, EntityLifecycle, EntityLifecycleType,
    domain_actions,
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
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

#[domain_actions(entity)]
trait TodoActions {
    #[action(id = "activate", label = "Activate")]
    fn activate(&mut self);

    #[action(id = "complete", label = "Complete")]
    fn complete(&mut self);
}

#[derive(EntityLifecycle)]
#[domain(
    id = "workflow",
    label = "Todo workflow",
    owner = Todo,
    initial = Draft
)]
enum TodoLifecycle {
    #[domain(id = "draft", label = "Draft")]
    #[transition(action = TodoActions::ACTIVATE, to = Active)]
    Draft,
    #[domain(id = "active", label = "Active")]
    #[transition(action = TodoActions::COMPLETE, to = Completed)]
    Active,
    #[domain(id = "completed", label = "Completed")]
    Completed,
}

#[derive(DomainIdentity)]
#[domain(owner = Todo)]
struct TodoId(u8);

#[derive(Entity)]
#[domain(
    id = "todo",
    label = "Todo",
    owner = Owner,
    actions = [TodoActions],
    lifecycle = TodoLifecycle
)]
struct Todo {
    #[domain(identity)]
    id: TodoId,
}

impl TodoActions for Todo {
    fn activate(&mut self) {}
    fn complete(&mut self) {}
}

const _: domain::EntityLifecycleDescriptor = TodoLifecycle::DESCRIPTOR;

fn main() {}
