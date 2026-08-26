use rostfrei_domain::DomainService;

struct PlainContext;

#[derive(DomainService)]
#[domain(id = "mail-transfer", label = "Mail transfer", context = PlainContext)]
struct MailTransfer;

fn main() {}
