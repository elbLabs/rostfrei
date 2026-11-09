use std::{
    fs,
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use xtask::{environment, process};

#[test]
#[ignore = "subprocess fixture, invoked explicitly by lifecycle regressions"]
fn child_fixture() {
    let Some(marker) = std::env::var_os("XTASK_FIXTURE_MARKER") else {
        return;
    };
    let descendant = Command::new("sleep").arg("300").spawn().unwrap();
    fs::write(
        marker,
        format!("{} {}", std::process::id(), descendant.id()),
    )
    .unwrap();
    for byte in "start fragmented-password-🔒\ntail fragmented-password-🔒".as_bytes() {
        std::io::stdout().write_all(&[*byte]).unwrap();
        std::io::stdout().flush().unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    std::process::exit(
        std::env::var("XTASK_FIXTURE_EXIT")
            .unwrap()
            .parse()
            .unwrap(),
    );
}
#[test]
fn inherited_pipes_are_bounded_descendants_cleaned_and_fragmented_secrets_redacted() {
    for code in [0, 7] {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("pids");
        let mut env = environment();
        env.insert("XTASK_FIXTURE_MARKER".into(), marker.display().to_string());
        env.insert("XTASK_FIXTURE_EXIT".into(), code.to_string());
        let command = vec![
            std::env::current_exe().unwrap().display().to_string(),
            "--ignored".into(),
            "--exact".into(),
            "child_fixture".into(),
            "--nocapture".into(),
        ];
        let started = Instant::now();
        let result = process::capture(
            &command,
            temp.path(),
            &env,
            Duration::from_secs(5),
            &["fragmented-password-🔒".into()],
            false,
        )
        .unwrap();
        assert_eq!(result.code, code);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!result.stdout.contains("fragmented-password"));
        assert_eq!(result.stdout.matches("[redacted]").count(), 2);
        for pid in fs::read_to_string(marker).unwrap().split_whitespace() {
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                let alive = fs::read_to_string(format!("/proc/{pid}/stat"))
                    .is_ok_and(|s| s.split_whitespace().nth(2) != Some("Z"));
                if !alive {
                    break;
                }
                assert!(Instant::now() < deadline, "child {pid} survived cleanup");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
#[test]
fn port_bindings_and_readiness_are_validated() {
    for value in [
        serde_json::json!({}),
        serde_json::json!({"port":[]}),
        serde_json::json!({"port":[{"HostIp":"0.0.0.0","HostPort":"1234"}]}),
        serde_json::json!({"port":[{"HostIp":"127.0.0.1","HostPort":"0"}]}),
        serde_json::json!({"port":[{"HostIp":"127.0.0.1","HostPort":"70000"}]}),
        serde_json::json!({"port":[{},{}]}),
    ] {
        assert!(xtask::nats::mapped_port(&value, "port").is_err());
    }
    assert_eq!(
        xtask::nats::mapped_port(
            &serde_json::json!({"port":[{"HostIp":"127.0.0.1","HostPort":"1234"}]}),
            "port"
        )
        .unwrap(),
        1234
    );
    assert!(xtask::nats::wait_ready(1, Duration::ZERO).is_err());
}

#[test]
fn fixture_overrides_inherited_credentials_preserves_exit_and_cleans_its_container() {
    use std::os::unix::fs::PermissionsExt;
    for (code, cleanup_fails) in [(0, false), (7, false), (0, true)] {
        let temp = tempfile::tempdir().unwrap();
        let docker = temp.path().join("docker");
        fs::write(&docker,r#"#!/bin/sh
echo "$*" >> "$FIXTURE/log"
case "$1" in
run) for arg in "$@"; do case "$arg" in type=bind,src=*) config=${arg#type=bind,src=}; config=${config%,dst=*}; cp "$config" "$FIXTURE/config"; stat -c %a "$config" > "$FIXTURE/mode";; esac; done;;
inspect) printf '{"4222/tcp":[{"HostIp":"127.0.0.1","HostPort":"14222"}],"8222/tcp":[{"HostIp":"127.0.0.1","HostPort":"%s"}]}' "$MONITOR";;
logs) cat "$FIXTURE/config" >&2;;
rm) if [ "$FAIL_CLEANUP" = 1 ]; then exit 9; fi;;
esac
"#).unwrap();
        fs::set_permissions(&docker, fs::Permissions::from_mode(0o755)).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let body = "{\"status\":\"ok\"}";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let child = temp.path().join("child");
        fs::write(&child,format!("#!/bin/sh\nprintf '%s\\n' \"$ROSTFREI_NATS_URL\" \"$ROSTFREI_CONTRACT_NATS_PASSWORD\"\nprintf '%s' \"$ROSTFREI_CONTRACT_EVIDENCE_DIR\" > \"$FIXTURE/evidence\"\nexit {code}\n")).unwrap();
        fs::set_permissions(&child, fs::Permissions::from_mode(0o755)).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["test", "--contracts", "--", child.to_str().unwrap()])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    temp.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("FIXTURE", temp.path())
            .env("FAIL_CLEANUP", if cleanup_fails { "1" } else { "0" })
            .env("MONITOR", port.to_string())
            .env("ROSTFREI_NATS_SERVER", std::env::current_exe().unwrap())
            .env("ROSTFREI_NATS_URL", "nats://do-not-use:4222")
            .env("ROSTFREI_CONTRACT_NATS_PASSWORD", "inherited-password")
            .env("ROSTFREI_CONTRACT_EVIDENCE_DIR", "/do-not-use")
            .output()
            .unwrap();
        server.join().unwrap();
        assert_eq!(
            result.status.code(),
            Some(if cleanup_fails { 1 } else { code })
        );
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            output.contains("nats://contract-tests:[redacted]@127.0.0.1:14222"),
            "{output}"
        );
        let config = fs::read_to_string(temp.path().join("config")).unwrap();
        let password = config
            .split("password: \"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(!output.contains(password));
        assert_ne!(password, "inherited-password");
        assert_eq!(
            fs::read_to_string(temp.path().join("mode")).unwrap().trim(),
            "600"
        );
        assert!(
            !std::path::Path::new(&fs::read_to_string(temp.path().join("evidence")).unwrap())
                .exists()
        );
        let log = fs::read_to_string(temp.path().join("log")).unwrap();
        let started = log.lines().find(|l| l.starts_with("run ")).unwrap();
        let name = started
            .split("--name ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert_eq!(
            log.lines().last().unwrap(),
            format!("rm --force --volumes {name}")
        );
        assert_eq!(log.lines().any(|l| l.starts_with("logs ")), code != 0);
    }
}

#[test]
fn missing_native_binary_and_invalid_timeouts_fail_before_docker() {
    for args in [
        vec!["test", "--contracts", "--", "true"],
        vec!["test", "--ready-timeout", "NaN"],
        vec!["test", "--ready-timeout", "0"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(args)
            .env("ROSTFREI_NATS_SERVER", "/missing/native-nats")
            .stdout(Stdio::piped())
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}

#[test]
fn readiness_failure_and_cancellation_remove_only_the_owned_broker() {
    use std::os::unix::fs::PermissionsExt;
    for cancel in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let docker = temp.path().join("docker");
        fs::write(&docker,r#"#!/bin/sh
echo "$*" >> "$FIXTURE/log"
case "$1" in
run) touch "$FIXTURE/started";;
inspect) printf '{"4222/tcp":[{"HostIp":"127.0.0.1","HostPort":"14222"}],"8222/tcp":[{"HostIp":"127.0.0.1","HostPort":"1"}]}' ;;
esac
"#).unwrap();
        fs::set_permissions(&docker, fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args([
                "test",
                "--ready-timeout",
                if cancel { "30" } else { "0.01" },
                "--",
                "sh",
                "-c",
                "exit 99",
            ])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    temp.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("FIXTURE", temp.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if cancel {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !temp.path().join("started").exists() {
                assert!(Instant::now() < deadline);
                thread_sleep();
            }
            nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(i32::try_from(child.id()).unwrap()),
                nix::sys::signal::Signal::SIGTERM,
            )
            .unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(if cancel { 143 } else { 1 }));
                break;
            }
            assert!(Instant::now() < deadline);
            thread_sleep();
        }
        let log = fs::read_to_string(temp.path().join("log")).unwrap();
        let started = log.lines().find(|l| l.starts_with("run ")).unwrap();
        let name = started
            .split("--name ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert_eq!(
            log.lines().last().unwrap(),
            format!("rm --force --volumes {name}")
        );
    }
}
fn thread_sleep() {
    std::thread::sleep(Duration::from_millis(10));
}

#[test]
fn default_prefetch_failure_preserves_status_and_never_starts_a_broker() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    for program in ["cargo", "docker"] {
        let script = temp.path().join(program);
        fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s\\n' \"{program} $*\" >> \"$FIXTURE/log\"\nexit 7\n"),
        )
        .unwrap();
        fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("test")
        .env("FIXTURE", temp.path())
        .env("PATH", temp.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("log")).unwrap(),
        format!(
            "cargo fetch --locked --manifest-path {}\n",
            xtask::root().join(xtask::versions::FIXTURE).display()
        )
    );
}

#[test]
fn timed_out_commands_retain_redacted_diagnostics() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("command.log");
    let result = process::capture_logged(
        &process::args(&[
            "sh",
            "-c",
            "printf 'evidence fixture-password\\n'; sleep 30",
        ]),
        temp.path(),
        &environment(),
        Duration::from_millis(500),
        &["fixture-password".into()],
        false,
        Some(&log),
    );
    assert!(result.unwrap_err().to_string().contains("exceeded"));
    assert_eq!(fs::read_to_string(log).unwrap(), "evidence [redacted]\n");
}

#[test]
#[ignore = "subprocess fixture for large machine-readable output"]
fn large_output_fixture() {
    if std::env::var_os("XTASK_LARGE_OUTPUT").is_none() {
        return;
    }
    std::io::stdout()
        .write_all("x".repeat(8 * 1024 * 1024).as_bytes())
        .unwrap();
    std::io::stdout().flush().unwrap();
    std::process::exit(0);
}

#[test]
fn large_unterminated_output_is_not_truncated_after_child_exit() {
    let temp = tempfile::tempdir().unwrap();
    let mut env = environment();
    env.insert("XTASK_LARGE_OUTPUT".into(), "1".into());
    let output = process::capture(
        &[
            std::env::current_exe().unwrap().display().to_string(),
            "--ignored".into(),
            "--exact".into(),
            "large_output_fixture".into(),
            "--nocapture".into(),
        ],
        temp.path(),
        &env,
        Duration::from_secs(15),
        &[],
        false,
    )
    .unwrap();
    assert_eq!(output.code, 0);
    assert!(
        output.stdout.ends_with(&"x".repeat(8 * 1024 * 1024)),
        "captured only {} bytes",
        output.stdout.len()
    );
}

#[test]
fn executable_lookup_preserves_multicall_symlink_names() {
    let temp = tempfile::tempdir().unwrap();
    let cargo = temp.path().join("cargo");
    std::os::unix::fs::symlink(process::executable("cargo").unwrap(), &cargo).unwrap();
    let found = process::executable(cargo.to_str().unwrap()).unwrap();
    assert_eq!(found, cargo);
    let output = Command::new(found).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("cargo "));
}

#[test]
#[ignore = "subprocess supervisor fixture"]
fn nested_supervisor_fixture() {
    let Some(marker) = std::env::var_os("XTASK_NESTED_MARKER") else {
        return;
    };
    process::install_signals().unwrap();
    let mut child = Command::new("sleep");
    child.arg("300");
    let child = process::OwnedChild::spawn(&mut child).unwrap();
    fs::write(marker, child.child.id().to_string()).unwrap();
    while process::cancelled().is_ok() {
        thread_sleep();
    }
    drop(child);
}

#[test]
fn timeout_allows_nested_supervisors_to_clean_their_own_process_groups() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("child-pid");
    let mut env = environment();
    env.insert("XTASK_NESTED_MARKER".into(), marker.display().to_string());
    let result = process::capture(
        &[
            std::env::current_exe().unwrap().display().to_string(),
            "--ignored".into(),
            "--exact".into(),
            "nested_supervisor_fixture".into(),
            "--nocapture".into(),
        ],
        temp.path(),
        &env,
        Duration::from_secs(1),
        &[],
        false,
    );
    assert!(result.unwrap_err().to_string().contains("exceeded"));
    let pid = fs::read_to_string(marker).unwrap();
    assert!(
        !fs::read_to_string(format!("/proc/{pid}/stat"))
            .is_ok_and(|s| s.split_whitespace().nth(2) != Some("Z")),
        "nested child {pid} survived timeout"
    );
}
