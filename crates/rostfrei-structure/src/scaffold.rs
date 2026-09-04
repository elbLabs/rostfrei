use std::{
    fs, io,
    path::{Path, PathBuf},
};

use heck::{ToKebabCase, ToSnakeCase, ToTitleCase, ToUpperCamelCase};
use tempfile::Builder;
use thiserror::Error;

const ROSTFREI_VERSION: &str = env!("CARGO_PKG_VERSION");
const RUST_VERSION: &str = env!("CARGO_PKG_RUST_VERSION");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatedProject {
    pub destination: PathBuf,
    pub package_name: String,
}

#[derive(Debug, Error)]
pub enum NewProjectError {
    #[error("project path `{0}` has no usable directory name")]
    InvalidPath(PathBuf),
    #[error(
        "project name `{0}` must begin with a lowercase ASCII letter, end with a lowercase ASCII letter or digit, and contain only lowercase ASCII letters, digits, `-`, or `_`"
    )]
    InvalidName(String),
    #[error("project name `{0}` produces a reserved Rust module name")]
    ReservedName(String),
    #[error("destination `{0}` already exists")]
    DestinationExists(PathBuf),
    #[error("could not inspect destination `{path}`: {source}")]
    InspectDestination {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not create parent directory `{path}`: {source}")]
    CreateParent {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not create temporary project directory in `{path}`: {source}")]
    CreateTemporaryDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not create generated directory `{path}`: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write generated file `{path}`: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not move the generated project to `{path}`: {source}")]
    Commit {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug)]
struct ProjectNames {
    package: String,
    module: String,
    context_type: String,
    context_id: String,
    context_label: String,
    domain_modules: String,
    domain_exports: String,
}

struct Template {
    path: &'static str,
    contents: &'static str,
}

const TEMPLATES: &[Template] = &[
    Template {
        path: ".gitignore",
        contents: include_str!("../templates/new/gitignore"),
    },
    Template {
        path: "Cargo.toml",
        contents: include_str!("../templates/new/Cargo.toml"),
    },
    Template {
        path: "README.md",
        contents: include_str!("../templates/new/README.md"),
    },
    Template {
        path: "clippy.toml",
        contents: include_str!("../templates/new/clippy.toml"),
    },
    Template {
        path: "compose.yaml",
        contents: include_str!("../templates/new/compose.yaml"),
    },
    Template {
        path: "nats-server.conf",
        contents: include_str!("../templates/new/nats-server.conf"),
    },
    Template {
        path: "rust-toolchain.toml",
        contents: include_str!("../templates/new/rust-toolchain.toml"),
    },
    Template {
        path: "src/main.rs",
        contents: include_str!("../templates/new/src/main.rs"),
    },
    Template {
        path: "src/lib.rs",
        contents: include_str!("../templates/new/src/lib.rs"),
    },
    Template {
        path: "src/bin/rostfrei-domain-check.rs",
        contents: include_str!("../templates/new/src/bin/rostfrei-domain-check.rs"),
    },
    Template {
        path: "src/domain/mod.rs",
        contents: include_str!("../templates/new/src/domain/mod.rs"),
    },
    Template {
        path: "src/domain/model.rs",
        contents: include_str!("../templates/new/src/domain/model.rs"),
    },
    Template {
        path: "src/domain/{{module_name}}/mod.rs",
        contents: include_str!("../templates/new/src/domain/context/mod.rs"),
    },
    Template {
        path: "src/domain/{{module_name}}/context.rs",
        contents: include_str!("../templates/new/src/domain/context/context.rs"),
    },
    Template {
        path: "src/domain/tests/mod.rs",
        contents: include_str!("../templates/new/src/domain/tests/mod.rs"),
    },
    Template {
        path: "src/domain/tests/model.rs",
        contents: include_str!("../templates/new/src/domain/tests/model.rs"),
    },
];

pub fn create_project(destination: impl AsRef<Path>) -> Result<CreatedProject, NewProjectError> {
    let destination = destination.as_ref();
    ensure_destination_is_available(destination)?;
    let names = project_names(destination)?;
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| NewProjectError::CreateParent {
        path: parent.to_path_buf(),
        source,
    })?;
    let temporary = Builder::new()
        .prefix(".rostfrei-new-")
        .tempdir_in(parent)
        .map_err(|source| NewProjectError::CreateTemporaryDirectory {
            path: parent.to_path_buf(),
            source,
        })?;

    render_project(temporary.path(), &names)?;
    fs::rename(temporary.path(), destination).map_err(|source| NewProjectError::Commit {
        path: destination.to_path_buf(),
        source,
    })?;

    Ok(CreatedProject {
        destination: destination.to_path_buf(),
        package_name: names.package,
    })
}

fn ensure_destination_is_available(destination: &Path) -> Result<(), NewProjectError> {
    match fs::symlink_metadata(destination) {
        Ok(_) => Err(NewProjectError::DestinationExists(
            destination.to_path_buf(),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(NewProjectError::InspectDestination {
            path: destination.to_path_buf(),
            source,
        }),
    }
}

fn project_names(destination: &Path) -> Result<ProjectNames, NewProjectError> {
    let Some(name) = destination.file_name().and_then(|name| name.to_str()) else {
        return Err(NewProjectError::InvalidPath(destination.to_path_buf()));
    };
    if !name.starts_with(|character: char| character.is_ascii_lowercase())
        || !name.ends_with(|character: char| {
            character.is_ascii_lowercase() || character.is_ascii_digit()
        })
        || !name.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
    {
        return Err(NewProjectError::InvalidName(name.to_owned()));
    }

    let module = name.to_snake_case();
    if is_reserved_rust_identifier(&module) {
        return Err(NewProjectError::ReservedName(name.to_owned()));
    }

    let context_type = name.to_upper_camel_case();
    let (domain_modules, domain_exports) = if module.as_str() < "model" {
        (
            format!("mod {module};\nmod model;"),
            format!("pub use {module}::{context_type};\npub use model::domain_model;"),
        )
    } else {
        (
            format!("mod model;\nmod {module};"),
            format!("pub use model::domain_model;\npub use {module}::{context_type};"),
        )
    };

    Ok(ProjectNames {
        package: name.to_owned(),
        module,
        context_type,
        context_id: name.to_kebab_case(),
        context_label: name.to_title_case(),
        domain_modules,
        domain_exports,
    })
}

fn is_reserved_rust_identifier(identifier: &str) -> bool {
    matches!(
        identifier,
        "abstract"
            | "as"
            | "async"
            | "await"
            | "become"
            | "box"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "do"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
    )
}

fn render_project(root: &Path, names: &ProjectNames) -> Result<(), NewProjectError> {
    for template in TEMPLATES {
        let relative_path = render(template.path, names);
        let path = root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| NewProjectError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&path, render(template.contents, names)).map_err(|source| {
            NewProjectError::WriteFile {
                path: path.clone(),
                source,
            }
        })?;
    }
    Ok(())
}

fn render(template: &str, names: &ProjectNames) -> String {
    template
        .replace("{{package_name}}", &names.package)
        .replace("{{module_name}}", &names.module)
        .replace("{{context_type}}", &names.context_type)
        .replace("{{context_id}}", &names.context_id)
        .replace("{{context_label}}", &names.context_label)
        .replace("{{domain_modules}}", &names.domain_modules)
        .replace("{{domain_exports}}", &names.domain_exports)
        .replace("{{rostfrei_version}}", ROSTFREI_VERSION)
        .replace("{{rust_version}}", RUST_VERSION)
}

#[cfg(test)]
mod tests {
    use super::{ProjectNames, is_reserved_rust_identifier, project_names, render};
    use std::path::Path;

    #[test]
    fn derives_all_names_from_the_destination_directory() {
        let names = project_names(Path::new("somewhere/bike-rental"));
        assert!(names.is_ok(), "name derivation failed: {names:?}");
        let Some(names) = names.ok() else {
            return;
        };

        assert_eq!(names.package, "bike-rental");
        assert_eq!(names.module, "bike_rental");
        assert_eq!(names.context_type, "BikeRental");
        assert_eq!(names.context_id, "bike-rental");
        assert_eq!(names.context_label, "Bike Rental");
    }

    #[test]
    fn generated_manifest_lints_match_workspace_lints() {
        let root_manifest: toml::Value = toml::from_str(include_str!("../../../Cargo.toml"))
            .unwrap_or_else(|error| panic!("workspace manifest must parse: {error}"));
        let names = ProjectNames {
            package: "sample".to_owned(),
            module: "sample".to_owned(),
            context_type: "Sample".to_owned(),
            context_id: "sample".to_owned(),
            context_label: "Sample".to_owned(),
            domain_modules: "mod model;\nmod sample;".to_owned(),
            domain_exports: "pub use model::domain_model;\npub use sample::Sample;".to_owned(),
        };
        let generated_manifest: toml::Value =
            toml::from_str(&render(include_str!("../templates/new/Cargo.toml"), &names))
                .unwrap_or_else(|error| panic!("generated manifest must parse: {error}"));

        assert_eq!(
            generated_manifest
                .get("lints")
                .and_then(|lints| lints.get("rust")),
            root_manifest
                .get("workspace")
                .and_then(|workspace| workspace.get("lints"))
                .and_then(|lints| lints.get("rust"))
        );
        assert_eq!(
            generated_manifest
                .get("lints")
                .and_then(|lints| lints.get("clippy")),
            root_manifest
                .get("workspace")
                .and_then(|workspace| workspace.get("lints"))
                .and_then(|lints| lints.get("clippy"))
        );
    }

    #[test]
    fn copied_tool_configuration_stays_in_sync() {
        assert_eq!(
            include_str!("../templates/new/clippy.toml"),
            include_str!("../../../clippy.toml")
        );
        assert_eq!(
            include_str!("../templates/new/rust-toolchain.toml"),
            include_str!("../../../rust-toolchain.toml")
        );
    }

    #[test]
    fn rejects_names_that_cannot_be_generated_as_plain_rust_identifiers() {
        for name in ["9lives", "has spaces", "BikeRental", "trailing-", "type"] {
            assert!(project_names(Path::new(name)).is_err(), "accepted `{name}`");
        }
        assert!(is_reserved_rust_identifier("type"));
    }
}
