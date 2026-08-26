use rostfrei_domain::{ActionDescriptor, extension::ActionGroupType};

struct PlainOwner;
struct PlainOwnerActions;

impl ActionGroupType for PlainOwnerActions {
    type Owner = PlainOwner;

    const ACTIONS: &'static [ActionDescriptor] = &[];
}

fn main() {}
