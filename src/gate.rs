use crate::types::GateResult;
use crate::util::{Abort, tail};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Only the last 2048 bytes of stdout+stderr are reported, so keep that much of each.
const KEEP: usize = 2048;
/// Gates run test suites, so the bound is generous — but it exists: a gate like
/// `sleep infinity` used to hang finalize forever, keeping the job's heartbeat alive and
/// making cancel/shutdown unable to finish the job.
pub const DEFAULT_GATE_TIMEOUT_MS: u64 = 10 * 60 * 1000;

/// How long a SIGTERMed gate gets to exit before SIGKILL.
const KILL_GRACE: Duration = Duration::from_secs(2);

pub struct GateOpts<'a> {
    pub timeout_ms: Option<u64>,
    pub signal: Option<&'a Abort>,
}

fn read_tail(mut r: impl Read) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = r.read(&mut buf) {
        if n == 0 {
            break;
        }
        kept.extend_from_slice(&buf[..n]);
        if kept.len() > KEEP {
            kept.drain(..kept.len() - KEEP);
        }
    }
    kept
}

/// Run the postcondition `command` via `/bin/sh -c` in `cwd`. Never fails — always returns a
/// GateResult, `passed = exitCode == 0`. Bounded: the gate's whole process group is
/// SIGTERMed after `timeout_ms` or when `signal` is set, so a test runner the shell started
/// can't keep the pipes open.
pub fn run_gate(command: &str, cwd: &str, opts: GateOpts<'_>) -> GateResult {
    let timeout = Duration::from_millis(opts.timeout_ms.unwrap_or(DEFAULT_GATE_TIMEOUT_MS));
    let start = Instant::now();
    let spawned = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            return GateResult {
                command: command.to_string(),
                exit_code: 1,
                passed: false,
                output_tail: e.to_string(),
                error: None,
            };
        }
    };
    let pgid = child.id() as libc::pid_t;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (status, killed, out, err) = std::thread::scope(|s| {
        let out = s.spawn(|| read_tail(stdout));
        let err = s.spawn(|| read_tail(stderr));
        let mut termed_at: Option<Instant> = None;
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Some(st),
                Ok(None) => {}
                Err(_) => break None,
            }
            let aborted = opts.signal.is_some_and(|a| a.load(Ordering::SeqCst));
            match termed_at {
                None if aborted || start.elapsed() >= timeout => {
                    termed_at = Some(Instant::now());
                    unsafe { libc::kill(-pgid, libc::SIGTERM) };
                }
                // A gate that traps SIGTERM would otherwise hold finalize and the path lock forever.
                Some(t) if t.elapsed() >= KILL_GRACE => unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                },
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let killed = termed_at.is_some();
        if killed {
            // The shell is gone; take down anything it left holding the pipes.
            unsafe { libc::kill(-pgid, libc::SIGKILL) };
        }
        (
            status,
            killed,
            out.join().unwrap_or_default(),
            err.join().unwrap_or_default(),
        )
    });
    let combined = String::from_utf8_lossy(&out).into_owned() + &String::from_utf8_lossy(&err);
    let exit_code = match status {
        Some(st) => st.code().unwrap_or(1),
        None => 1,
    };
    GateResult {
        command: command.to_string(),
        exit_code,
        passed: exit_code == 0 && !killed,
        output_tail: tail(&combined, KEEP),
        error: killed
            .then(|| format!("gate killed after timeout or abort signal (exitCode {exit_code})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    #[test]
    fn passing_command() {
        let r = run_gate(
            "echo hello && true",
            &std::env::current_dir().unwrap().to_string_lossy(),
            GateOpts {
                timeout_ms: None,
                signal: None,
            },
        );
        assert!(r.passed);
        assert_eq!(r.exit_code, 0);
        assert!(r.output_tail.contains("hello"));
    }

    #[test]
    fn failing_command() {
        let r = run_gate(
            "echo boom >&2; exit 3",
            &std::env::current_dir().unwrap().to_string_lossy(),
            GateOpts {
                timeout_ms: None,
                signal: None,
            },
        );
        assert!(!r.passed);
        assert_eq!(r.exit_code, 3);
        assert!(r.output_tail.contains("boom"));
    }

    #[test]
    fn never_throws_nonsense() {
        let r = run_gate(
            "this-command-does-not-exist-xyz",
            &std::env::current_dir().unwrap().to_string_lossy(),
            GateOpts {
                timeout_ms: None,
                signal: None,
            },
        );
        assert!(!r.passed);
        assert_ne!(r.exit_code, 0);
    }

    #[test]
    fn never_throws_bad_cwd() {
        let r = run_gate(
            "true",
            "/no/such/dir-xyz",
            GateOpts {
                timeout_ms: None,
                signal: None,
            },
        );
        assert!(!r.passed);
        assert_ne!(r.exit_code, 0);
    }

    #[test]
    fn output_tail_capped() {
        let r = run_gate(
            "seq 1 2000",
            &std::env::current_dir().unwrap().to_string_lossy(),
            GateOpts {
                timeout_ms: None,
                signal: None,
            },
        );
        assert!(r.passed);
        assert_eq!(r.command, "seq 1 2000");
        assert!(r.output_tail.len() <= 2048 + 4);
        assert!(r.output_tail.contains("2000"));
    }

    #[test]
    fn hung_gate_killed_by_timeout() {
        let start = Instant::now();
        let r = run_gate(
            "sleep 30",
            &std::env::current_dir().unwrap().to_string_lossy(),
            GateOpts {
                timeout_ms: Some(200),
                signal: None,
            },
        );
        assert!(start.elapsed().as_millis() < 5000);
        assert!(!r.passed);
        assert!(r.error.unwrap_or_default().contains("timeout or abort"));
    }

    #[test]
    fn gate_ignoring_sigterm_is_sigkilled() {
        let start = Instant::now();
        let r = run_gate(
            "trap '' TERM; sleep 30",
            &std::env::current_dir().unwrap().to_string_lossy(),
            GateOpts {
                timeout_ms: Some(200),
                signal: None,
            },
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "took {:?}",
            start.elapsed()
        );
        assert!(!r.passed);
    }

    #[test]
    fn aborted_signal_kills_gate() {
        let flag: Abort = Arc::new(AtomicBool::new(false));
        let flag2 = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            flag2.store(true, Ordering::SeqCst);
        });
        let r = run_gate(
            "sleep 30",
            &std::env::current_dir().unwrap().to_string_lossy(),
            GateOpts {
                timeout_ms: Some(10_000),
                signal: Some(&flag),
            },
        );
        assert!(!r.passed);
        assert!(r.error.unwrap_or_default().contains("timeout or abort"));
    }

    #[test]
    fn killing_the_gate_takes_down_grandchildren_holding_the_pipes() {
        let start = Instant::now();
        let r = run_gate(
            "sleep 30 & sleep 30; wait",
            &std::env::current_dir().unwrap().to_string_lossy(),
            GateOpts {
                timeout_ms: Some(200),
                signal: None,
            },
        );
        assert!(start.elapsed().as_millis() < 5000);
        assert!(!r.passed);
    }
}
