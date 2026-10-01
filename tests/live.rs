//! Opt-in: real cursor-agent. Run with: cargo test --test live -- --ignored

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
