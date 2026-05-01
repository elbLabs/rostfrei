use domain::{ActionDescriptor, ActionId, domain_action};

#[domain_action(id = "rent-bicycle", label = "Rent bicycle")]
trait RentBicycle {
    fn rent(&mut self);
}

#[derive(Default)]
struct Bicycle {
    rented: bool,
}

impl RentBicycle for Bicycle {
    fn rent(&mut self) {
        self.rented = true;
    }
}

struct RentalFleet;

impl RentBicycle for RentalFleet {
    fn rent(&mut self) {}
}

#[test]
fn annotated_trait_keeps_ordinary_behavior_and_global_metadata() {
    let mut bicycle = Bicycle::default();
    bicycle.rent();

    assert!(bicycle.rented);
    assert_eq!(
        <Bicycle as RentBicycle>::DESCRIPTOR,
        ActionDescriptor {
            id: ActionId("rent-bicycle"),
            label: "Rent bicycle",
        }
    );
    assert_eq!(<Bicycle as RentBicycle>::LOCAL_ID, "rent-bicycle");
    assert_eq!(<Bicycle as RentBicycle>::LABEL, "Rent bicycle");
    assert_eq!(
        <Bicycle as RentBicycle>::DESCRIPTOR,
        <RentalFleet as RentBicycle>::DESCRIPTOR
    );
}
rostfrei_domain_macros::__install_test_macro_support!();
