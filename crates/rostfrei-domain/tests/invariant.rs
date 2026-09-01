#![allow(dead_code, non_snake_case)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, InvariantId, InvariantViolation,
    ValueObject, domain_invariants,
};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[domain_invariants]
trait InventoryBounds {
    #[invariant(id = "stock-nonnegative", label = "Stock is nonnegative")]
    fn stock_nonnegative(candidate: &ProductRoot) -> Option<InvariantViolation>;

    #[invariant(id = "reserved-nonnegative", label = "Reserved is nonnegative")]
    fn reserved_nonnegative(candidate: &ProductRoot) -> Option<InvariantViolation>;
}

#[domain_invariants]
trait AllocationBounds {
    #[invariant(id = "reservation-within-stock", label = "Reservation is within stock")]
    fn reservation_within_stock(candidate: &ProductRoot) -> Option<InvariantViolation>;

    #[invariant(id = "available-nonnegative", label = "Available is nonnegative")]
    fn available_nonnegative(candidate: &ProductRoot) -> Option<InvariantViolation>;
}

#[derive(Clone, DomainIdentity)]
#[domain(owner = ProductRoot)]
struct ProductId(u64);

#[derive(Clone, Entity)]
#[domain(id = "product-root", label = "Product")]
struct ProductRoot {
    #[domain(identity)]
    id: ProductId,
    stock: i32,
    reserved: i32,
}

impl domain::EntityDefinition for ProductRoot {
    type Owner = Product;
    type Identity = ProductId;
}

#[derive(Aggregate)]
#[domain(id = "product", label = "Product")]
struct Product;

impl domain::AggregateDefinition for Product {
    type Context = Catalog;
    type Root = ProductRoot;
    type Event = domain::NoDomainEvents;
}

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
        (candidate.stock < candidate.reserved)
            .then(|| InvariantViolation::new("available", "must be nonnegative"))
    }
}

fn validate_product(candidate: &ProductRoot) -> Result<(), Vec<InvariantViolation>> {
    let violations = [
        <Product as InventoryBounds>::stock_nonnegative(candidate),
        <Product as InventoryBounds>::reserved_nonnegative(candidate),
        <Product as AllocationBounds>::reservation_within_stock(candidate),
        <Product as AllocationBounds>::available_nonnegative(candidate),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

#[domain_invariants]
trait LineBounds {
    #[invariant(id = "positive-quantity", label = "Quantity is positive")]
    fn positive_quantity(candidate: &Line) -> Option<InvariantViolation>;
}

#[derive(DomainIdentity)]
#[domain(owner = Line)]
struct LineId(u64);

#[derive(Entity)]
#[domain(id = "line", label = "Line")]
struct Line {
    #[domain(identity)]
    id: LineId,
    quantity: i32,
}

impl domain::EntityDefinition for Line {
    type Owner = Product;
    type Identity = LineId;
}

impl LineBounds for Line {
    fn positive_quantity(candidate: &Self) -> Option<InvariantViolation> {
        (candidate.quantity <= 0).then(|| InvariantViolation::new("quantity", "must be positive"))
    }
}

#[domain_invariants]
trait SkuBounds {
    #[invariant(id = "not-blank", label = "SKU is not blank")]
    fn not_blank(candidate: &Sku) -> Option<InvariantViolation>;
}

#[derive(ValueObject)]
#[domain(id = "sku", label = "SKU", owner = Catalog)]
struct Sku(String);

impl SkuBounds for Sku {
    fn not_blank(candidate: &Self) -> Option<InvariantViolation> {
        candidate
            .0
            .trim()
            .is_empty()
            .then(|| InvariantViolation::new("sku", "must not be blank"))
    }
}

mod change_inventory_action {
    use super::{InvariantViolation, ProductRoot, validate_product};

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
            validate_product(&candidate).map_err(Self::translate_violations)?;
            *current = candidate;
            Ok(())
        }

        const fn translate_violations(violations: Vec<InvariantViolation>) -> ChangeInventoryError {
            ChangeInventoryError::InvalidCandidate(violations)
        }
    }
}

const fn product(stock: i32, reserved: i32) -> ProductRoot {
    ProductRoot {
        id: ProductId(1),
        stock,
        reserved,
    }
}

#[test]
fn invariant_metadata_is_owner_independent() {
    assert_eq!(
        <Product as InventoryBounds>::__DOMAIN_INVARIANTS[0].id,
        InvariantId("stock-nonnegative")
    );
    assert_eq!(
        <Line as LineBounds>::__DOMAIN_INVARIANTS[0].id,
        InvariantId("positive-quantity")
    );
    assert_eq!(
        <Sku as SkuBounds>::__DOMAIN_INVARIANTS[0].id,
        InvariantId("not-blank")
    );
}

#[test]
fn ordinary_invariant_methods_remain_callable() {
    assert_eq!(validate_product(&product(5, 2)), Ok(()));
    assert_eq!(
        <Line as LineBounds>::positive_quantity(&Line {
            id: LineId(1),
            quantity: 1,
        }),
        None
    );
    assert_eq!(<Sku as SkuBounds>::not_blank(&Sku("ABC-123".into())), None);

    assert!(validate_product(&product(-3, -1)).is_err());
    assert!(
        <Line as LineBounds>::positive_quantity(&Line {
            id: LineId(1),
            quantity: 0,
        })
        .is_some()
    );
    assert!(<Sku as SkuBounds>::not_blank(&Sku("  ".into())).is_some());
}

#[test]
fn explicit_composition_collects_violations_in_contract_order() {
    assert_eq!(
        validate_product(&product(-3, -1)),
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
