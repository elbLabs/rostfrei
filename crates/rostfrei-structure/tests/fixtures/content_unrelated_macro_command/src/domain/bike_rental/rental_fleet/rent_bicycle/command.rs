#[derive(Command)]
#[domain(id = "rent-bicycle", label = "Rent bicycle")]
pub struct RentBicycle;

macro_rules! make_stream_subject {
    () => {
        "rent-bicycle"
    };
}
