#[path = "../../event_codec_support.rs"]
mod model;
use model::*;
rostfrei::install_macro_support!();

#[derive(rostfrei::DomainEvent)]
#[domain(id = "stock-added", label = "Not in the aggregate event set")]
struct Foreign;

// A user-supplied conversion is insufficient: membership also needs EventVariant.
impl From<Foreign> for InventoryEvents {
    fn from(_: Foreign) -> Self {
        Added { quantity: 1 }.into()
    }
}

fn main() {
    let _ = rostfrei::EventCodecBuilder::<Inventory>::new()
        .upcast(|_: AddedV1| Foreign)
        .build();
}
