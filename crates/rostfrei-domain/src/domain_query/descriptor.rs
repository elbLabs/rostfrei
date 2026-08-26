use super::{QueryId, QueryInputDescriptor, QueryOutputDescriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryDescriptor {
    pub id: QueryId,
    pub label: &'static str,
    pub input: Option<QueryInputDescriptor>,
    pub output: QueryOutputDescriptor,
}
