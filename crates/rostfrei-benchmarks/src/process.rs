use std::{process::Output, time::Duration};

use tokio::process::Command;

use crate::BenchResult;

/// Bound subprocess waits and terminate the child when a run is cancelled.
pub async fn output(command: &mut Command, timeout: Duration) -> BenchResult<Output> {
    command.kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.as_std_mut().process_group(0);
    }
    let child = command.spawn()?;
    let mut group = ProcessGroup(child.id());
    let result = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| format!("subprocess exceeded {timeout:?}"))??;
    group.0 = None;
    Ok(result)
}

struct ProcessGroup(Option<u32>);

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            // Safe process-group termination without unsafe/libc. This group belongs only
            // to our child, never the shell/session that launched the benchmark runner.
            let _ = std::process::Command::new("kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .status();
        }
    }
}
