#[path = "../read_model_support.rs"]
mod support;
use rostfrei::{ReadModelBackend, ReadModels};
use support::*;
rostfrei::install_macro_support!();

fn register<B: ReadModelBackend>(models: &ReadModels<B>) {
    let _ = models.register::<View>().from_aggregate::<Organization>()
        .on_domain_event::<ForeignEvent>(|_| "org-1".to_owned(), |_, _| {});
}
fn main() {}
