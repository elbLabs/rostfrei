use std::{env, sync::Arc};

use bike_rental::{
    rental::RentBicycle,
    runtime::{control_plane_builder, seed_demo},
    ui,
};
use rostfrei::{EventHistory, InMemoryEventStore};
use rostfrei_control_plane::{
    ExposeTracePayloadsForLocalDevelopment,
    http::{self, HttpConfig},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = InMemoryEventStore::new();
    seed_demo(&store).await?;

    let history: Arc<dyn EventHistory> = Arc::new(store);
    let mut builder = control_plane_builder(history)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder.register_json::<RentBicycle>()?;
    let control_plane = builder.build()?;
    let api_token = env::var("ROSTFREI_API_TOKEN")?;
    let app = ui::router().merge(http::router(control_plane, HttpConfig::new(api_token)?));

    let address = env::var("ROSTFREI_API_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("bike-rental control plane listening on http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}
