use domain::DomainService;

struct PlainContext;

#[derive(DomainService)]
#[domain(id = "missing-definition", label = "Missing definition")]
struct MissingDefinition;

#[derive(DomainService)]
#[domain(id = "mail-transfer", label = "Mail transfer")]
struct MailTransfer;

impl domain::DomainServiceDefinition for MailTransfer {
    type Context = PlainContext;
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
