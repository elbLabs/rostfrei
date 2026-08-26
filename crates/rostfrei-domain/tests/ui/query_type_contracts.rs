use domain::{Aggregate, BoundedContext, DomainCommand, DomainError, DomainEvent, DomainIdentity, Entity, domain_queries};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainIdentity)]
#[domain(owner = FirstRoot)]
struct FirstId(u64);

#[derive(Entity)]
#[domain(id = "first-root", label = "First", owner = First)]
struct FirstRoot { #[domain(identity)] id: FirstId }

#[derive(Aggregate)]
#[domain(id = "first", label = "First", context = Catalog, root = FirstRoot, events = [Event])]
struct First;

#[derive(DomainIdentity)]
#[domain(owner = SecondRoot)]
struct SecondId(u64);

#[derive(Entity)]
#[domain(id = "second-root", label = "Second", owner = Second)]
struct SecondRoot { #[domain(identity)] id: SecondId }

#[derive(Aggregate)]
#[domain(id = "second", label = "Second", context = Catalog, root = SecondRoot)]
struct Second;

#[derive(DomainCommand)]
#[domain(id = "command", label = "Command", owner = First)]
struct Command;

#[derive(DomainEvent)]
#[domain(id = "event", label = "Event")]
struct Event;

#[derive(DomainError)]
#[domain(id = "error", label = "Error", owner = First, code = "ERROR", message = "Error.")]
struct Error;

struct Plain;

#[domain_queries(group = TypeQueries)]
impl First {
    #[query(id = "wrong-root", label = "Wrong root")]
    pub fn wrong_root(root: &SecondRoot) -> bool { true }

    #[query(id = "result", label = "Result")]
    pub fn result(root: &FirstRoot) -> Result<bool, Error> { Ok(true) }

    #[query(id = "event", label = "Event")]
    pub fn event(root: &FirstRoot) -> Event { Event }

    #[query(id = "error", label = "Error")]
    pub fn error(root: &FirstRoot) -> Error { Error }

    #[query(id = "plain", label = "Plain")]
    pub fn plain(root: &FirstRoot) -> Plain { Plain }

    #[query(id = "command", label = "Command")]
    pub fn command(root: &FirstRoot, input: &Command) -> bool { true }

    #[query(id = "cross", label = "Cross")]
    pub fn cross(root: &FirstRoot, input: &SecondId) -> SecondId { SecondId(1) }
}

fn main() {}
