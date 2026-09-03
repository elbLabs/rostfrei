mod code;

use std::fmt;
use std::path::{Path, PathBuf};

pub use code::DiagnosticCode;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub path: PathBuf,
    pub line: usize,
    pub message: String,
    pub help: Option<String>,
}

impl Diagnostic {
    pub(crate) fn new(
        code: DiagnosticCode,
        path: impl Into<PathBuf>,
        line: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            path: path.into(),
            line,
            message: message.into(),
            help: None,
        }
    }

    pub(crate) fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn render(&self, package_root: &Path) -> String {
        let path = self.path.strip_prefix(package_root).unwrap_or(&self.path);
        let mut rendered = format!(
            "{}: {}\n  --> {}:{}",
            self.code,
            self.message,
            path.display(),
            self.line
        );
        if let Some(help) = &self.help {
            rendered.push_str("\n  help: ");
            rendered.push_str(&help.replace('\n', "\n        "));
        }
        rendered
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {} at {}:{}",
            self.code,
            self.message,
            self.path.display(),
            self.line
        )
    }
}
