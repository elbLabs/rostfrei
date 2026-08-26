use std::{
    path::{Path, PathBuf},
    process::Command,
};

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompilerStatus {
    available: bool,
    version: String,
}

#[tauri::command]
fn compiler_status() -> Result<CompilerStatus, String> {
    cargo_status()
}

fn cargo_status() -> Result<CompilerStatus, String> {
    let output = Command::new("cargo")
        .arg("--version")
        .output()
        .map_err(|error| format!("failed to start cargo: {error}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    Ok(CompilerStatus {
        available: true,
        version: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    })
}

#[derive(Debug, PartialEq, Serialize)]
struct CheckResult {
    success: bool,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, PartialEq, Serialize)]
struct Diagnostic {
    level: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rendered: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
}

#[tauri::command]
fn load_domain_model(workspace_path: String, package: Option<String>) -> Result<Value, String> {
    let workspace = validate_workspace(&workspace_path)?;
    let mut command = Command::new("cargo");
    command.args(["run", "--quiet"]);

    if let Some(package) = package {
        command.args(["-p", &package]);
    }

    let output = command.current_dir(&workspace).output().map_err(|error| {
        format!(
            "failed to start cargo in '{}': {error}",
            workspace.display()
        )
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        let detail = if stderr.is_empty() {
            "no stderr output"
        } else {
            stderr
        };

        return Err(format!(
            "cargo run failed in '{}' with {}: {detail}",
            workspace.display(),
            output.status
        ));
    }

    serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "cargo run in '{}' produced invalid JSON: {error}",
            workspace.display()
        )
    })
}

#[tauri::command]
fn check_workspace(workspace_path: String) -> Result<CheckResult, String> {
    let workspace = validate_workspace(&workspace_path)?;
    let output = Command::new("cargo")
        .args(["check", "--message-format=json"])
        .current_dir(&workspace)
        .output()
        .map_err(|error| {
            format!(
                "failed to start cargo in '{}': {error}",
                workspace.display()
            )
        })?;

    Ok(CheckResult {
        success: output.status.success(),
        diagnostics: parse_cargo_diagnostics(&String::from_utf8_lossy(&output.stdout)),
    })
}

fn validate_workspace(workspace_path: &str) -> Result<PathBuf, String> {
    let workspace = Path::new(workspace_path)
        .canonicalize()
        .map_err(|error| format!("invalid workspace path '{workspace_path}': {error}"))?;

    if !workspace.is_dir() {
        return Err(format!(
            "invalid workspace path '{}': not a directory",
            workspace.display()
        ));
    }

    let manifest = workspace.join("Cargo.toml");
    if !manifest.is_file() {
        return Err(format!(
            "invalid workspace path '{}': Cargo.toml was not found",
            workspace.display()
        ));
    }

    Ok(workspace)
}

fn parse_cargo_diagnostics(output: &str) -> Vec<Diagnostic> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry.get("reason").and_then(Value::as_str) == Some("compiler-message"))
        .filter_map(|entry| {
            let message = entry.get("message")?;
            let primary_span = message
                .get("spans")
                .and_then(Value::as_array)
                .and_then(|spans| {
                    spans
                        .iter()
                        .find(|span| span.get("is_primary").and_then(Value::as_bool) == Some(true))
                });

            Some(Diagnostic {
                level: message.get("level")?.as_str()?.to_owned(),
                message: message.get("message")?.as_str()?.to_owned(),
                rendered: message
                    .get("rendered")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                file: primary_span
                    .and_then(|span| span.get("file_name"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                line: primary_span
                    .and_then(|span| span.get("line_start"))
                    .and_then(Value::as_u64),
            })
        })
        .collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            compiler_status,
            load_domain_model,
            check_workspace
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::{cargo_status, parse_cargo_diagnostics, validate_workspace, Diagnostic};

    #[test]
    fn detects_the_rust_compiler_toolchain() {
        let status = cargo_status().expect("cargo should be installed for rostfrei Studio");

        assert!(status.available);
        assert!(status.version.starts_with("cargo "));
    }

    #[test]
    fn rejects_invalid_workspaces() {
        let missing = std::env::temp_dir().join(format!(
            "rostfrei-studio-missing-{}-{:?}",
            std::process::id(),
            SystemTime::now()
        ));
        assert!(validate_workspace(missing.to_str().unwrap()).is_err());

        let without_manifest = std::env::temp_dir().join(format!(
            "rostfrei-studio-no-manifest-{}-{:?}",
            std::process::id(),
            SystemTime::now()
        ));
        fs::create_dir(&without_manifest).unwrap();

        let error = validate_workspace(without_manifest.to_str().unwrap()).unwrap_err();
        assert!(error.contains("Cargo.toml was not found"));

        fs::remove_dir(without_manifest).unwrap();
    }

    #[test]
    fn parses_compiler_messages_and_ignores_other_lines() {
        let output = r#"
not JSON
{"reason":"build-script-executed","package_id":"example"}
{"reason":"compiler-message","message":{"level":"warning","message":"unused variable","rendered":"warning: unused variable\n","spans":[{"file_name":"src/lib.rs","line_start":12,"is_primary":false},{"file_name":"src/main.rs","line_start":7,"is_primary":true}]}}
{"reason":"compiler-message","message":{"level":"error","message":"type mismatch","rendered":null,"spans":[]}}
"#;

        assert_eq!(
            parse_cargo_diagnostics(output),
            vec![
                Diagnostic {
                    level: "warning".to_owned(),
                    message: "unused variable".to_owned(),
                    rendered: Some("warning: unused variable\n".to_owned()),
                    file: Some("src/main.rs".to_owned()),
                    line: Some(7),
                },
                Diagnostic {
                    level: "error".to_owned(),
                    message: "type mismatch".to_owned(),
                    rendered: None,
                    file: None,
                    line: None,
                },
            ]
        );
    }
}
