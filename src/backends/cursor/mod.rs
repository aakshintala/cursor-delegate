//! The cursor-agent backend: binary, argv, spawn, and stream-json parsing.

pub(crate) mod doctor;

use super::types::{BackendResult, Event, EventFn, ProgressSnapshotRaw, Spawned};
use crate::stream::{RawCursorJson, StreamState, init_stream_state, parse_line};
use crate::types::{Capability, JobSpec};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;

const NO_RESULT: &str = "no result line";

pub(crate) fn resolve_bin(r#override: Option<&str>) -> String {
    if let Some(o) = r#override.filter(|s| !s.is_empty()) {
        return o.to_string();
    }
    if let Ok(env) = std::env::var("CURSOR_AGENT_BIN")
        && !env.is_empty()
    {
        return env;
    }
    if let Ok(out) = Command::new("which").arg("cursor-agent").output()
        && out.status.success()
    {
        let found = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !found.is_empty() {
            return found;
        }
    }
    crate::util::homedir()
        .join(".local/bin/cursor-agent")
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn argv(
    model: &str,
    capability: Capability,
    session: Option<&str>,
    prompt: &str,
) -> (Vec<String>, bool) {
    let (flags, is_write): (&[&str], bool) = match capability {
        Capability::Ask => (&["--mode", "ask", "--force"], false),
        Capability::WriteUnsandboxed => (&["--sandbox", "disabled", "--force"], true),
    };
    let mut args = vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--trust".into(),
        "--approve-mcps".into(),
        "--model".into(),
        model.to_string(),
    ];
    args.extend(flags.iter().map(|s| (*s).to_string()));
    if let Some(s) = session {
        args.push("--resume".into());
        args.push(s.to_string());
    }
    args.push("--".into());
    args.push(prompt.to_string());
    (args, is_write)
}

pub(crate) fn spawn(spec: &JobSpec) -> Spawned {
    spawn_with_session(spec, None)
}

/// `launched_session` is an id chosen before launch. The stream's own id wins;
/// this fills in when the child dies before a result line (cancel).
pub(crate) fn spawn_with_session(spec: &JobSpec, launched_session: Option<String>) -> Spawned {
    let started = match super::start_child(spec) {
        Ok(s) => s,
        Err(msg) => return super::spawn_failed(&msg),
    };
    let pid = started.pid;
    let reaped = started.reaped;
    let reaped_k = Arc::clone(&reaped);
    let stdout = started.stdout;
    let stderr = started.stderr;
    let mut child = started.child;
    Spawned {
        kill: super::killer(pid, reaped_k),
        drive: Box::new(move |on| {
            let mut state = init_stream_state();
            let mut result: Option<RawCursorJson> = None;
            let pumped = super::pump(
                stdout,
                stderr,
                on,
                move || {
                    let clean = child.wait().map(|s| s.success()).unwrap_or(false);
                    reaped.store(true, Ordering::SeqCst);
                    clean
                },
                |line| handle_line(line, &mut state, &mut result, on),
            );
            finish(
                result,
                pumped.clean_exit,
                &pumped.stderr,
                !pumped.saw_stdout,
                launched_session.as_deref(),
            )
        }),
    }
}

/// Pure parse of a finished cursor-agent stdout. The spawn driver uses [`finish`] too.
pub fn parse_stdout(stdout: &str, clean_exit: bool, stderr: &str) -> BackendResult {
    let mut state = init_stream_state();
    let mut raw = None;
    for line in stdout.split_inclusive('\n') {
        handle_line(line.as_bytes(), &mut state, &mut raw, &|_: Event| {});
    }
    finish(raw, clean_exit, stderr, stdout.is_empty(), None)
}

fn handle_line(
    line: &[u8],
    state: &mut StreamState,
    raw: &mut Option<RawCursorJson>,
    on: EventFn<'_>,
) {
    let parsed = parse_line(&String::from_utf8_lossy(line), state);
    if parsed.result.is_some() {
        *raw = parsed.result;
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

/// Turn the last result line (or the lack of one) into the normalized result.
fn finish(
    raw: Option<RawCursorJson>,
    clean_exit: bool,
    stderr: &str,
    stdout_empty: bool,
    launched_session: Option<&str>,
) -> BackendResult {
    let launched = || launched_session.map(str::to_string);
    if let Some(raw) = raw {
        return BackendResult {
            text: raw.result.unwrap_or_default(),
            session_id: raw.session_id.or_else(launched),
            usage: raw.usage,
            cost_usd: raw.cost_usd,
            is_error: raw.is_error,
            duration_ms: raw.duration_ms,
            clean_exit,
            stderr: stderr.to_string(),
            permission_denials: raw.permission_denials,
        };
    }
    // A non-clean exit with no stdout is the bad-model case: the text is the stderr we kept.
    // Any other missing result line is an error. CANCELLED is the supervisor's status, never ours.
    // ponytail: the exit code is not carried (clean_exit is a bool); carry Option<i32> if an error text ever needs it.
    let text = if !clean_exit && stdout_empty {
        stderr.to_string()
    } else {
        NO_RESULT.to_string()
    };
    BackendResult {
        text,
        session_id: launched(),
        is_error: Some(true),
        clean_exit,
        stderr: stderr.to_string(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Usage;
    use std::sync::Mutex;

    fn drive(stdout: &[u8], stderr: &[u8], on: EventFn<'_>, clean: bool) -> BackendResult {
        let mut state = init_stream_state();
        let mut result: Option<RawCursorJson> = None;
        let pumped = crate::backends::pump(
            stdout,
            stderr,
            on,
            move || clean,
            |line| {
                handle_line(line, &mut state, &mut result, on);
            },
        );
        finish(
            result,
            pumped.clean_exit,
            &pumped.stderr,
            !pumped.saw_stdout,
            None,
        )
    }

    #[test]
    fn ask_is_read_only_and_write_disables_the_sandbox() {
        let (ask, ask_write) = argv("composer-2.5", Capability::Ask, None, "hi");
        assert!(!ask_write);
        assert_eq!(
            ask.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "--print",
                "--output-format",
                "stream-json",
                "--trust",
                "--approve-mcps",
                "--model",
                "composer-2.5",
                "--mode",
                "ask",
                "--force",
                "--",
                "hi",
            ]
        );
        let (write, is_write) = argv(
            "composer-2.5",
            Capability::WriteUnsandboxed,
            Some("sid"),
            "go",
        );
        assert!(is_write);
        let pair = |a, b| write.windows(2).any(|w| w[0] == a && w[1] == b);
        assert!(pair("--sandbox", "disabled"));
        assert!(pair("--resume", "sid"));
        assert!(!pair("--sandbox", "enabled"));
    }

    #[test]
    fn explicit_override_wins() {
        assert_eq!(
            resolve_bin(Some("/custom/cursor-agent")),
            "/custom/cursor-agent"
        );
    }

    #[test]
    fn env_used_when_no_override() {
        let prev = std::env::var("CURSOR_AGENT_BIN").ok();
        unsafe { std::env::set_var("CURSOR_AGENT_BIN", "/env/cursor-agent") };
        assert_eq!(resolve_bin(None), "/env/cursor-agent");
        match prev {
            Some(v) => unsafe { std::env::set_var("CURSOR_AGENT_BIN", v) },
            None => unsafe { std::env::remove_var("CURSOR_AGENT_BIN") },
        }
    }

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
        let res = drive(stdout.as_bytes(), &b""[..], &on, true);
        assert!(res.clean_exit);
        assert_eq!(res.text, "all good");
        assert_eq!(res.session_id.as_deref(), Some("sid"));
        let progress = progress.into_inner().unwrap();
        assert!(!progress.is_empty());
        assert_eq!(progress[0].last_tool.as_deref(), Some("shell"));
    }

    #[test]
    fn non_zero_exit_is_unclean_and_stderr_is_captured() {
        let res = drive(&b""[..], &b"trouble"[..], &|_| {}, false);
        assert!(!res.clean_exit);
        assert_eq!(res.stderr, "trouble");
        assert_eq!(res.is_error, Some(true));
        assert_eq!(res.text, "trouble");
    }

    #[test]
    fn stderr_keeps_only_a_bounded_tail() {
        let big = vec![b'x'; crate::backends::STDERR_KEEP * 2 + 5];
        let res = drive(&b""[..], &big[..], &|_| {}, true);
        assert_eq!(res.stderr.len(), crate::backends::STDERR_KEEP);
    }

    #[test]
    fn spawn_failure_is_an_error_result() {
        let spec = crate::job::tests::spec_of(|s| s.bin = "/nonexistent/cursor-agent".into());
        let spawned = spawn(&spec);
        (spawned.kill)();
        let res = (spawned.drive)(&|_| {});
        assert!(!res.clean_exit);
        assert_eq!(res.is_error, Some(true));
        assert_eq!(res.text, res.stderr);
        assert!(!res.text.is_empty());
    }

    #[test]
    fn real_child_runs_and_sigterm_kills_it() {
        let spec = crate::job::tests::spec_of(|s| {
            s.bin = "/bin/sh".into();
            s.argv = vec!["-c".into(), "exec sleep 30".into()];
        });
        let spawned = spawn(&spec);
        let kill = spawned.kill;
        let t = std::thread::spawn(move || (spawned.drive)(&|_| {}));
        std::thread::sleep(std::time::Duration::from_millis(100));
        kill();
        let res = t.join().unwrap();
        assert!(!res.clean_exit);
    }

    #[test]
    fn unparseable_lines_are_skipped_and_a_clean_exit_without_a_result_is_an_error() {
        let skipped = parse_stdout("not json\n", true, "");
        assert!(skipped.clean_exit);
        assert_eq!(skipped.is_error, Some(true));
        assert_eq!(skipped.text, NO_RESULT);
        assert!(!skipped.text.contains("CANCELLED"));

        let res = parse_stdout(
            "not json\n{\"type\":\"result\",\"is_error\":false,\"result\":\"ok\",\"session_id\":\"s\"}\n",
            true,
            "",
        );
        assert_eq!(res.text, "ok");
        assert_eq!(res.session_id.as_deref(), Some("s"));
        assert_eq!(res.is_error, Some(false));
    }

    #[test]
    fn unclean_empty_stdout_uses_stderr_as_text() {
        let res = parse_stdout("", false, "Cannot use this model");
        assert!(!res.clean_exit);
        assert_eq!(res.is_error, Some(true));
        assert_eq!(res.text, "Cannot use this model");
        assert!(res.session_id.is_none());
        assert!(res.usage.is_none());
    }

    /// Exit status is not stored in the fixtures. Everything else is a clean exit.
    fn clean_exit_for(stem: &str) -> bool {
        !matches!(stem, "error-bad-model" | "cancelled")
    }

    #[test]
    fn fixtures_parse_to_a_normalized_result() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cursor");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("stdout"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no cursor stdout fixtures");

        for path in files {
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
            let stdout = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let stderr_path = path.with_extension("stderr");
            let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
            let clean = clean_exit_for(&stem);
            let res = parse_stdout(&stdout, clean, &stderr);

            assert_eq!(res.clean_exit, clean, "{stem}");
            assert_eq!(res.stderr, stderr, "{stem}");
            assert_eq!(res.cost_usd, None, "{stem}");
            if !clean && stdout.is_empty() {
                assert_eq!(res.is_error, Some(true), "{stem}");
                assert_eq!(res.text, stderr, "{stem}");
                assert!(res.session_id.is_none() && res.usage.is_none(), "{stem}");
            } else if !clean {
                assert_eq!(res.is_error, Some(true), "{stem}");
                assert_eq!(res.text, NO_RESULT, "{stem}");
                assert!(res.session_id.is_none() && res.usage.is_none(), "{stem}");
            } else {
                assert_eq!(res.is_error, Some(false), "{stem}");
                assert!(res.session_id.is_some() && res.usage.is_some(), "{stem}");
                assert!(!res.text.is_empty(), "{stem}");
            }

            if stem == "plain-answer" {
                assert_eq!(
                    res,
                    BackendResult {
                        text: "391\n\nSTATUS: DONE".into(),
                        session_id: Some("54b96753-cec4-4820-ade9-aa49ebbb463a".into()),
                        usage: Some(Usage {
                            input_tokens: 9451.0,
                            output_tokens: 72.0,
                            cache_read_tokens: 3872.0,
                            cache_write_tokens: 0.0,
                        }),
                        cost_usd: None,
                        is_error: Some(false),
                        duration_ms: Some(3279.0),
                        clean_exit: true,
                        stderr: String::new(),
                        permission_denials: Vec::new(),
                    }
                );
            }
            if stem == "tool-calls-fix" {
                let mut state = init_stream_state();
                for line in stdout.lines() {
                    parse_line(line, &mut state);
                }
                assert!(
                    state.last_tool.is_some(),
                    "tool-calls-fix progress saw no tool call"
                );
            }
        }
    }
}
