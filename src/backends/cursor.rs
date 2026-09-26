//! The cursor-agent backend: spawn the child, parse stream-json (NDJSON) from stdout,
//! report progress/liveness events, and return a BackendResult when it exits.

use super::types::{Backend, BackendResult, Event, EventFn, ProgressSnapshotRaw, Spawned};
use crate::stream::{StreamState, init_stream_state, parse_line};
use crate::types::{JobSpec, RawCursorJson};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Only a 2 KB tail of stderr is ever reported; keep a bounded window of it.
const STDERR_KEEP: usize = 64 * 1024;

pub struct CursorAdapter;

pub fn make_cursor_adapter() -> CursorAdapter {
    CursorAdapter
}

impl Backend for CursorAdapter {
    fn run(&self, spec: &JobSpec) -> Spawned {
        let spawned = Command::new(&spec.bin)
            .args(&spec.argv)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Own process group, so cancel also stops the shell commands the agent started.
            .process_group(0)
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                let msg = e.to_string();
                return Spawned {
                    kill: Box::new(|| {}),
                    drive: Box::new(move |_| BackendResult {
                        raw: RawCursorJson {
                            is_error: Some(true),
                            result: Some(msg.clone()),
                            ..Default::default()
                        },
                        clean_exit: false,
                        stderr: msg,
                    }),
                };
            }
        };
        let pid = child.id() as libc::pid_t;
        // Set once the child is reaped, so a late kill can't hit a recycled pid.
        let reaped = Arc::new(AtomicBool::new(false));
        let reaped_k = Arc::clone(&reaped);
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        Spawned {
            kill: Box::new(move || {
                if !reaped_k.load(Ordering::SeqCst) {
                    unsafe { libc::kill(-pid, libc::SIGTERM) };
                }
            }),
            drive: Box::new(move |on| {
                drive_streams(stdout, stderr, on, move || {
                    let clean = child.wait().map(|s| s.success()).unwrap_or(false);
                    reaped.store(true, Ordering::SeqCst);
                    clean
                })
            }),
        }
    }
}

/// Pump both pipes to EOF (stderr on a scoped thread), then `wait` for the exit status.
pub fn drive_streams(
    mut stdout: impl Read,
    mut stderr: impl Read + Send,
    on: EventFn<'_>,
    wait: impl FnOnce() -> bool,
) -> BackendResult {
    std::thread::scope(|s| {
        let err = s.spawn(|| {
            let mut kept: Vec<u8> = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                kept.extend_from_slice(&buf[..n]);
                if kept.len() > STDERR_KEEP {
                    kept.drain(..kept.len() - STDERR_KEEP);
                }
                on(Event::Stderr);
            }
            String::from_utf8_lossy(&kept).into_owned()
        });

        let mut state = init_stream_state();
        let mut result: Option<RawCursorJson> = None;
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = [0u8; 8192];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 {
                break;
            }
            on(Event::Activity);
            pending.extend_from_slice(&buf[..n]);
            while let Some(i) = pending.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = pending.drain(..=i).collect();
                handle_line(&line[..i], &mut state, &mut result, on);
            }
        }
        // Flush a trailing line that arrived without a terminating newline.
        if !pending.is_empty() {
            handle_line(&pending, &mut state, &mut result, on);
        }
        let stderr = err.join().unwrap_or_default();
        BackendResult {
            raw: result.unwrap_or_default(),
            clean_exit: wait(),
            stderr,
        }
    })
}

fn handle_line(
    line: &[u8],
    state: &mut StreamState,
    result: &mut Option<RawCursorJson>,
    on: EventFn<'_>,
) {
    let parsed = parse_line(&String::from_utf8_lossy(line), state);
    if parsed.result.is_some() {
        *result = parsed.result;
    }
    if parsed.changed {
        on(Event::Progress(ProgressSnapshotRaw {
            last_tool: state.last_tool.clone(),
            tokens_so_far: state.tokens_so_far,
            last_assistant: state.last_assistant.clone(),
            files_touched: state.files_touched.clone(),
            phase: state.phase.clone(),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn parses_ndjson_emits_progress_resolves_result() {
        // A tool call line (with newline), then the terminal result WITHOUT a trailing newline.
        let stdout = concat!(
            r#"{"type":"tool_call","subtype":"started","tool_call":{"shellToolCall":{}}}"#,
            "\n",
            r#"{"type":"result","subtype":"success","is_error":false,"result":"all good","session_id":"sid","usage":{"outputTokens":7}}"#,
        );
        let progress = Mutex::new(Vec::new());
        let on = |e: Event| {
            if let Event::Progress(p) = e {
                progress.lock().unwrap().push(p);
            }
        };
        let res = drive_streams(stdout.as_bytes(), &b""[..], &on, || true);
        assert!(res.clean_exit);
        assert_eq!(res.raw.result.as_deref(), Some("all good"));
        assert_eq!(res.raw.session_id.as_deref(), Some("sid"));
        let progress = progress.into_inner().unwrap();
        assert!(!progress.is_empty());
        assert_eq!(progress[0].last_tool.as_deref(), Some("shell"));
    }

    #[test]
    fn non_zero_exit_is_unclean_and_stderr_is_captured() {
        let res = drive_streams(&b""[..], &b"trouble"[..], &|_| {}, || false);
        assert!(!res.clean_exit);
        assert_eq!(res.stderr, "trouble");
    }

    #[test]
    fn stderr_keeps_only_a_bounded_tail() {
        let big = vec![b'x'; STDERR_KEEP * 2 + 5];
        let res = drive_streams(&b""[..], &big[..], &|_| {}, || true);
        assert_eq!(res.stderr.len(), STDERR_KEEP);
    }

    #[test]
    fn spawn_failure_is_an_error_result() {
        let spec =
            crate::job_registry::tests::spec_of(|s| s.bin = "/nonexistent/cursor-agent".into());
        let spawned = CursorAdapter.run(&spec);
        (spawned.kill)();
        let res = (spawned.drive)(&|_| {});
        assert!(!res.clean_exit);
        assert_eq!(res.raw.is_error, Some(true));
    }

    #[test]
    fn real_child_runs_and_sigterm_kills_it() {
        let spec = crate::job_registry::tests::spec_of(|s| {
            s.bin = "/bin/sh".into();
            s.argv = vec!["-c".into(), "exec sleep 30".into()];
        });
        let spawned = CursorAdapter.run(&spec);
        let kill = spawned.kill;
        let t = std::thread::spawn(move || (spawned.drive)(&|_| {}));
        std::thread::sleep(std::time::Duration::from_millis(100));
        kill();
        let res = t.join().unwrap();
        assert!(!res.clean_exit);
    }
}
