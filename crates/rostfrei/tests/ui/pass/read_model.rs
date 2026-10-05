#![allow(dead_code)]
#[path = "../read_model_support.rs"]
mod support;

use rostfrei::{IntegrationEventOrder, ReadModelBackend, ReadModels};
use support::*;
rostfrei::install_macro_support!();

fn register<B: ReadModelBackend>(models: &ReadModels<B>, producer: rostfrei::BoundedContext) {
    let _builder = models.register::<View>()
        .from_aggregate::<Organization>()
        .on_domain_event::<Changed>(|event| event.id.clone(), |view, _| view.enabled = true)
        .on_integration_event::<PublicChanged>(|event| event.id.clone(), |view, _| view.enabled = false)
        .integration_source::<PublicChanged>(producer, IntegrationEventOrder::<PublicChanged>::latest("access", |event| event.version));
}

fn main() {
    assert_eq!(<View as rostfrei::ReadModel>::NAME, "access-view");
    assert_eq!(<View as rostfrei::ReadModel>::SCHEMA_VERSION, 1);
}
