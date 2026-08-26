use crate::AggregateType;

use super::QueryDescriptor;

pub trait QueryGroupType: 'static {
    type Owner: AggregateType;

    const QUERIES: &'static [QueryDescriptor];
}
