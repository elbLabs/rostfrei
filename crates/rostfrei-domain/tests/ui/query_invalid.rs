use domain::{Aggregate, BoundedContext, DomainCommand, DomainError, DomainEvent, DomainIdentity, Entity, ValueObject, domain_queries};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct Id(u64);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Model)]
struct Root { #[domain(identity)] id: Id }

#[derive(Aggregate)]
#[domain(id = "model", label = "Model", context = Catalog, root = Root)]
struct Model;

#[derive(ValueObject)]
#[domain(id = "filter", label = "Filter", owner = Model)]
struct Filter(String);

#[derive(DomainCommand)]
#[domain(id = "command", label = "Command", owner = Model)]
struct Command;

#[derive(DomainEvent)]
#[domain(id = "event", label = "Event", owner = Model)]
struct Event;

#[derive(DomainError)]
#[domain(id = "error", label = "Error", owner = Model, code = "ERROR", message = "Error.")]
struct Error;

struct Plain;

#[domain_queries(group = InvalidQueries)]
impl Model {
    #[query(id = "mutable-root", label = "Mutable root")]
    pub fn mutable_root(root: &mut Root) -> bool { true }

    #[query(id = "bad-name", label = "Bad name")]
    pub fn bad_name(candidate: &Root) -> bool { true }

    #[query(id = "receiver", label = "Receiver")]
    pub fn receiver(&self) -> bool { true }

    #[query(id = "private", label = "Private")]
    fn private(root: &Root) -> bool { true }

    #[query(id = "owned-input", label = "Owned input")]
    pub fn owned_input(root: &Root, input: String) -> bool { true }

    #[query(id = "excess", label = "Excess")]
    pub fn excess(root: &Root, input: &String, extra: &String) -> bool { true }

    #[query(id = "unit", label = "Unit")]
    pub fn unit(root: &Root) {}

    #[query(id = "reference", label = "Reference")]
    pub fn reference(root: &Root) -> &String { todo!() }

    #[query(id = "result", label = "Result")]
    pub fn result(root: &Root) -> Result<bool, Error> { Ok(true) }

    #[query(id = "event-output", label = "Event output")]
    pub fn event_output(root: &Root) -> Event { Event }

    #[query(id = "error-output", label = "Error output")]
    pub fn error_output(root: &Root) -> Error { Error }

    #[query(id = "plain-output", label = "Plain output")]
    pub fn plain_output(root: &Root) -> Plain { Plain }

    #[query(id = "command-input", label = "Command input")]
    pub fn command_input(root: &Root, input: &Command) -> bool { true }
}

#[domain_queries(group = Empty)]
impl Model {}

#[domain_queries(group = Unsupported)]
impl Plain {
    #[query(id = "plain", label = "Plain")]
    pub fn plain(root: &Root) -> bool { true }
}

fn main() {}
