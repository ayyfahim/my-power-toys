//! "End task" / "End all": SIGTERM, then SIGKILL after a timeout.

use anyhow::{Result, bail};
use std::time::{Duration, Instant};

/// How a process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillOutcome {
    /// Exited after SIGTERM.
    Terminated,
    /// Still alive after the timeout and was sent SIGKILL.
    Killed,
    /// Was already gone.
    NotRunning,
}

fn send(pid: u32, sig: libc::c_int) -> std::io::Result<()> {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
    };
    if pid <= 0 {
        // Never signal process groups / every process.
        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
    }
    // SAFETY: kill() has no memory-safety preconditions.
    let rc = unsafe { libc::kill(pid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// True while the process exists and is not a zombie.
pub fn is_alive(pid: u32) -> bool {
    match send(pid, 0) {
        Ok(()) => !is_zombie(pid),
        Err(e) => e.raw_os_error() == Some(libc::EPERM),
    }
}

fn is_zombie(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| parse_stat_state(&s))
        .is_some_and(|c| c == 'Z' || c == 'X')
}

/// Process state letter from `/proc/<pid>/stat` (the field after `(comm)`).
pub fn parse_stat_state(stat: &str) -> Option<char> {
    let after = &stat[stat.rfind(')')? + 1..];
    after.trim_start().chars().next()
}

fn signal_checked(pid: u32, sig: libc::c_int) -> Result<bool> {
    match send(pid, sig) {
        Ok(()) => Ok(true),
        Err(e) if e.raw_os_error() == Some(libc::ESRCH) => Ok(false),
        Err(e) if e.raw_os_error() == Some(libc::EPERM) => {
            bail!("permission denied ending process {pid} (try Run as administrator)")
        }
        Err(e) => bail!("failed to signal process {pid}: {e}"),
    }
}

fn wait_for_exit(pids: &[u32], timeout: Duration) -> Vec<u32> {
    let deadline = Instant::now() + timeout;
    loop {
        let alive: Vec<u32> = pids.iter().copied().filter(|&p| is_alive(p)).collect();
        if alive.is_empty() || Instant::now() >= deadline {
            return alive;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// End one process: SIGTERM, wait up to `timeout`, then SIGKILL.
/// A zero timeout sends SIGKILL right away if SIGTERM did not end it.
pub fn end_process(pid: u32, timeout: Duration) -> Result<KillOutcome> {
    if !signal_checked(pid, libc::SIGTERM)? {
        return Ok(KillOutcome::NotRunning);
    }
    if wait_for_exit(&[pid], timeout).is_empty() {
        return Ok(KillOutcome::Terminated);
    }
    signal_checked(pid, libc::SIGKILL)?;
    Ok(KillOutcome::Killed)
}

/// End several processes. SIGTERM is sent to all of them first, then the
/// ones still alive after `timeout` get SIGKILL.
pub fn end_all(pids: &[u32], timeout: Duration) -> Vec<(u32, Result<KillOutcome>)> {
    let mut results = Vec::new();
    let mut waiting = Vec::new();
    for &pid in pids {
        match signal_checked(pid, libc::SIGTERM) {
            Ok(true) => waiting.push(pid),
            Ok(false) => results.push((pid, Ok(KillOutcome::NotRunning))),
            Err(e) => results.push((pid, Err(e))),
        }
    }
    let still_alive = wait_for_exit(&waiting, timeout);
    for pid in waiting {
        if still_alive.contains(&pid) {
            let r = signal_checked(pid, libc::SIGKILL).map(|_| KillOutcome::Killed);
            results.push((pid, r));
        } else {
            results.push((pid, Ok(KillOutcome::Terminated)));
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_state() {
        assert_eq!(parse_stat_state("123 (my (odd) prog) S 1 2 3"), Some('S'));
        assert_eq!(parse_stat_state("9 (z) Z 1"), Some('Z'));
        assert_eq!(parse_stat_state("garbage"), None);
    }

    #[test]
    fn finds_and_ends_real_child() {
        let dir = std::env::temp_dir().join(format!("mpt-fl-kill-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let Ok(mut child) = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(&dir)
            .spawn()
        else {
            return; // no `sleep` available
        };
        let pid = child.id();
        std::thread::sleep(Duration::from_millis(100));
        let res = crate::scan::find_processes(std::slice::from_ref(&dir));
        assert!(res.processes.iter().any(|p| p.pid == pid));
        assert_eq!(
            end_process(pid, Duration::from_secs(2)).unwrap(),
            KillOutcome::Terminated
        );
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_invalid_pids() {
        assert!(end_process(0, Duration::ZERO).is_err());
        assert_eq!(
            end_process(u32::MAX, Duration::ZERO).unwrap(),
            KillOutcome::NotRunning
        );
    }
}
