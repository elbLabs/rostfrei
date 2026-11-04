#[path = "../../event_codec_support.rs"]
mod model;
#[allow(unused_imports)]
use model::*;
rostfrei::install_macro_support!();

struct Codec<T>(std::marker::PhantomData<T>);
#[rostfrei::event_codec(aggregate = Inventory)]
impl<T> Codec<T> {
    #[upcast]
    fn convert(old: AddedV1) -> Added {
        added_v1(old)
    }
}
fn main() {}
