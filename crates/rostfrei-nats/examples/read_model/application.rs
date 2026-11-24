//! Business data and event transformations. Persistence and checkpoints are automatic.

use async_trait::async_trait;
use rostfrei_runtime::{
    QueryErrorPayload, QueryHandler, QueryHandlerRequest, ReadModelBackend, ReadModelKey,
    ReadModelProcessingError, ReadModelReader, ReadModelRuntime, ReadModels,
};
use serde::{Deserialize, Serialize};

use super::source::{BicycleRegistered, Fleet, InspectionChanged, RentalWindowChanged};

#[derive(Debug, Default, Deserialize, Eq, PartialEq, Serialize, rostfrei_runtime::ReadModel)]
#[read_model(id = "fleet-status", version = 1)]
pub struct FleetStatus {
    pub bicycles_registered: u64,
    pub rentals_open: bool,
    pub inspection_passed: bool,
}

pub async fn register<B: ReadModelBackend>(
    read_models: &ReadModels<B>,
) -> Result<ReadModelRuntime<FleetStatus>, ReadModelProcessingError> {
    read_models
        .register::<FleetStatus>()
        .from_aggregate::<Fleet>()
        .try_on_domain_event::<BicycleRegistered>(
            |event| event.fleet_id.clone(),
            |view, _event| {
                view.bicycles_registered =
                    view.bicycles_registered.checked_add(1).ok_or_else(|| {
                        ReadModelProcessingError::Transformation(
                            "registration count overflow".to_owned(),
                        )
                    })?;
                Ok(())
            },
        )
        .on_domain_event::<RentalWindowChanged>(
            |event| event.fleet_id.clone(),
            |view, event| view.rentals_open = event.open,
        )
        .on_integration_event::<InspectionChanged>(
            |event| event.fleet_id.clone(),
            |view, event| view.inspection_passed = event.inspection_passed,
        )
        .build()
        .await
}

pub struct GetFleetStatus {
    pub reader: ReadModelReader<FleetStatus>,
}

#[async_trait]
impl QueryHandler<String, Option<FleetStatus>> for GetFleetStatus {
    async fn handle(
        &self,
        request: QueryHandlerRequest<String>,
    ) -> Result<Option<FleetStatus>, QueryErrorPayload> {
        let key = ReadModelKey::new(request.payload())
            .map_err(|_| QueryErrorPayload::internal_error())?;
        self.reader
            .read(&key)
            .await
            .map_err(|_| QueryErrorPayload::internal_error())
    }
}
