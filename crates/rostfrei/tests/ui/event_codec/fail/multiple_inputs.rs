#[path = "../../event_codec_support.rs"]
mod model;
#[allow(unused_imports)]
use model::*;
rostfrei::install_macro_support!();

struct Codec;
#[rostfrei::event_codec(aggregate = Inventory)]
impl Codec {
    #[upcast]
    fn convert(old: AddedV1, _: u32) -> Added {
        added_v1(old)
    }
}
fn main() {}
