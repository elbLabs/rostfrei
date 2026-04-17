use serde_json::{Value, json};

use crate::{InvariantDescriptor, InvariantId, InvariantOwnerId};

use super::id_projection::invariant_owner as invariant_owner_id;

pub(super) struct InvariantProjection {
    registered_owners: Vec<InvariantOwnerId>,
    invariants: Vec<(InvariantDescriptor, Value)>,
}

impl InvariantProjection {
    pub(super) const fn new() -> Self {
        Self {
            registered_owners: Vec::new(),
            invariants: Vec::new(),
        }
    }

    pub(super) fn register_owner(&mut self, owner: InvariantOwnerId) {
        if !self.registered_owners.contains(&owner) {
            self.registered_owners.push(owner);
        }
    }

    pub(super) fn add_group(
        &mut self,
        expected_owner: InvariantOwnerId,
        descriptors: &'static [InvariantDescriptor],
    ) {
        self.validate_registered_owner(expected_owner);
        self.validate_group(expected_owner, descriptors);
        self.invariants.extend(
            descriptors
                .iter()
                .map(|descriptor| (*descriptor, invariant(*descriptor))),
        );
    }

    pub(super) fn into_values(self) -> Vec<Value> {
        self.invariants
            .into_iter()
            .map(|(_, value)| value)
            .collect()
    }

    fn validate_registered_owner(&self, owner: InvariantOwnerId) {
        if !self.registered_owners.contains(&owner) {
            panic!("unregistered invariant owner: {owner:?}");
        }
    }

    fn validate_group(
        &self,
        expected_owner: InvariantOwnerId,
        descriptors: &'static [InvariantDescriptor],
    ) {
        for (index, descriptor) in descriptors.iter().enumerate() {
            Self::validate_owner(expected_owner, descriptor);
            self.validate_id(descriptor.id, descriptors.iter().take(index));
        }
    }

    fn validate_owner(expected_owner: InvariantOwnerId, descriptor: &InvariantDescriptor) {
        if descriptor.id.owner != expected_owner {
            panic!("invariant descriptor owner mismatch: {:?}", descriptor.id);
        }
    }

    fn validate_id<'a>(
        &self,
        id: InvariantId,
        preceding: impl Iterator<Item = &'a InvariantDescriptor>,
    ) {
        if self.has_id(id) || preceding.into_iter().any(|descriptor| descriptor.id == id) {
            panic!("duplicate InvariantId: {id:?}");
        }
    }

    fn has_id(&self, id: InvariantId) -> bool {
        self.invariants
            .iter()
            .any(|(descriptor, _)| descriptor.id == id)
    }
}

fn invariant(descriptor: InvariantDescriptor) -> Value {
    json!({
        "id": {
            "owner": invariant_owner_id(descriptor.id.owner),
            "local": descriptor.id.local,
        },
        "label": descriptor.label,
    })
}
