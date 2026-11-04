#[path = "../../event_codec_support.rs"]
mod model;
use model::*;
rostfrei::install_macro_support!();

#[derive(serde::Deserialize)]
struct OldPayload {
    count: u32,
}

struct Codec;
#[rostfrei::event_codec(aggregate = Inventory)]
impl Codec {
    #[upcast]
    fn convert(old: OldPayload) -> Added {
        Added {
            quantity: old.count,
        }
    }
}
fn main() {}
