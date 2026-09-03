use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "add-bicycle", label = "Add bicycle")]
pub struct AddBicycle;
