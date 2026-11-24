use rostfrei::DomainError;

use crate::domain::rental_fleet::ReservationId;

#[derive(DomainError, Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[domain(
    id = "reservation-rejected",
    label = "Reservation rejected",
    code = "RESERVATION_REJECTED",
    message = "The requested reservation is not allowed."
)]
pub struct ReservationRejected {
    pub reservation_id: ReservationId,
    pub reason: String,
}
