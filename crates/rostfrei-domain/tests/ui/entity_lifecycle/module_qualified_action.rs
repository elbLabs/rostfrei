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

mod contracts {
    use domain::domain_actions;

    #[domain_actions(entity)]
    pub(crate) trait TodoActions {
        #[action(id = "activate", label = "Activate")]
        fn activate(&mut self);
    }
}

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Draft)]
enum TodoLifecycle {
    #[domain(id = "draft", label = "Draft")]
    #[transition(action = crate::contracts::TodoActions::ACTIVATE, to = Active)]
    Draft,
    #[domain(id = "active", label = "Active")]
    Active,
}

#[derive(DomainIdentity)]
#[domain(owner = Todo)]
struct TodoId(u8);

#[derive(Entity)]
#[domain(
    id = "todo",
    label = "Todo",
    owner = Owner,
    actions = [contracts::TodoActions],
    lifecycle = TodoLifecycle
)]
struct Todo {
    #[domain(identity)]
    id: TodoId,
}

impl contracts::TodoActions for Todo {
    fn activate(&mut self) {}
}

fn main() {}
