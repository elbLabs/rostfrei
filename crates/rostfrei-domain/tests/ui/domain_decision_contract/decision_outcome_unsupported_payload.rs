use domain::DecisionOutcome;

struct Unsupported;

#[derive(DecisionOutcome)]
enum UnsupportedPayload {
    #[outcome(id = "custom", label = "Custom")]
    Custom(Unsupported),
}

#[derive(DecisionOutcome)]
enum ReferencedPayload {
    #[outcome(id = "borrowed", label = "Borrowed")]
    Borrowed(&'static u8),
}

fn main() {}
