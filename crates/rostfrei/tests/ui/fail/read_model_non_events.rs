#[path = "../read_model_support.rs"]
mod support;
use rostfrei::{ReadModelBackend, ReadModels};
use support::*;
rostfrei::install_macro_support!();

fn register<B: ReadModelBackend>(models: &ReadModels<B>) {
    let _ = models.register::<View>()
        .on_integration_event::<ReserveCycle>(|_| "fleet-1".to_owned(), |_, _| {});
    let _ = models.register::<View>()
        .on_integration_event::<FindCycle>(|_| "fleet-1".to_owned(), |_, _| {});
    let _ = models.register::<View>().from_aggregate::<RentalFleet>()
        .on_domain_event::<ReserveCycle>(|_| "fleet-1".to_owned(), |_, _| {});
    let _ = models.register::<View>().from_aggregate::<RentalFleet>()
        .on_domain_event::<FindCycle>(|_| "fleet-1".to_owned(), |_, _| {});
}
fn main() {}
