//! Wall-clock and supervisor RSS budgets for the `delegate` CLI (debug test build).

use serde_json::Value;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FAKE_AGENT: &str = r#"#!/bin/sh
printf '%s\n' "$@" > "$(dirname "$0")/argv.txt"
rel="$(dirname "$0")/go"
case "$*" in
  *CANCELME*)
    sleep 300 &
    echo $! > "$(dirname "$0")/grandchild.pid"
    echo $$ > "$(dirname "$0")/agent.pid"
    i=0
    while [ ! -e "$rel" ] && [ "$i" -lt 400 ]; do sleep 0.05; i=$((i+1)); done
    printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"late\nSTATUS: DONE","session_id":"s-9"}'
    exit 0
    ;;
esac
case "$*" in
  *SLOW*)
    i=0
    while [ ! -e "$rel" ] && [ "$i" -lt 400 ]; do sleep 0.05; i=$((i+1)); done
    ;;
esac
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"391\nSTATUS: DONE","session_id":"s-1"}'
"#;

const ABOUT: &str = "\
About Cursor CLI

CLI Version         2026.09.28-64d2043
Model               Composer 2.5
Subscription Tier   Pro
User Email          alice@example.com";

const FULL_LIST: &str = "composer-2.5 grok-4.7-high grok-4.7-xhigh";

fn info_agent_script() -> String {
    format!(
        r#"#!/bin/sh
case "$1" in
  --version) echo "2026.09.28-64d2043" ;;
  about) printf '%s\n' "{ABOUT}" ;;
  models|--list-models) printf '%s\n' {FULL_LIST} ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac
"#
    )
}

const RUN_BUDGET_MS: u128 = 102; // measured: 34ms on M-series Mac, budget 3x
const RESUME_BUDGET_MS: u128 = 93; // measured: 31ms on M-series Mac, budget 3x
const CANCEL_BUDGET_MS: u128 = 192; // measured: 64ms on M-series Mac, budget 3x
const WATCH_BUDGET_MS: u128 = 6; // measured: 2ms on M-series Mac, budget 3x
const DOCTOR_BUDGET_MS: u128 = 567; // measured: 189ms on M-series Mac, budget 3x
const MODELS_BUDGET_MS: u128 = 6; // measured: 2ms on M-series Mac, budget 3x
const SUPERVISOR_RSS_BUDGET_KB: u64 = 8736; // measured: 2912KB on M-series Mac, budget 3x

/// Wall-clock and RSS samples are not meaningful when these tests run in parallel.
static BUDGET_SERIAL: Mutex<()> = Mutex::new(());

fn serial_budgets(f: impl FnOnce()) {
    let _guard = BUDGET_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    f();
}

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cdm-budget-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agent = dir.join("agent.sh");
        std::fs::write(&agent, FAKE_AGENT).unwrap();
        std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        Env { dir }
    }

    fn new_info(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cdm-budget-info-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agent = dir.join("agent.sh");
        std::fs::write(&agent, info_agent_script()).unwrap();
        std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        Env { dir }
    }

    fn release(&self) {
        std::fs::write(self.dir.join("go"), "").unwrap();
    }

    fn until_agent_spawned(&self) {
        let t = Instant::now();
        while !(self.dir.join("agent.pid").exists() && self.dir.join("grandchild.pid").exists()) {
            assert!(
                t.elapsed() < Duration::from_secs(10),
                "timed out waiting for CANCELME agent"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn delegate(&self, args: &[&str], stdin: Option<&str>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_delegate"));
        cmd.args(args)
            .current_dir(&self.dir)
            .env("TMPDIR", &self.dir)
            .env("CURSOR_AGENT_BIN", self.dir.join("agent.sh"))
            .env("DELEGATE_HEARTBEAT_MS", "100");
        if args.first() == Some(&"models") || args.first() == Some(&"doctor") {
            cmd.env(
                "CURSOR_DELEGATE_HOST_PROFILE",
                self.dir.join("nonexistent-profile.json"),
            );
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut si = child.stdin.take().unwrap();
        if let Some(s) = stdin {
            let _ = si.write_all(s.as_bytes());
        }
        drop(si);
        child.wait_with_output().unwrap()
    }

    fn wall_ms(&self, args: &[&str], stdin: Option<&str>) -> (u128, Output) {
        // One throwaway spawn so the timed sample is not paying a cold binary load.
        let _ = self.delegate(&["watch", "nope"], None);
        let start = Instant::now();
        let out = self.delegate(args, stdin);
        (start.elapsed().as_millis(), out)
    }

    fn run_id(&self, prompt: &str) -> String {
        let (ms, out) = self.wall_ms(&["run", "--model", "composer-2.5"], Some(prompt));
        assert!(
            out.status.success(),
            "run failed in {ms}ms: stderr={} stdout={}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn record(&self, id: &str) -> Value {
        let p = self.dir.join("delegate-jobs").join(format!("{id}.json"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    fn wait_terminal(&self, id: &str) {
        let out = self.delegate(&["watch", id, "--timeout", "15"], None);
        assert_eq!(out.status.code(), Some(0));
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(10), "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn supervisor_rss_kb(pid: i32) -> u64 {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    assert!(
        out.status.success(),
        "ps failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("rss parse")
}

#[test]
fn run_exits_within_wall_clock_budget() {
    serial_budgets(|| {
        let e = Env::new("run");
        let (ms, out) = e.wall_ms(&["run", "--model", "composer-2.5"], Some("SLOW brief"));
        assert!(
            out.status.success(),
            "stderr={} stdout={}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        let id = String::from_utf8(out.stdout).unwrap().trim().to_string();
        assert_eq!(e.record(&id)["status"], "RUNNING");
        assert!(
            ms <= RUN_BUDGET_MS,
            "run took {ms}ms, budget {RUN_BUDGET_MS}ms"
        );
        e.release();
        e.wait_terminal(&id);
    });
}

#[test]
fn resume_exits_within_wall_clock_budget() {
    serial_budgets(|| {
        let e = Env::new("resume");
        let a = e.run_id("quick");
        e.wait_terminal(&a);
        let (ms, out) = e.wall_ms(&["resume", &a], Some("SLOW follow"));
        assert!(
            out.status.success(),
            "stderr={} stdout={}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        let b = String::from_utf8(out.stdout).unwrap().trim().to_string();
        assert_eq!(e.record(&b)["status"], "RUNNING");
        assert!(
            ms <= RESUME_BUDGET_MS,
            "resume took {ms}ms, budget {RESUME_BUDGET_MS}ms"
        );
        e.release();
        e.wait_terminal(&b);
    });
}

#[test]
fn cancel_exits_within_wall_clock_budget() {
    serial_budgets(|| {
        let e = Env::new("cancel");
        let id = e.run_id("CANCELME");
        e.until_agent_spawned();
        assert_eq!(e.record(&id)["status"], "RUNNING");
        let (ms, out) = e.wall_ms(&["cancel", &id], None);
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            ms <= CANCEL_BUDGET_MS,
            "cancel took {ms}ms, budget {CANCEL_BUDGET_MS}ms"
        );
    });
}

#[test]
fn watch_terminal_job_within_wall_clock_budget() {
    serial_budgets(|| {
        let e = Env::new("watch");
        let id = e.run_id("quick");
        e.wait_terminal(&id);
        let (ms, out) = e.wall_ms(&["watch", &id], None);
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            ms <= WATCH_BUDGET_MS,
            "watch took {ms}ms, budget {WATCH_BUDGET_MS}ms"
        );
    });
}

#[test]
fn doctor_exits_within_wall_clock_budget() {
    serial_budgets(|| {
        let e = Env::new_info("doctor");
        let (ms, out) = e.wall_ms(&["doctor"], None);
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            ms <= DOCTOR_BUDGET_MS,
            "doctor took {ms}ms, budget {DOCTOR_BUDGET_MS}ms"
        );
    });
}

#[test]
fn models_exits_within_wall_clock_budget() {
    serial_budgets(|| {
        let e = Env::new_info("models");
        let (ms, out) = e.wall_ms(&["models"], None);
        assert!(
            out.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            ms <= MODELS_BUDGET_MS,
            "models took {ms}ms, budget {MODELS_BUDGET_MS}ms"
        );
    });
}

#[test]
fn supervisor_rss_while_running_within_budget() {
    serial_budgets(|| {
        let e = Env::new("rss");
        let id = e.run_id("SLOW");
        until("RUNNING", || e.record(&id)["status"] == "RUNNING");
        let pid = e.record(&id)["supervisorPid"].as_i64().unwrap() as i32;
        let rss = supervisor_rss_kb(pid);
        assert!(
            rss <= SUPERVISOR_RSS_BUDGET_KB,
            "supervisor rss {rss}KB, budget {SUPERVISOR_RSS_BUDGET_KB}KB"
        );
        e.release();
        e.wait_terminal(&id);
    });
}
