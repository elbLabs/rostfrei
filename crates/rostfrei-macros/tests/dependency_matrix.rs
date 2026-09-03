use std::path::PathBuf;
use std::process::Command;

const FIXTURES: &[&str] = &[
    "macro-path-renamed-facade",
    "macro-path-dev-poison",
    "macro-path-optional-poison",
];

#[test]
fn facade_bridge_is_stable_across_dependency_layouts() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("macro crate belongs to the Rostfrei workspace");
    let matrix_root = manifest_dir.join("tests/dependency-matrix");
    let manifest = matrix_root.join("Cargo.toml");
    let target = workspace_root.join("target/macro-dependency-matrix");

    for fixture in FIXTURES {
        let output = Command::new(env!("CARGO"))
            .args([
                "check",
                "--quiet",
                "--offline",
                "--manifest-path",
                manifest.to_str().expect("UTF-8 fixture manifest path"),
                "--package",
                fixture,
            ])
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .expect("run Cargo for dependency-matrix fixture");

        assert!(
            output.status.success(),
            "fixture `{fixture}` failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
