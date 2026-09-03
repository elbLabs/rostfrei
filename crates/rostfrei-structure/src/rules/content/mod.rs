mod diagnostic;
mod policy;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diagnostic::Diagnostic;
use crate::source::SourceFileFacts;

use self::diagnostic::unexpected_item;
use self::policy::RolePolicy;

pub(super) fn check(
    root: &Path,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tests_root = root.join("tests");
    for file in facts
        .values()
        .filter(|file| !file.path.starts_with(&tests_root))
    {
        let Some(policy) = RolePolicy::for_file(file) else {
            continue;
        };
        for item in &file.top_level_items {
            if !policy.allows(item, file) {
                diagnostics.push(unexpected_item(file, item));
            }
        }
    }
}
