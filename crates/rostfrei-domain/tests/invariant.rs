#![allow(dead_code, non_snake_case)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, InvariantOwnerId, InvariantOwnerType,
    InvariantViolation, ValueObject, domain_invariants,
};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[domain_invariants(aggregate)]
trait InventoryBounds {
    #[invariant(id = "stock-nonnegative", label = "Stock is nonnegative")]
    fn stock_nonnegative(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;

    #[invariant(id = "reserved-nonnegative", label = "Reserved is nonnegative")]
    fn reserved_nonnegative(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

#[domain_invariants(aggregate)]
trait AllocationBounds {
    #[invariant(id = "reservation-within-stock", label = "Reservation is within stock")]
    fn reservation_within_stock(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;

    #[invariant(id = "available-nonnegative", label = "Available is nonnegative")]
    fn available_nonnegative(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

#[derive(Clone, DomainIdentity)]
#[domain(owner = ProductRoot)]
struct ProductId(u64);

#[derive(Clone, Entity)]
#[domain(id = "product-root", label = "Product", owner = Product)]
struct ProductRoot {
    #[domain(identity)]
    id: ProductId,
    stock: i32,
    reserved: i32,
}

#[derive(Aggregate)]
#[domain(
    id = "product",
    label = "Product",
    context = Catalog,
    root = ProductRoot,
    invariants = [InventoryBounds, AllocationBounds]
)]
struct Product;

impl InventoryBounds for Product {
    fn stock_nonnegative(candidate: &ProductRoot) -> Option<InvariantViolation> {
        (candidate.stock < 0).then(|| InvariantViolation::new("stock", "must be nonnegative"))
    }

    fn reserved_nonnegative(candidate: &ProductRoot) -> Option<InvariantViolation> {
        (candidate.reserved < 0).then(|| InvariantViolation::new("reserved", "must be nonnegative"))
    }
}

impl AllocationBounds for Product {
    fn reservation_within_stock(candidate: &ProductRoot) -> Option<InvariantViolation> {
        (candidate.reserved > candidate.stock)
            .then(|| InvariantViolation::new("reserved", "must not exceed stock"))
    }

    fn available_nonnegative(candidate: &ProductRoot) -> Option<InvariantViolation> {
        (candidate.stock - candidate.reserved < 0)
            .then(|| InvariantViolation::new("available", "must be nonnegative"))
    }
}

#[domain_invariants(entity)]
trait LineBounds {
    #[invariant(id = "positive-quantity", label = "Quantity is positive")]
    fn positive_quantity(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

#[derive(DomainIdentity)]
#[domain(owner = Line)]
struct LineId(u64);

#[derive(Entity)]
#[domain(
    id = "line",
    label = "Line",
    owner = Product,
    invariants = [LineBounds]
)]
struct Line {
    #[domain(identity)]
    id: LineId,
    quantity: i32,
}

impl LineBounds for Line {
    fn positive_quantity(candidate: &Line) -> Option<InvariantViolation> {
        (candidate.quantity <= 0).then(|| InvariantViolation::new("quantity", "must be positive"))
    }
}

#[domain_invariants(value_object)]
trait SkuBounds {
    #[invariant(id = "not-blank", label = "SKU is not blank")]
    fn not_blank(candidate: &<Self as InvariantOwnerType>::Candidate)
    -> Option<InvariantViolation>;
}

#[derive(ValueObject)]
#[domain(
    id = "sku",
    label = "SKU",
    owner = Catalog,
    invariants = [SkuBounds]
)]
struct Sku(String);

impl SkuBounds for Sku {
    fn not_blank(candidate: &Sku) -> Option<InvariantViolation> {
        candidate
            .0
            .trim()
            .is_empty()
            .then(|| InvariantViolation::new("sku", "must not be blank"))
    }
}

mod change_inventory_action {
    use super::{InvariantOwnerType, InvariantViolation, Product, ProductRoot};

    #[derive(Debug, Eq, PartialEq)]
    pub enum ChangeInventoryError {
        InvalidCandidate(Vec<InvariantViolation>),
    }

    pub struct ChangeInventory;

    impl ChangeInventory {
        pub fn execute(
            current: &mut ProductRoot,
            stock: i32,
            reserved: i32,
        ) -> Result<(), ChangeInventoryError> {
            let mut candidate = current.clone();
            candidate.stock = stock;
            candidate.reserved = reserved;
            <Product as InvariantOwnerType>::validate_invariants(&candidate)
                .map_err(Self::translate_violations)?;
            *current = candidate;
            Ok(())
        }

        fn translate_violations(violations: Vec<InvariantViolation>) -> ChangeInventoryError {
            ChangeInventoryError::InvalidCandidate(violations)
        }
    }
}

fn product(stock: i32, reserved: i32) -> ProductRoot {
    ProductRoot {
        id: ProductId(1),
        stock,
        reserved,
    }
}

#[test]
fn exposes_the_candidate_type_and_owner_kind_for_each_supported_owner() {
    fn candidate_is<Owner, Candidate>()
    where
        Owner: InvariantOwnerType<Candidate = Candidate>,
    {
    }

    candidate_is::<Product, ProductRoot>();
    candidate_is::<Line, Line>();
    candidate_is::<Sku, Sku>();

    assert!(matches!(
        Product::INVARIANT_OWNER_ID,
        InvariantOwnerId::Aggregate(_)
    ));
    assert!(matches!(
        Line::INVARIANT_OWNER_ID,
        InvariantOwnerId::Entity(_)
    ));
    assert!(matches!(
        Sku::INVARIANT_OWNER_ID,
        InvariantOwnerId::ValueObject(_)
    ));
}

#[test]
fn returns_ok_or_a_nonempty_error_for_every_owner_kind() {
    assert_eq!(
        <Product as InvariantOwnerType>::validate_invariants(&product(5, 2)),
        Ok(())
    );
    assert_eq!(
        <Line as InvariantOwnerType>::validate_invariants(&Line {
            id: LineId(1),
            quantity: 1,
        }),
        Ok(())
    );
    assert_eq!(
        <Sku as InvariantOwnerType>::validate_invariants(&Sku("ABC-123".into())),
        Ok(())
    );

    let invalid_results = [
        <Product as InvariantOwnerType>::validate_invariants(&product(-3, -1)),
        <Line as InvariantOwnerType>::validate_invariants(&Line {
            id: LineId(1),
            quantity: 0,
        }),
        <Sku as InvariantOwnerType>::validate_invariants(&Sku("  ".into())),
    ];

    for result in invalid_results {
        match result {
            Err(violations) => assert!(!violations.is_empty()),
            Ok(()) => panic!("invalid candidate passed invariant validation"),
        }
    }
}

#[test]
fn collects_all_violations_in_attachment_then_method_source_order() {
    assert_eq!(
        <Product as InvariantOwnerType>::validate_invariants(&product(-3, -1)),
        Err(vec![
            InvariantViolation::new("stock", "must be nonnegative"),
            InvariantViolation::new("reserved", "must be nonnegative"),
            InvariantViolation::new("reserved", "must not exceed stock"),
            InvariantViolation::new("available", "must be nonnegative"),
        ])
    );
}

#[test]
fn action_stages_translates_and_commits_only_after_successful_validation() {
    use change_inventory_action::{ChangeInventory, ChangeInventoryError};

    let mut current = product(5, 2);
    let denied = ChangeInventory::execute(&mut current, -3, -1);

    assert_eq!(
        denied,
        Err(ChangeInventoryError::InvalidCandidate(vec![
            InvariantViolation::new("stock", "must be nonnegative"),
            InvariantViolation::new("reserved", "must be nonnegative"),
            InvariantViolation::new("reserved", "must not exceed stock"),
            InvariantViolation::new("available", "must be nonnegative"),
        ]))
    );
    assert_eq!((current.stock, current.reserved), (5, 2));

    assert_eq!(ChangeInventory::execute(&mut current, 10, 4), Ok(()));
    assert_eq!((current.stock, current.reserved), (10, 4));
}
