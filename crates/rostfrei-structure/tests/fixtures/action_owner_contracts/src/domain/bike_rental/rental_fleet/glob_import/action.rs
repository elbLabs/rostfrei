#[domain_action(id = "glob-import", label = "Glob import")]
pub trait GlobImportAction {
    fn execute(&mut self);
}
