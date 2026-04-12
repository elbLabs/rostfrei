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
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "rename", label = "Rename")]
    fn rename(&mut self);
}

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Active)]
enum Lifecycle {
    #[domain(id = "active", label = "Active")]
    #[transition(action = Actions::RENAME, to = Active)]
    Active,
    #[domain(id = "terminal", label = "Terminal")]
    Terminal,
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
    fn rename(&mut self) {}
}

fn main() {}
