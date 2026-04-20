use domain::DecisionOutcome;

#[derive(DecisionOutcome)]
enum DuplicateIdKey {
    #[outcome(id = "first", id = "second", label = "Duplicate ID key")]
    Duplicate,
}

#[derive(DecisionOutcome)]
enum DuplicateLabelKey {
    #[outcome(id = "duplicate-label", label = "First", label = "Second")]
    Duplicate,
}

#[derive(DecisionOutcome)]
enum DuplicateLocalId {
    #[outcome(id = "same", label = "First")]
    First,
    #[outcome(id = "same", label = "Second")]
    Second,
}

fn main() {}
