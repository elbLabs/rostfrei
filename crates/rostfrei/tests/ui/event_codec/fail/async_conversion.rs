#[path = "../../event_codec_support.rs"]
mod model;
#[allow(unused_imports)]
use model::*;
rostfrei::install_macro_support!();

struct Codec;
#[rostfrei::event_codec(aggregate = Inventory)]
impl Codec {
    #[upcast]
    async fn convert(old: AddedV1) -> Added {
        added_v1(old)
    }
}
fn main() {}
