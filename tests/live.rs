//! Opt-in: drives the real binary against the real cursor-agent (installed + logged in).
//! Costs one Cursor request. Run with: cargo test --test live -- --ignored

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
#[ignore = "needs a logged-in cursor-agent; spends a Cursor request"]
fn ask_task_round_trips_through_real_cursor_agent() {
    let dir = std::env::temp_dir().join(format!("cdm-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cursor-delegate-mcp"))
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut call = |req: Value| -> Value {
        writeln!(stdin, "{req}").unwrap();
        loop {
            let mut line = String::new();
            assert!(
                stdout.read_line(&mut line).unwrap() > 0,
                "server closed stdout"
            );
            let v: Value = serde_json::from_str(&line).unwrap();
            if v.get("id") == req.get("id") {
                return v;
            }
        }
    };
    let tool = |call: &mut dyn FnMut(Value) -> Value, id: u64, name: &str, args: Value| {
        let r = call(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":args}}));
        let text = r["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{r}"));
        serde_json::from_str::<Value>(text).unwrap()
    };

    call(
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
        "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"live","version":"0"}}}),
    );
    let mut out = tool(
        &mut call,
        1,
        "cursor_run",
        json!({
            "prompt": "Reply with exactly the word OK and then a final line 'STATUS: DONE'. Do not run any commands.",
            "model": "composer-2.5",
            "capability": "ask",
            "waitMs": 120000,
        }),
    );
    // A slow run detaches; follow it to the end rather than accepting RUNNING.
    if out["status"] == "RUNNING" {
        let job = out["jobId"].as_str().unwrap().to_string();
        out = tool(
            &mut call,
            2,
            "cursor_wait",
            json!({"jobId": job, "timeoutMs": 600000}),
        );
    }

    assert!(
        ["DONE", "DONE_WITH_CONCERNS"].contains(&out["status"].as_str().unwrap_or("")),
        "{out:#}"
    );
    assert_eq!(out["backend"], "cursor");
    assert!(out["text"].as_str().unwrap_or("").contains("OK"), "{out:#}");
    assert!(
        out["sessionId"].as_str().is_some_and(|s| !s.is_empty()),
        "{out:#}"
    );

    drop(stdin);
    child.wait().unwrap();
}
