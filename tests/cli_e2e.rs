//! Drives the real `delegate` binary as a process against a fake cursor-agent.

use serde_json::Value;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const FAKE_AGENT: &str = r#"#!/bin/sh
printf '%s\n' "$@" > "$(dirname "$0")/argv.txt"
printf '%s\n' '{"type":"tool_call","subtype":"started","tool_call":{"shellToolCall":{"args":{"command":"ls"}}}}'
case "$*" in
  *SLOW*) sleep 1 ;;
  *LONG*) sleep 2 ;;
esac
case "$*" in
  *ASKME*) printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"Which db?\nSTATUS: NEEDS_CONTEXT","session_id":"s-2"}' ;;
  *) printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"391\nSTATUS: DONE","session_id":"s-1"}' ;;
esac
"#;

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cdm-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agent = dir.join("agent.sh");
        std::fs::write(&agent, FAKE_AGENT).unwrap();
        std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        Env { dir }
    }

    fn argv(&self) -> Vec<String> {
        let s = std::fs::read_to_string(self.dir.join("argv.txt")).unwrap();
        s.lines().map(String::from).collect()
    }

    fn delegate(&self, args: &[&str], stdin: Option<&str>) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_delegate"))
            .args(args)
            .current_dir(&self.dir)
            .env("TMPDIR", &self.dir)
            .env("CURSOR_AGENT_BIN", self.dir.join("agent.sh"))
            .env("DELEGATE_HEARTBEAT_MS", "100")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut si = child.stdin.take().unwrap();
        if let Some(s) = stdin {
            si.write_all(s.as_bytes()).unwrap();
        }
        drop(si);
        child.wait_with_output().unwrap()
    }

    /// Starts a job and returns its id.
    fn run(&self, prompt: &str) -> String {
        let out = self.delegate(&["run", "--model", "composer-2.5"], Some(prompt));
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn record(&self, id: &str) -> Value {
        let p: PathBuf = self.dir.join("delegate-jobs").join(format!("{id}.json"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    fn wait_terminal(&self, id: &str) -> Value {
        let out = self.delegate(&["watch", id, "--timeout", "10"], None);
        assert_eq!(out.status.code(), Some(0));
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn alive(pid: i64) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(10), "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn run_returns_id_while_job_runs_then_it_finishes() {
    let e = Env::new("run");
    let id = e.run("SLOW do it");
    assert_eq!(id.len(), 36);
    let r = e.record(&id);
    assert_eq!(r["status"], "RUNNING");
    assert_eq!(r["resume"]["model"], "composer-2.5");
    assert_eq!(r["resume"]["capability"], "read-only");
    assert_eq!(r["resume"]["sessionId"], Value::Null);
    let pid = r["supervisorPid"].as_i64().unwrap();
    assert!(alive(pid), "supervisor must outlive `run`");

    let done = e.wait_terminal(&id);
    let argv = e.argv();
    assert!(argv.windows(2).any(|w| w == ["--mode", "ask"]), "{argv:?}");
    assert!(argv.contains(&"--force".to_string()), "{argv:?}");
    assert_eq!(done["status"], "DONE");
    assert_eq!(done["result"]["text"], "391\nSTATUS: DONE");
    assert_eq!(done["result"]["jobId"], id.as_str());
    assert_eq!(done["resume"]["sessionId"], "s-1");
    until("supervisor exit", || !alive(pid));
}

#[test]
fn running_record_heartbeat_advances() {
    let e = Env::new("hb");
    let id = e.run("SLOW");
    let a = e.record(&id)["lastHeartbeatAt"].as_f64().unwrap();
    until("heartbeat", || {
        let r = e.record(&id);
        r["status"] != "RUNNING" || r["lastHeartbeatAt"].as_f64().unwrap() > a
    });
    assert_ne!(
        e.record(&id)["status"],
        "DONE",
        "heartbeat must advance while RUNNING"
    );
    e.wait_terminal(&id);
}

#[test]
fn needs_context_is_parsed() {
    let e = Env::new("nc");
    let id = e.run("ASKME");
    let done = e.wait_terminal(&id);
    assert_eq!(done["status"], "NEEDS_CONTEXT");
    assert_eq!(done["resume"]["sessionId"], "s-2");
}

#[test]
fn read_write_and_prompt_file() {
    let e = Env::new("rw");
    std::fs::write(e.dir.join("p.txt"), "from file").unwrap();
    let out = e.delegate(
        &[
            "run",
            "--model",
            "composer-2.5",
            "--capability",
            "read-write",
            "--prompt-file",
            "p.txt",
        ],
        None,
    );
    assert!(out.status.success());
    let id = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert_eq!(e.record(&id)["resume"]["capability"], "read-write");
    e.wait_terminal(&id);
    let argv = e.argv();
    assert!(
        argv.windows(2).any(|w| w == ["--sandbox", "disabled"]),
        "{argv:?}"
    );
    assert!(argv.contains(&"--force".to_string()), "{argv:?}");
}

#[test]
fn watch_prints_all_in_order_and_times_out_with_exit_1() {
    let e = Env::new("watch");
    let slow = e.run("LONG");
    let fast = e.run("quick");
    let out = e.delegate(&["watch", &slow, &fast, "--timeout", "1"], None);
    assert_eq!(out.status.code(), Some(1));
    let lines: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["status"], "RUNNING");

    let out = e.delegate(&["watch", &fast, &slow, "--timeout", "10"], None);
    assert_eq!(out.status.code(), Some(0));
    let lines: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["result"]["jobId"], fast.as_str());
    assert_eq!(lines[1]["result"]["jobId"], slow.as_str());
}

#[test]
fn watch_unknown_job_exits_2() {
    let e = Env::new("unknown");
    let out = e.delegate(&["watch", "nope"], None);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown job nope"));
}

#[test]
fn bad_run_input_exits_2() {
    let e = Env::new("bad");
    let out = e.delegate(&["run", "--model", "nope-9"], Some("hi"));
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("nope-9") && err.contains("composer-2.5"),
        "{err}"
    );

    let out = e.delegate(&["run", "--model", "composer-2.5"], Some("  \n"));
    assert_eq!(out.status.code(), Some(2));
    std::fs::write(e.dir.join("empty.txt"), "").unwrap();
    let out = e.delegate(
        &[
            "run",
            "--model",
            "composer-2.5",
            "--prompt-file",
            "empty.txt",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(!Path::new(&e.dir.join("delegate-jobs")).exists());
}
