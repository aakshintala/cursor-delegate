//! `delegate models` and `delegate doctor` against a fake cursor-agent script.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const ABOUT: &str = "\
About Cursor CLI

CLI Version         2026.09.28-64d2043
Model               Composer 2.5
Subscription Tier   Pro
User Email          alice@example.com";

fn agent_script(listed: &str) -> String {
    format!(
        r#"#!/bin/sh
case "$1" in
  --version) echo "2026.09.28-64d2043" ;;
  about) printf '%s\n' "{ABOUT}" ;;
  models|--list-models) printf '%s\n' {listed} ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac
"#
    )
}

const FULL_LIST: &str = "composer-2.5 grok-4.7-high grok-4.7-xhigh";
const MISSING_XHIGH: &str = "composer-2.5 grok-4.7-high";

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str, script: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cdm-info-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let agent = dir.join("agent.sh");
        std::fs::write(&agent, script).unwrap();
        std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        Env { dir }
    }

    fn delegate(&self, args: &[&str], stdin: Option<&str>) -> Output {
        self.delegate_with_bin(args, stdin, &self.dir.join("agent.sh").to_string_lossy())
    }

    fn delegate_with_bin(&self, args: &[&str], stdin: Option<&str>, bin: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_delegate"))
            .args(args)
            .current_dir(&self.dir)
            .env("TMPDIR", &self.dir)
            .env("CURSOR_AGENT_BIN", bin)
            // Isolate from the developer machine's real host profile: exact
            // table assertions need the bundled models only.
            .env(
                "CURSOR_DELEGATE_HOST_PROFILE",
                self.dir.join("nonexistent-profile.json"),
            )
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
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const ALL_IDS: [&str; 12] = [
    "composer-2.5",
    "grok-4.7-high",
    "grok-4.7-xhigh",
    "opencode-go/muse-spark-1.3-contributor",
    "opencode-go/glm-5.3-flash",
    "opencode-go/deepseek-v4.1-flash",
    "openai-codex/gpt-6-astra",
    "openai-codex/gpt-6-luna",
    "openai-codex/gpt-6.1-sol",
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-fable-5-1",
];

const SORTED_IDS: [&str; 12] = [
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "composer-2.5",
    "grok-4.7-high",
    "grok-4.7-xhigh",
    "openai-codex/gpt-6-astra",
    "openai-codex/gpt-6-luna",
    "openai-codex/gpt-6.1-sol",
    "opencode-go/deepseek-v4.1-flash",
    "opencode-go/glm-5.3-flash",
    "opencode-go/muse-spark-1.3-contributor",
];

#[test]
fn models_lists_all_twelve_with_default_marked() {
    let e = Env::new("models", &agent_script(FULL_LIST));
    let out = e.delegate(&["models"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    for id in ALL_IDS {
        assert!(stdout.contains(id), "missing {id}:\n{stdout}");
    }
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 13, "{stdout}");
    assert_eq!(
        lines[0],
        "  ID                                      LABEL                       BACKEND  $IN/1M  $OUT/1M"
    );
    assert_eq!(
        lines[4],
        "* composer-2.5                            Composer 2.5                cursor     0.50     2.50"
    );
    assert_eq!(
        lines[12],
        "  opencode-go/muse-spark-1.3-contributor  Muse Spark 1.3 Contributor  pi         0.10     0.20"
    );
    // Rows sort by backend, then by id; only the default row is starred.
    let ids: Vec<&str> = lines[1..]
        .iter()
        .map(|l| l[2..].split_whitespace().next().unwrap())
        .collect();
    assert_eq!(ids, SORTED_IDS);
    assert_eq!(lines.iter().filter(|l| l.starts_with('*')).count(), 1);
}

#[test]
fn doctor_passes_against_full_fake() {
    let e = Env::new("doctor-ok", &agent_script(FULL_LIST));
    let out = e.delegate(&["doctor"], None);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("ok    delegate 0.5.0"), "{stdout}");
    assert!(
        stdout.contains("ok    cursor: cursor-agent 2026.09.28-64d2043 ("),
        "{stdout}"
    );
    assert!(stdout.contains("ok    cursor: logged in"), "{stdout}");
    assert!(
        stdout.contains("skip  pi: backend not implemented (6 models)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("skip  claude: backend not implemented (3 models)"),
        "{stdout}"
    );
    assert!(!stdout.contains("warn"), "{stdout}");
    assert!(!stdout.contains("fail"), "{stdout}");
}

#[test]
fn doctor_warns_on_missing_model_and_still_exits_0() {
    let e = Env::new("doctor-warn", &agent_script(MISSING_XHIGH));
    let out = e.delegate(&["doctor"], None);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout
            .contains("warn  cursor: model grok-4.7-xhigh missing from cursor-agent --list-models"),
        "{stdout}"
    );
    assert!(!stdout.contains("fail"), "{stdout}");
}

#[test]
fn doctor_fails_when_binary_missing() {
    let e = Env::new("doctor-fail", &agent_script(FULL_LIST));
    let out = e.delegate_with_bin(&["doctor"], None, "/nonexistent/cursor-agent-xyz");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("fail  cursor: cursor-agent not found"),
        "{stdout}"
    );
}

#[test]
fn run_rejects_unimplemented_backend_with_exit_2() {
    let e = Env::new("run-gate", &agent_script(FULL_LIST));
    let out = e.delegate(&["run", "--model", "claude-sonnet-5-5"], Some("hi"));
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(
            r#"model "claude-sonnet-5-5" uses backend "claude", which is not implemented yet"#
        ),
        "{err}"
    );
    let out = e.delegate(&["run", "--model", "openai-codex/gpt-6-luna"], Some("hi"));
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(
            r#"model "openai-codex/gpt-6-luna" uses backend "pi", which is not implemented yet"#
        ),
        "{err}"
    );
}
