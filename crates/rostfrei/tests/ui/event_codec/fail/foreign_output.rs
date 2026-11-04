#[path = "../../event_codec_support.rs"]
mod model;
use model::*;
rostfrei::install_macro_support!();

#[derive(rostfrei::DomainEvent)]
#[domain(id = "stock-added", label = "Not in the aggregate event set")]
struct Foreign;

struct Codec;
#[rostfrei::event_codec(aggregate = Inventory)]
impl Codec {
    #[upcast]
    fn convert(_: AddedV1) -> Foreign {
        Foreign
    }
}
fn main() {}
