use domain::{ActionDescriptor, ActionId, domain_action};

#[domain_action(id = "rename", label = "Rename")]
trait Rename {
    fn rename(&mut self, name: String);
}

struct Item(String);

impl Rename for Item {
    fn rename(&mut self, name: String) {
        self.0 = name;
    }
}

fn main() {
    let mut item = Item(String::new());
    item.rename("new".to_owned());
    let descriptor: ActionDescriptor = <Item as Rename>::DESCRIPTOR;
    assert_eq!(descriptor.id, ActionId("rename"));
}
rostfrei_domain_macros::__install_test_macro_support!();
