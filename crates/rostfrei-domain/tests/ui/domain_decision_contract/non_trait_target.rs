use domain::domain_decisions;

struct Group;

#[domain_decisions(aggregate, group = Group)]
struct Owner;

fn main() {}
