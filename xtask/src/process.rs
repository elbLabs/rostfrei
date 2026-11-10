//! Owned process groups, bounded output drains, cancellation and credential redaction.
use crate::Environment;
use anyhow::{Context, Result, bail, ensure};
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicI32, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

static SIGNAL: AtomicI32 = AtomicI32::new(0);
static SIGNALS: OnceLock<()> = OnceLock::new();

pub fn install_signals() -> Result<()> {
    if SIGNALS.get().is_some() {
        return Ok(());
    }
    let mut signals = signal_hook::iterator::Signals::new([
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
    ])?;
    thread::spawn(move || {
        for signal in signals.forever() {
            SIGNAL.store(signal, Ordering::SeqCst);
        }
    });
    let _ = SIGNALS.set(());
    Ok(())
}
pub fn cancelled() -> Result<()> {
    let signal = SIGNAL.load(Ordering::SeqCst);
    ensure!(signal == 0, "cancelled by signal {signal}");
    Ok(())
}
pub fn signal_code() -> Option<i32> {
    let signal = SIGNAL.load(Ordering::SeqCst);
    (signal != 0).then_some(128 + signal)
}
pub fn code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
}
pub struct OwnedChild {
    pub child: Child,
    pid: Pid,
}
impl OwnedChild {
    pub fn spawn(command: &mut Command) -> Result<Self> {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        let child = command
            .spawn()
            .with_context(|| format!("start {:?}", command.get_program()))?;
        let pid = Pid::from_raw(i32::try_from(child.id())?);
        Ok(Self { child, pid })
    }
    pub fn signal(&self, signal: Signal) {
        let _ = killpg(self.pid, signal);
    }
    pub fn stop(&mut self, timeout: Duration) -> Result<i32> {
        self.signal(Signal::SIGINT);
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.signal(Signal::SIGKILL);
                return Ok(code(status));
            }
            if Instant::now() >= deadline {
                bail!("child did not finish supervised shutdown");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            // Nested xtasks also supervise their own process groups. Give them
            // time to run cleanup before forcibly terminating this group.
            self.signal(Signal::SIGTERM);
            let deadline = Instant::now() + Duration::from_secs(10);
            while killpg(self.pid, None).is_ok() && Instant::now() < deadline {
                let _ = self.child.try_wait();
                thread::sleep(Duration::from_millis(20));
            }
        }
        self.signal(Signal::SIGKILL);
        let _ = self.child.wait();
    }
}
#[derive(Debug)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
impl Output {
    pub fn checked(self) -> Result<Self> {
        ensure!(
            self.code == 0,
            "command exited {}\n{}{}",
            self.code,
            self.stdout,
            self.stderr
        );
        Ok(self)
    }
}
pub fn command(args: &[String], cwd: &Path, env: &Environment) -> Result<Command> {
    let (program, arguments) = args.split_first().context("empty command")?;
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(cwd)
        .env_clear()
        .envs(env);
    Ok(command)
}
pub fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}
pub fn run(values: &[&str], cwd: &Path, env: &Environment) -> Result<String> {
    Ok(capture(
        &args(values),
        cwd,
        env,
        Duration::from_secs(1200),
        &[],
        false,
    )?
    .checked()?
    .stdout)
}
pub fn run_owned(values: &[String], cwd: &Path, env: &Environment) -> Result<String> {
    Ok(
        capture(values, cwd, env, Duration::from_secs(1200), &[], false)?
            .checked()?
            .stdout,
    )
}
pub fn redact(mut text: String, secrets: &[String]) -> String {
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        text = text.replace(secret, "[redacted]");
    }
    text
}
pub fn capture(
    values: &[String],
    cwd: &Path,
    env: &Environment,
    timeout: Duration,
    secrets: &[String],
    stream: bool,
) -> Result<Output> {
    capture_logged(values, cwd, env, timeout, secrets, stream, None)
}

pub fn capture_logged(
    values: &[String],
    cwd: &Path,
    env: &Environment,
    timeout: Duration,
    secrets: &[String],
    stream: bool,
    log: Option<&Path>,
) -> Result<Output> {
    let mut log = log.map(File::create).transpose()?;
    cancelled()?;
    let mut command = command(values, cwd, env)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut process = OwnedChild::spawn(&mut command)?;
    let (send, receive) = mpsc::channel();
    let readers: Vec<Box<dyn Read + Send>> = vec![
        Box::new(process.child.stdout.take().context("stdout")?),
        Box::new(process.child.stderr.take().context("stderr")?),
    ];
    for (index, mut reader) in readers.into_iter().enumerate() {
        let send = send.clone();
        thread::spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if send.send((index, buffer[..count].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
    drop(send);
    let started = Instant::now();
    let mut ended = None;
    let mut status = None;
    let mut pending = [Vec::new(), Vec::new()];
    let mut output = [String::new(), String::new()];
    loop {
        cancelled()?;
        ensure!(
            started.elapsed() < timeout,
            "subprocess exceeded {timeout:?}"
        );
        if status.is_none()
            && let Some(exit) = process.child.try_wait()?
        {
            status = Some(code(exit));
            process.signal(Signal::SIGKILL);
            ended = Some(Instant::now());
        }
        match receive.recv_timeout(Duration::from_millis(20)) {
            Ok((index, bytes)) => {
                // Scan only newly read bytes. Rescanning an unterminated JSON
                // document on every read is quadratic and can exhaust the final
                // drain budget while complete output is still queued.
                for fragment in bytes.split_inclusive(|byte| *byte == b'\n') {
                    pending[index].extend_from_slice(fragment);
                    if fragment.last() != Some(&b'\n') {
                        continue;
                    }
                    let bytes = std::mem::take(&mut pending[index]);
                    let line = redact(String::from_utf8_lossy(&bytes).into_owned(), secrets);
                    if let Some(log) = &mut log {
                        log.write_all(line.as_bytes())?;
                        log.flush()?;
                    }
                    if stream {
                        print!("{line}");
                        std::io::stdout().flush()?;
                    }
                    output[index].push_str(&line);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) if status.is_some() => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => thread::sleep(Duration::from_millis(20)),
            _ => {}
        }
        if ended.is_some_and(|time| time.elapsed() >= Duration::from_secs(1)) {
            break;
        }
    }
    for index in 0..2 {
        let line = redact(
            String::from_utf8_lossy(&pending[index]).into_owned(),
            secrets,
        );
        if let Some(log) = &mut log {
            log.write_all(line.as_bytes())?;
            log.flush()?;
        }
        if stream {
            print!("{line}");
            std::io::stdout().flush()?;
        }
        output[index].push_str(&line);
    }
    Ok(Output {
        code: status.context("child status")?,
        stdout: output[0].clone(),
        stderr: output[1].clone(),
    })
}
pub fn background(
    values: &[String],
    cwd: &Path,
    env: &Environment,
    log: &Path,
) -> Result<OwnedChild> {
    cancelled()?;
    let file = File::create(log)?;
    let mut command = command(values, cwd, env)?;
    command.stdout(file.try_clone()?).stderr(file);
    OwnedChild::spawn(&mut command)
}
pub fn executable(name: &str) -> Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let executable = |path: &Path| {
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let path = Path::new(name);
    if path.components().count() > 1 {
        ensure!(executable(path), "native executable missing: {name}");
        return Ok(std::path::absolute(path)?);
    }
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let path = directory.join(name);
        if executable(&path) {
            // Keep argv[0]'s basename for rustup and other multicall binaries.
            return Ok(std::path::absolute(path)?);
        }
    }
    bail!("native executable missing: {name}")
}
