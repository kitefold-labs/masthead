// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Ending a process from the Processes page.
//!
//! The signal goes through `kill(1)`: the workspace denies unsafe code, so
//! there is no direct `kill(2)` call. A process owned by another user, such as
//! root, is signalled through `pkexec`, which asks for the user's password in
//! the desktop's polkit dialog.

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// SIGTERM: the process may clean up first.
    Term,
    /// SIGKILL: ends the process at once.
    Kill,
}

impl Signal {
    fn name(self) -> &'static str {
        match self {
            Self::Term => "TERM",
            Self::Kill => "KILL",
        }
    }
}

/// A process the user asked to end, as it was when they asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub pid: u32,
    pub name: String,
    pub user: String,
    /// Start time in clock ticks since boot, from `/proc/<pid>/stat`. A
    /// different value later means the PID now belongs to another process.
    start: u64,
    /// True when the process belongs to another user and needs `pkexec`.
    pub privileged: bool,
}

impl Target {
    /// Reads the process's start time and owner. `None` if it has exited.
    pub fn new(pid: u32, name: String, user: String) -> Option<Self> {
        let start = start_time(pid)?;
        let owner = std::fs::metadata(format!("/proc/{pid}")).ok()?.uid();
        let me = std::fs::metadata("/proc/self").ok()?.uid();
        Some(Self { pid, name, user, start, privileged: owner != me })
    }
}

/// Field 22 of `/proc/<pid>/stat`. The name field can hold spaces and
/// parentheses, so fields are counted from the last `)`.
fn start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_start(&stat)
}

fn parse_start(stat: &str) -> Option<u64> {
    stat[stat.rfind(')')? + 1..].split_whitespace().nth(19)?.parse().ok()
}

/// Sends `signal` to `target`. Blocks until `kill`, or the password dialog,
/// finishes. Returns a sentence for the page.
pub fn send(target: &Target, signal: Signal) -> Result<String, String> {
    let Target { pid, name, .. } = target;
    if start_time(*pid) != Some(target.start) {
        return Err(format!("{name} ({pid}) had already exited."));
    }
    let kill = find("kill").ok_or("kill(1) is not installed.")?;
    let mut cmd = if target.privileged {
        // pkexec needs an absolute path to the program it runs.
        let mut c = Command::new(find("pkexec").ok_or("pkexec is not installed, so other users' processes cannot be ended.")?);
        c.arg(kill);
        c
    } else {
        Command::new(kill)
    };
    cmd.args(["-s", signal.name(), "--", &pid.to_string()]);
    let out = cmd.output().map_err(|e| format!("Could not run kill: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(match signal {
            Signal::Term => format!("Asked {name} ({pid}) to quit."),
            Signal::Kill => format!("Killed {name} ({pid})."),
        }),
        // pkexec: 126 when the dialog was dismissed, 127 when not authorised.
        Some(126 | 127) if target.privileged => Err(format!("{name} ({pid}) was left running: not authorised.")),
        _ => {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = err.trim();
            Err(if err.is_empty() { format!("Could not end {name} ({pid}).") } else { format!("Could not end {name} ({pid}): {err}") })
        }
    }
}

fn find(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .chain(["/usr/bin".into(), "/bin".into()])
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_time_skips_odd_names() {
        let stat = "42 (a) b (c)) S 1 42 42 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 1 0 98765 1000 200";
        assert_eq!(parse_start(stat), Some(98765));
        assert_eq!(parse_start("42 (x"), None);
    }

    #[test]
    fn own_process_is_not_privileged() {
        let t = Target::new(std::process::id(), "test".into(), "me".into()).unwrap();
        assert!(!t.privileged);
        assert!(t.start > 0);
    }

    #[test]
    fn ends_a_child() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let t = Target::new(child.id(), "sleep".into(), "me".into()).unwrap();
        assert_eq!(send(&t, Signal::Term).unwrap(), format!("Asked sleep ({}) to quit.", child.id()));
        let status = child.wait().unwrap();
        assert!(!status.success());
        // The PID is gone (or reused), so a second attempt must not signal it.
        assert!(send(&t, Signal::Kill).is_err());
    }
}
