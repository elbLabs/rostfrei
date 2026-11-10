#[test]
fn ci_workflows_are_valid_and_use_native_tooling() {
    for entry in std::fs::read_dir(xtask::root().join(".github/workflows")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "yml" && e != "yaml") {
            continue;
        }
        let contents = std::fs::read_to_string(&path).unwrap();
        yaml_rust2::YamlLoader::load_from_str(&contents)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(
            !contents.contains("setup-python") && !contents.contains("python3 "),
            "{} requires the removed interpreter",
            path.display()
        );
    }
}
