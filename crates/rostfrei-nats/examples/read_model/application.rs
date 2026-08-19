//! Business data and event transformations. Persistence and checkpoints are automatic.

use async_trait::async_trait;
use rostfrei::{
    QueryErrorPayload, QueryHandler, QueryHandlerRequest, ReadModelBackend, ReadModelKey,
    ReadModelProcessingError, ReadModelReader, ReadModelRuntime, ReadModels,
};
use serde::{Deserialize, Serialize};

use super::source::{BillingChanged, DemoChanged, MemberJoined, Organization};

#[derive(Debug, Default, Deserialize, Eq, PartialEq, Serialize, rostfrei::ReadModel)]
#[read_model(id = "organization-access", version = 1)]
pub struct OrganizationAccess {
    pub members: u64,
    pub demo: bool,
    pub paid: bool,
}

pub async fn register<B: ReadModelBackend>(
    read_models: &ReadModels<B>,
) -> Result<ReadModelRuntime<OrganizationAccess>, ReadModelProcessingError> {
    read_models
        .register::<OrganizationAccess>()
        .from_aggregate::<Organization>()
        .try_on_domain_event::<MemberJoined>(
            |event| event.organization_id.clone(),
            |view, _event| {
                view.members = view.members.checked_add(1).ok_or_else(|| {
                    ReadModelProcessingError::Transformation("member count overflow".to_owned())
                })?;
                Ok(())
            },
        )
        .on_domain_event::<DemoChanged>(
            |event| event.organization_id.clone(),
            |view, event| view.demo = event.active,
        )
        .on_integration_event::<BillingChanged>(
            |event| event.organization_id.clone(),
            |view, event| view.paid = event.paid,
        )
        .build()
        .await
}

pub struct GetAccess {
    pub reader: ReadModelReader<OrganizationAccess>,
}

#[async_trait]
impl QueryHandler<String, Option<OrganizationAccess>> for GetAccess {
    async fn handle(
        &self,
        request: QueryHandlerRequest<String>,
    ) -> Result<Option<OrganizationAccess>, QueryErrorPayload> {
        let key = ReadModelKey::new(request.payload())
            .map_err(|_| QueryErrorPayload::internal_error())?;
        self.reader
            .read(&key)
            .await
            .map_err(|_| QueryErrorPayload::internal_error())
    }
}
