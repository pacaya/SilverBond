use std::{
    fs::File,
    os::unix::{fs::FileExt, process::CommandExt},
    process::{Child, Command, ExitStatus, Output, Stdio},
    time::{Duration, Instant},
};

use anyhow::Context;

const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub(crate) struct TimedOutOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl std::fmt::Display for TimedOutOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("command timed out with captured output")
    }
}

impl std::error::Error for TimedOutOutput {}

pub(crate) fn timeout_captured_output(error: &anyhow::Error) -> Option<&TimedOutOutput> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<TimedOutOutput>())
}

enum WaitResult {
    Exited(ExitStatus),
    TimedOut,
}

pub(crate) fn command_status_with_timeout(
    mut command: Command,
    timeout: Duration,
    operation: &str,
) -> anyhow::Result<ExitStatus> {
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn command while {operation}"))?;
    match wait_for_child(&mut child, timeout, operation)? {
        WaitResult::Exited(status) => Ok(status),
        WaitResult::TimedOut => {
            anyhow::bail!("timed out while {operation} after {timeout:?}");
        }
    }
}

pub(crate) fn command_output_with_timeout(
    mut command: Command,
    timeout: Duration,
    operation: &str,
) -> anyhow::Result<Output> {
    let stdout = tempfile::tempfile().context("failed to create stdout capture file")?;
    let stderr = tempfile::tempfile().context("failed to create stderr capture file")?;
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            stdout
                .try_clone()
                .context("failed to clone stdout capture file")?,
        ))
        .stderr(Stdio::from(
            stderr
                .try_clone()
                .context("failed to clone stderr capture file")?,
        ));
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn command while {operation}"))?;
    let wait = wait_for_child(&mut child, timeout, operation)?;
    let stdout = read_capture(&stdout).context("failed to read command stdout")?;
    let stderr = read_capture(&stderr).context("failed to read command stderr")?;

    match wait {
        WaitResult::Exited(status) => Ok(Output {
            status,
            stdout,
            stderr,
        }),
        WaitResult::TimedOut => Err(anyhow::Error::new(TimedOutOutput { stdout, stderr }))
            .with_context(|| format!("timed out while {operation} after {timeout:?}")),
    }
}

fn wait_for_child(
    child: &mut Child,
    timeout: Duration,
    operation: &str,
) -> anyhow::Result<WaitResult> {
    let process_group = i32::try_from(child.id()).context("command pid exceeded i32")?;
    let deadline = Instant::now() + timeout;

    loop {
        match child
            .try_wait()
            .with_context(|| format!("failed to wait for command while {operation}"))?
        {
            Some(status) => return Ok(WaitResult::Exited(status)),
            None if Instant::now() >= deadline => {
                // Every command is its own process-group leader, so this also
                // terminates descendants that inherited its output fds.
                unsafe {
                    libc::kill(-process_group, libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                return Ok(WaitResult::TimedOut);
            }
            None => {
                std::thread::sleep(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(WAIT_POLL_INTERVAL),
                );
            }
        }
    }
}

fn read_capture(file: &File) -> std::io::Result<Vec<u8>> {
    let length = file.metadata()?.len();
    let mut bytes = Vec::new();
    let mut offset = 0_u64;
    let mut buffer = [0_u8; 8192];
    while offset < length {
        let remaining = usize::try_from((length - offset).min(buffer.len() as u64))
            .expect("capture chunk length fits usize");
        let read = file.read_at(&mut buffer[..remaining], offset)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        offset += read as u64;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_timeout_does_not_wait_for_descendants_holding_inherited_fds() {
        let temp = tempfile::TempDir::new().unwrap();
        let descendant_marker = temp.path().join("descendant-survived");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("(trap '' HUP; sleep 0.3; printf leaked > \"$1\") & wait")
            .arg("sh")
            .arg(&descendant_marker);

        let started = Instant::now();
        let error = command_output_with_timeout(
            command,
            Duration::from_millis(50),
            "running inherited-fd regression child",
        )
        .expect_err("the command should time out");
        let elapsed = started.elapsed();

        assert!(
            format!("{error:#}").contains("timed out"),
            "the timeout must be reported: {error:#}"
        );
        assert!(
            elapsed < Duration::from_millis(250),
            "inherited output fds must not delay the timeout; took {elapsed:?}"
        );
        std::thread::sleep(Duration::from_millis(350));
        assert!(
            !descendant_marker.exists(),
            "the timed-out command's process group must be terminated"
        );
    }
}
