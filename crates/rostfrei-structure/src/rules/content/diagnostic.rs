use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::{SourceFileFacts, TopLevelItem};

pub(super) fn unexpected_item(file: &SourceFileFacts, item: &TopLevelItem) -> Diagnostic {
    let file_name = file
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("role file");
    let item_name = item
        .name
        .as_deref()
        .map(|name| format!(" `{name}`"))
        .unwrap_or_default();
    Diagnostic::new(
        DiagnosticCode::UnexpectedRoleContent,
        &file.path,
        item.line,
        format!("{}{item_name} is not allowed in `{file_name}`", item.label),
    )
    .with_help("move unrelated content to the domain object file responsible for it")
}
