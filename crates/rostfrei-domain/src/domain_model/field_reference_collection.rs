use std::fmt;

use crate::{
    CommandDescriptor, CommandId, DomainErrorDescriptor, DomainErrorId, DomainEventDescriptor,
    DomainEventId, EntityDescriptor, EntityId, FieldDescriptor, FieldKind, ValueObjectDescriptor,
    ValueObjectId, ValueObjectShapeDescriptor, ValueObjectVariantShapeDescriptor,
};

#[derive(Clone, Copy)]
pub(super) enum FieldReference {
    DomainIdentity(crate::DomainIdentityId),
    Entity(EntityId),
    ValueObject(ValueObjectId),
    Aggregate(crate::AggregateId),
}

#[derive(Clone, Copy)]
enum FieldDescriptorOwner {
    Entity(EntityId),
    ValueObject(ValueObjectId),
    Command(CommandId),
    DomainEvent(DomainEventId),
    DomainError(DomainErrorId),
}

#[derive(Clone, Copy)]
pub(super) struct FieldDescriptorLocation {
    owner: FieldDescriptorOwner,
    variant: Option<&'static str>,
    field: &'static str,
}

impl fmt::Display for FieldDescriptorLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.owner {
            FieldDescriptorOwner::Entity(id) => write!(formatter, "entity {id:?}"),
            FieldDescriptorOwner::ValueObject(id) => write!(formatter, "value object {id:?}"),
            FieldDescriptorOwner::Command(id) => write!(formatter, "command {id:?}"),
            FieldDescriptorOwner::DomainEvent(id) => write!(formatter, "domain event {id:?}"),
            FieldDescriptorOwner::DomainError(id) => write!(formatter, "domain error {id:?}"),
        }?;
        if let Some(variant) = self.variant {
            write!(formatter, " variant {variant:?}")?;
        }
        write!(formatter, " field {:?}", self.field)
    }
}

#[derive(Clone, Copy)]
pub(super) struct FieldReferenceRecord {
    pub(super) reference: FieldReference,
    pub(super) location: FieldDescriptorLocation,
}

pub(super) struct FieldReferenceCollection {
    records: Vec<FieldReferenceRecord>,
}

impl FieldReferenceCollection {
    pub(super) const fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    pub(super) fn add_entity(&mut self, descriptor: EntityDescriptor) {
        self.add_fields(
            FieldDescriptorOwner::Entity(descriptor.id),
            None,
            descriptor.fields,
        );
    }

    pub(super) fn add_value_object(&mut self, descriptor: ValueObjectDescriptor) {
        let owner = FieldDescriptorOwner::ValueObject(descriptor.id);
        match descriptor.shape {
            ValueObjectShapeDescriptor::Struct { fields } => self.add_fields(owner, None, fields),
            ValueObjectShapeDescriptor::Enum { .. } => {}
            ValueObjectShapeDescriptor::TaggedEnum { variants } => {
                for variant in variants {
                    match variant.shape {
                        ValueObjectVariantShapeDescriptor::Unit => {}
                        ValueObjectVariantShapeDescriptor::Tuple { fields }
                        | ValueObjectVariantShapeDescriptor::Struct { fields } => {
                            self.add_fields(owner, Some(variant.name), fields);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn add_command(&mut self, descriptor: CommandDescriptor) {
        self.add_fields(
            FieldDescriptorOwner::Command(descriptor.id),
            None,
            descriptor.fields,
        );
    }

    pub(super) fn add_domain_event(&mut self, descriptor: DomainEventDescriptor) {
        self.add_fields(
            FieldDescriptorOwner::DomainEvent(descriptor.id),
            None,
            descriptor.fields,
        );
    }

    pub(super) fn add_domain_error(&mut self, descriptor: DomainErrorDescriptor) {
        self.add_fields(
            FieldDescriptorOwner::DomainError(descriptor.id),
            None,
            descriptor.fields,
        );
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = FieldReferenceRecord> + '_ {
        self.records.iter().copied()
    }

    fn add_fields(
        &mut self,
        owner: FieldDescriptorOwner,
        variant: Option<&'static str>,
        fields: &'static [FieldDescriptor],
    ) {
        for field in fields {
            let reference = match field.value.kind {
                FieldKind::Scalar(_) | FieldKind::SemanticScalar(_) => continue,
                FieldKind::DomainIdentity(id) => FieldReference::DomainIdentity(id),
                FieldKind::Entity(id) => FieldReference::Entity(id),
                FieldKind::ValueObject(id) => FieldReference::ValueObject(id),
                FieldKind::AggregateReference(id) => FieldReference::Aggregate(id),
            };
            self.records.push(FieldReferenceRecord {
                reference,
                location: FieldDescriptorLocation {
                    owner,
                    variant,
                    field: field.name,
                },
            });
        }
    }
}
