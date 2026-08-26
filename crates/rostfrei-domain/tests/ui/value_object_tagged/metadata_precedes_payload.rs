use domain::ValueObject;

struct Context;
struct Custom;

#[derive(ValueObject)]
#[domain(label = "Invalid", owner = Context)]
enum Invalid {
    Custom(Custom),
}

fn main() {}
