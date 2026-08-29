use domain::DecisionOutcome;

#[derive(DecisionOutcome)]
enum MissingAttribute {
    Missing,
}

#[derive(DecisionOutcome)]
enum MissingId {
    #[outcome(label = "Missing ID")]
    Missing,
}

#[derive(DecisionOutcome)]
enum MissingLabel {
    #[outcome(id = "missing-label")]
    Missing,
}

fn main() {}
