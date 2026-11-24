use rostfrei::domain_model;

use super::{
    BikeRental,
    bicycle_transfer::BicycleTransfer,
    rental_fleet::{
        Bicycle, BicycleAlreadyInFleet, BicycleCannotBeRetired, BicycleCategory, BicycleCondition,
        BicycleNotRented, BicycleTransferRejected, BicycleTransferRejectionReason,
        BicycleUnavailable, CollectionAssignment, CollectionRejected, RegistrationNumber,
        RentalFleet, RentalFleetAggregate, RentalPeriod, RentalReturn, Reservation,
        ReservationRejected, ReturnTime,
    },
    rental_payment::{
        CollectionPayment, PaymentOutcome, PaymentRejected, RentalPayment, RentalPaymentAggregate,
    },
    rental_tariff::{
        InvalidRentalTariff, RentalQuote, RentalTariff, RentalTariffAggregate, RentalTariffTerms,
    },
    repair_job::{RepairDetails, RepairJob, RepairJobAggregate, RepairJobAlreadyOpened},
};

pub fn domain_model() -> Result<serde_json::Value, rostfrei::DomainModelError> {
    domain_model! {
        contexts: [BikeRental],
        aggregates: [RentalFleetAggregate, RepairJobAggregate, RentalTariffAggregate, RentalPaymentAggregate],
        entities: [RentalFleet, Bicycle, Reservation, RepairJob, RentalTariff, RentalPayment],
        value_objects: [BicycleCondition, RegistrationNumber, BicycleTransferRejectionReason, RepairDetails, BicycleCategory, RentalPeriod, RentalTariffTerms, RentalQuote, CollectionPayment, CollectionAssignment, PaymentOutcome, RentalReturn, ReturnTime],
        services: [BicycleTransfer],
        errors: [
            BicycleUnavailable,
            BicycleNotRented,
            BicycleAlreadyInFleet,
            BicycleCannotBeRetired,
            BicycleTransferRejected,
            RepairJobAlreadyOpened,
            ReservationRejected,
            InvalidRentalTariff,
            CollectionRejected,
            PaymentRejected,
        ],
    }
}
