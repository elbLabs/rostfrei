use crate::domain::rental_fleet::ReservationId;
use rostfrei::DomainError;

#[derive(DomainError, Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[domain(
    id = "collection-rejected",
    label = "Collection rejected",
    code = "COLLECTION_REJECTED",
    message = "The reserved bicycle cannot be collected."
)]
pub struct CollectionRejected {
    pub reservation_id: ReservationId,
    pub reason: String,
}
