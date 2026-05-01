use super::ImportedBicycle;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportRentalFleetInput {
    pub(super) bicycles: Vec<ImportedBicycle>,
}

impl ImportRentalFleetInput {
    pub const fn new(bicycles: Vec<ImportedBicycle>) -> Self {
        Self { bicycles }
    }
}
