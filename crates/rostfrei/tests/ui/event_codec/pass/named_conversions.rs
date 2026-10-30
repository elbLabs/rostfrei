#[path = "../../event_codec_support.rs"]
mod model;
use model::*;
rostfrei::install_macro_support!();

pub struct InventoryCodec;

#[rostfrei::event_codec(aggregate = Inventory)]
impl InventoryCodec {
    #[cfg(any())]
    #[upcast]
    fn disabled(_: MissingHistoricalType) -> MissingCurrentType {
        unreachable!()
    }

    #[upcast]
    pub fn added(old: AddedV1) -> Added {
        added_v1(old)
    }

    #[upcast]
    pub fn retired(old: RetiredV1) -> std::result::Result<Retired, &'static str> {
        retired_v1(old)
    }

    pub fn ordinary_method(&self) -> bool {
        true
    }
}

fn main() {
    let mut codecs = rostfrei::AggregateCodecs::default();
    codecs
        .register::<Inventory, _>(InventoryCodec::builder().build().unwrap())
        .unwrap();
    assert_eq!(InventoryCodec::added(AddedV1 { count: 3 }).quantity, 3);
    assert!(InventoryCodec::retired(RetiredV1 { count: -1 }).is_err());
    assert!(InventoryCodec.ordinary_method());
}
