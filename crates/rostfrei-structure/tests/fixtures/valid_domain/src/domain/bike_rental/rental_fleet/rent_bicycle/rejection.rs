#[derive(DomainError)]
#[domain(
    id = "bicycle-unavailable",
    label = "Bicycle unavailable",
    code = "BICYCLE_UNAVAILABLE",
    message = "The bicycle is unavailable."
)]
pub struct BicycleUnavailable;
