struct EntityChanged;

#[rostfrei::domain_actions(entity)]
trait EntityActions {
    #[action(id = "change", label = "Change", raises = [EntityChanged])]
    fn change(&mut self);
}

fn main() {}
