//! Small helpers for running external tools with a timeout.

use anyhow::{Context, Result, bail};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct CmdOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Run `program args...` (no shell), capturing stdout/stderr.
/// The child is killed if it runs longer than `timeout`.
pub fn run(program: &str, args: &[String], timeout: Duration) -> Result<CmdOutput> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run {program} (is it installed?)"))?;

    // Drain pipes on threads so a chatty child can't block on a full pipe.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_string(&mut s);
        }
        s
    });
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_string(&mut s);
        }
        s
    });

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("{program} timed out after {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(15));
    };

    Ok(CmdOutput {
        success: status.success(),
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
    })
}

/// Whether `program` can be found in PATH.
pub fn in_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}
