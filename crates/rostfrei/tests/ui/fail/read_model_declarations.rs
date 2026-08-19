rostfrei::install_macro_support!();

#[derive(rostfrei::ReadModel)]
#[read_model(id = "not-a-struct", version = 1)]
enum EnumModel { Value }

#[derive(rostfrei::ReadModel)]
#[read_model(id = "bad.name", version = 1)]
struct InvalidName;

#[derive(rostfrei::ReadModel)]
#[read_model(id = "bad-version", version = 0)]
struct InvalidVersion;

fn main() {}
