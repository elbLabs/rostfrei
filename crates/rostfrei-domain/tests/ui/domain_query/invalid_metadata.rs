use domain::domain_query;

#[domain_query(label = "Missing ID")]
trait MissingId {
    fn execute();
}

fn main() {}
