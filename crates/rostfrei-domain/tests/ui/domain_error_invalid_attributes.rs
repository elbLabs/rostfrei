use domain::DomainError;

struct Owner;

#[derive(DomainError)]
struct MissingDomain;

#[derive(DomainError)]
#[domain(label = "Missing id", owner = Owner, code = "MISSING_ID", message = "Missing id.")]
struct MissingId;

#[derive(DomainError)]
#[domain(id = "missing-code", label = "Missing code", owner = Owner, message = "Missing code.")]
struct MissingCode;

#[derive(DomainError)]
#[domain(id = "missing-message", label = "Missing message", owner = Owner, code = "MISSING_MESSAGE")]
struct MissingMessage;

#[derive(DomainError)]
#[domain(id = "duplicate", label = "Duplicate", owner = Owner, code = "DUPLICATE", code = "OTHER", message = "Duplicate.")]
struct Duplicate;

#[derive(DomainError)]
#[domain(id = "duplicate-message", label = "Duplicate message", owner = Owner, code = "DUPLICATE_MESSAGE", message = "Duplicate.", message = "Other.")]
struct DuplicateMessage;

#[derive(DomainError)]
#[domain(id = "unsupported", label = "Unsupported", owner = Owner, code = "UNSUPPORTED", message = "Unsupported.", schema = 1)]
struct Unsupported;

#[derive(DomainError)]
#[domain(id = "bad-code", label = "Bad code", owner = Owner, code = "1_BAD-code", message = "Bad code.")]
struct BadCode;

#[derive(DomainError)]
#[domain(id = "blank-message", label = "Blank message", owner = Owner, code = "BLANK_MESSAGE", message = " ")]
struct BlankMessage;

fn main() {}
