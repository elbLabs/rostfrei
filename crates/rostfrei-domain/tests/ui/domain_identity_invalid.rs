use domain::DomainIdentity;

#[derive(DomainIdentity)]
struct Named { value: u64 }

#[derive(DomainIdentity)]
struct Empty();

#[derive(DomainIdentity)]
struct Multiple(u64, u64);

#[derive(DomainIdentity)]
struct Generic<T>(T);

fn main() {}
