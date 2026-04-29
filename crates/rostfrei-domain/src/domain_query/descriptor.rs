use super::QueryId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryDescriptor {
    pub id: QueryId,
    pub label: &'static str,
}
