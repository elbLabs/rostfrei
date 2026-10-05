#[path = "../read_model_support.rs"]
mod support;
rostfrei::install_macro_support!();

fn mutate(reader: &rostfrei::ReadModelReader<support::View>) {
    reader.delete("org-1");
}
fn main() {}
