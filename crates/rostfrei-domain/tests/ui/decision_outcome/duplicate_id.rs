use domain::DecisionOutcome;

#[derive(DecisionOutcome)]
enum Outcome {
    #[outcome(id = "same", label = "First")]
    First,
    #[outcome(id = "same", label = "Second")]
    Second,
}

fn main() {}
