use domain::DecisionOutcome;

#[derive(DecisionOutcome)]
struct NotAnEnum;

#[derive(DecisionOutcome)]
enum GenericOutcome<T> {
    #[outcome(id = "value", label = "Value")]
    Value(T),
}

#[derive(DecisionOutcome)]
enum EmptyOutcome {}

fn main() {}
