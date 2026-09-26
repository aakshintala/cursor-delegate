//! Drives the real binary over stdio with a fake cursor-agent, the way an MCP client does.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const FAKE_AGENT: &str = r#"#!/bin/sh
printf '%s\n' '{"type":"tool_call","subtype":"started","tool_call":{"shellToolCall":{"args":{"command":"ls"}}}}'
case "$*" in *HANG*) sleep 300 ;; esac
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"hi\nSTATUS: DONE","session_id":"s-1"}'
"#;

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Server {
    fn start(dir: &std::path::Path) -> Self {
        let agent = dir.join("agent.sh");
        std::fs::write(&agent, FAKE_AGENT).unwrap();
        Command::new("chmod")
            .arg("+x")
            .arg(&agent)
            .status()
            .unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_cursor-delegate-mcp"))
            .current_dir(dir)
            .env("CURSOR_AGENT_BIN", &agent)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut s = Server {
            child,
            stdin,
            stdout,
        };
        s.call(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}));
        s
    }

    /// Send a request and return its reply, skipping notifications.
    fn call(&mut self, req: Value) -> Value {
        writeln!(self.stdin, "{req}").unwrap();
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            if v.get("id") == req.get("id") {
                return v;
            }
        }
    }

    fn tool(&mut self, id: u64, name: &str, args: Value) -> Value {
        let r = self.call(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":args}}));
        serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cdm-e2e-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

fn agent_pids(dir: &std::path::Path) -> Vec<i32> {
    let out = Command::new("pgrep")
        .arg("-f")
        .arg(dir.join("agent.sh"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect()
}

#[test]
fn run_completes_and_cancel_kills_a_hung_agent() {
    let dir = scratch("cancel");
    let mut s = Server::start(&dir);

    let done = s.tool(1, "cursor_run", json!({"prompt":"hi"}));
    assert_eq!(done["status"], "DONE");
    assert_eq!(done["sessionId"], "s-1");

    let bg = s.tool(2, "cursor_run", json!({"prompt":"HANG","background":true}));
    let job = bg["jobId"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(300));
    let pids = agent_pids(&dir);
    assert!(!pids.is_empty());

    let t = Instant::now();
    let cancelled = s.tool(3, "cursor_cancel", json!({"jobId": job}));
    assert_eq!(cancelled["status"], "CANCELLED");
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "cancel took {:?}",
        t.elapsed()
    );
    assert!(pids.iter().all(|p| !alive(*p)), "agent survived SIGTERM");

    drop(s.stdin);
    s.child.wait().unwrap();
}

#[test]
fn cancelled_request_gets_no_reply() {
    let dir = scratch("cancel-reply");
    let mut s = Server::start(&dir);
    let bg = s.tool(1, "cursor_run", json!({"prompt":"HANG","background":true}));
    let job = bg["jobId"].as_str().unwrap();

    writeln!(
        s.stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"cursor_wait","arguments":{"jobId":job,"timeoutMs":60000}}})
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    writeln!(
        s.stdin,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/cancelled",
        "params":{"requestId":2}})
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));

    // The ping reply comes back; nothing for id 2 precedes it.
    writeln!(
        s.stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":3,"method":"ping"})
    )
    .unwrap();
    loop {
        let mut line = String::new();
        s.stdout.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_ne!(v["id"], 2, "reply sent for a cancelled request: {v}");
        if v["id"] == 3 {
            break;
        }
    }
    s.tool(4, "cursor_cancel", json!({"jobId": job}));
    drop(s.stdin);
    s.child.wait().unwrap();
}

#[test]
fn sigterm_to_the_server_kills_running_agents() {
    let dir = scratch("shutdown");
    let mut s = Server::start(&dir);
    s.tool(1, "cursor_run", json!({"prompt":"HANG","background":true}));
    std::thread::sleep(Duration::from_millis(300));
    let pids = agent_pids(&dir);
    assert!(!pids.is_empty());

    unsafe { libc::kill(s.child.id() as i32, libc::SIGTERM) };
    s.child.wait().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        pids.iter().all(|p| !alive(*p)),
        "agent survived server shutdown"
    );
}
