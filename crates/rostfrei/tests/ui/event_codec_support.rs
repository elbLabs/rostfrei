#![allow(dead_code, clippy::needless_pass_by_value)] // Upcasts consume historical inputs.

use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[domain(id = "warehouse", label = "Warehouse")]
pub struct Warehouse;

#[derive(rostfrei::DomainIdentity)]
pub struct InventoryId(String);

#[derive(rostfrei::Entity)]
#[domain(id = "inventory-root", label = "Inventory root")]
pub struct InventoryRoot {
    id: InventoryId,
    pub available: u32,
}

impl rostfrei::EntityDefinition for InventoryRoot {
    type Owner = Inventory;
    type Identity = InventoryId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, rostfrei::DomainEvent)]
#[domain(id = "stock-added", label = "Stock added", schema_version = 2)]
pub struct Added {
    pub quantity: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, rostfrei::DomainEvent)]
#[domain(id = "stock-retired", label = "Stock retired", schema_version = 2)]
pub struct Retired {
    pub quantity: u32,
    pub reason: String,
}

// Historical payloads deliberately do not belong to InventoryEvents.
#[derive(Deserialize, rostfrei::DomainEvent)]
#[domain(id = "stock-added", label = "Stock added v1")]
pub struct AddedV1 {
    pub count: u32,
}

#[derive(Deserialize, rostfrei::DomainEvent)]
#[domain(id = "stock-retired", label = "Stock retired v1")]
pub struct RetiredV1 {
    pub count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, rostfrei::AggregateEvents)]
pub enum InventoryEvents {
    Added(Added),
    Retired(Retired),
}

#[derive(rostfrei::Aggregate)]
#[domain(id = "inventory", label = "Inventory")]
pub struct Inventory;

impl rostfrei::AggregateDefinition for Inventory {
    type Context = Warehouse;
    type Root = InventoryRoot;
    type Event = InventoryEvents;
}

impl rostfrei::Initialize<Inventory> for InventoryRoot {
    fn initialize(stream: &rostfrei::StreamId) -> Self {
        Self {
            id: InventoryId(stream.aggregate_id().as_str().to_owned()),
            available: 0,
        }
    }
}

impl rostfrei::Apply<Added> for InventoryRoot {
    fn apply(&mut self, event: &Added) {
        self.available = self.available.saturating_add(event.quantity);
    }
}

impl rostfrei::Apply<Retired> for InventoryRoot {
    fn apply(&mut self, event: &Retired) {
        self.available = self.available.saturating_sub(event.quantity);
    }
}

pub const fn added_v1(old: AddedV1) -> Added {
    Added {
        quantity: old.count,
    }
}

pub fn retired_v1(old: RetiredV1) -> Result<Retired, &'static str> {
    let quantity = u32::try_from(old.count).map_err(|_| "negative historical quantity")?;
    Ok(Retired {
        quantity,
        reason: "unrecorded".to_owned(),
    })
}
