//! Opt-in: real backends. Run with: cargo test --test live -- --ignored

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
#[ignore = "needs a logged-in cursor-agent; spends a Cursor request"]
fn ask_multiplies_on_real_agent() {
    let dir = std::env::temp_dir().join(format!("delegate-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bin = env!("CARGO_BIN_EXE_delegate");
    let mut run = Command::new(bin)
        .args([
            "run",
            "--model",
            "composer-2.5",
            "--capability",
            "read-only",
        ])
        .current_dir(&dir)
        .env("TMPDIR", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    run.stdin
        .take()
        .unwrap()
        .write_all(b"What is 17 * 23? Answer with just the number.")
        .unwrap();
    let id = String::from_utf8_lossy(&run.wait_with_output().unwrap().stdout)
        .trim()
        .to_string();
    let watch = Command::new(bin)
        .args(["watch", &id, "--timeout", "300"])
        .current_dir(&dir)
        .env("TMPDIR", &dir)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&watch.stdout);
    let rec: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(rec["status"], "DONE");
    assert!(rec["result"]["text"].as_str().unwrap().contains("391"));
    assert!(
        rec["result"]["sessionId"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
}

#[test]
#[ignore = "needs a logged-in pi; spends one muse-spark request"]
fn pi_live_plain_answer_records_a_fixture() {
    let dir = std::env::temp_dir().join(format!("delegate-live-pi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // record.sh (and pi itself) run in this cwd; a repo keeps them happy.
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(init.status.success());
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/record.sh");
    let mut record = Command::new(&script)
        .args([
            "pi",
            "opencode-go/muse-spark-1.3-contributor",
            "read-write",
            "live-plain-answer",
            &dir.to_string_lossy(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    record
        .stdin
        .take()
        .unwrap()
        .write_all(b"What is 17 * 23? Answer with just the number.")
        .unwrap();
    let out = record.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "record.sh failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pi/live-plain-answer.stdout");
    let stdout = std::fs::read_to_string(&fixture).unwrap();
    let res = delegate::backends::pi::parse_stdout(&stdout, true, "");
    assert!(res.text.contains("391"), "{}", res.text);
    assert!(
        res.session_id.as_deref().is_some_and(|s| !s.is_empty()),
        "{res:?}"
    );
    assert!(res.cost_usd.is_some_and(|c| c > 0.0), "{res:?}");
}
