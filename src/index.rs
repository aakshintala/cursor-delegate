use crate::config::{
    BUNDLED_MODELS_JSON, LoadConfigOpts, default_cli_config_path, load_cli_config, load_config,
};
use crate::job_registry::{JobRegistry, RegistryDeps, WaitOpts};
use crate::progress::{McpExtra, progress_sink_from};
use crate::runner::{AnswerError, RunnerDeps, answer_delegation, run_delegation};
use crate::tool_schemas::build_tools;
use crate::types::Config;
use crate::util::{Abort, json_pretty};
use crate::validate::validate_run_input;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
    "2024-10-07",
];
const SERVER_NAME: &str = "cursor-delegate";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct ServerDeps {
    pub config: Config,
    pub registry: Arc<JobRegistry>,
    pub cli_config: Option<crate::safety::CliConfig>,
    pub server_cwd: String,
}

struct Out(Mutex<std::io::Stdout>);

impl Out {
    fn send(&self, v: &Value) {
        let mut g = self.0.lock().expect("stdout");
        let _ = writeln!(
            g,
            "{}",
            serde_json::to_string(v).unwrap_or_else(|_| "null".into())
        );
        let _ = g.flush();
    }
}

pub fn json_content(value: &impl serde::Serialize) -> Value {
    json!({
        "content": [{ "type": "text", "text": json_pretty(value) }]
    })
}

fn require_string(args: Option<&Value>, key: &str) -> Result<String, String> {
    let v = args.and_then(|a| a.get(key));
    match v.and_then(|x| x.as_str()) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(format!("missing required field \"{key}\"")),
    }
}

fn as_string_array(v: Option<&Value>) -> Vec<String> {
    match v.and_then(|x| x.as_array()) {
        None => vec![],
        Some(arr) => arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect(),
    }
}

fn opt_timeout_ms(args: Option<&Value>) -> Result<Option<f64>, String> {
    let Some(v) = args.and_then(|a| a.get("timeoutMs")) else {
        return Ok(None);
    };
    v.as_f64()
        .filter(|n| n.is_finite())
        .map(Some)
        .ok_or_else(|| "invalid timeoutMs: must be a finite number".into())
}

fn runner_deps(deps: &ServerDeps) -> RunnerDeps {
    RunnerDeps {
        config: deps.config.clone(),
        registry: Arc::clone(&deps.registry),
        cli_config: deps.cli_config.clone(),
        server_cwd: deps.server_cwd.clone(),
        resolve_bin: None,
        capture_head: None,
    }
}

pub fn handle_call(
    name: &str,
    args: Option<&Value>,
    deps: &ServerDeps,
    opts: WaitOpts<'_>,
) -> Result<Value, String> {
    match name {
        "cursor_run" => {
            let input = validate_run_input(args.unwrap_or(&json!({})))?;
            let res = run_delegation(input, &runner_deps(deps), opts).map_err(|e| e.to_string())?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        "cursor_poll" => {
            let id = require_string(args, "jobId")?;
            serde_json::to_value(deps.registry.poll(&id)).map_err(|e| e.to_string())
        }
        "cursor_cancel" => {
            let id = require_string(args, "jobId")?;
            serde_json::to_value(deps.registry.cancel(&id)).map_err(|e| e.to_string())
        }
        "cursor_wait" => {
            let id = require_string(args, "jobId")?;
            let timeout = opt_timeout_ms(args)?;
            serde_json::to_value(deps.registry.wait(&id, timeout, opts)).map_err(|e| e.to_string())
        }
        "cursor_wait_any" => {
            let ids = as_string_array(args.and_then(|a| a.get("jobIds")));
            let timeout = opt_timeout_ms(args)?;
            serde_json::to_value(deps.registry.wait_any(&ids, timeout, opts))
                .map_err(|e| e.to_string())
        }
        "cursor_wait_all" => {
            let ids = as_string_array(args.and_then(|a| a.get("jobIds")));
            let timeout = opt_timeout_ms(args)?;
            serde_json::to_value(deps.registry.wait_all(&ids, timeout, opts))
                .map_err(|e| e.to_string())
        }
        "cursor_answer" => {
            let job_id = require_string(args, "jobId")?;
            let answer = require_string(args, "answer")?;
            match answer_delegation(&job_id, &answer, &runner_deps(deps), opts) {
                Ok(v) => serde_json::to_value(v).map_err(|e| e.to_string()),
                Err(e) => {
                    if e.downcast_ref::<AnswerError>()
                        .is_some_and(|a| matches!(a, AnswerError::NotFound))
                    {
                        Ok(json!({ "status": "NOT_FOUND" }))
                    } else {
                        Err(e.to_string())
                    }
                }
            }
        }
        "doctor" => {
            let report = crate::doctor::run_doctor(crate::doctor::RunDoctorOpts {
                config: &deps.config,
                resolve_bin: None,
                bin_exists: None,
                run_command: None,
                read_package_version: None,
                check_plugin_registration: None,
            });
            serde_json::to_value(report).map_err(|e| e.to_string())
        }
        other => Err(format!("unknown tool \"{other}\"")),
    }
}

fn negotiate_protocol(client: Option<&str>) -> &'static str {
    if let Some(v) = client
        && SUPPORTED_PROTOCOL_VERSIONS.contains(&v)
    {
        // Echo the same static slice when it matches.
        for s in SUPPORTED_PROTOCOL_VERSIONS {
            if *s == v {
                return s;
            }
        }
    }
    LATEST_PROTOCOL_VERSION
}

fn rpc_error(id: Option<&Value>, code: i64, message: &str) -> Value {
    let mut o = serde_json::Map::new();
    o.insert("jsonrpc".into(), json!("2.0"));
    o.insert("error".into(), json!({ "code": code, "message": message }));
    if let Some(id) = id {
        o.insert("id".into(), id.clone());
    } else {
        o.insert("id".into(), Value::Null);
    }
    Value::Object(o)
}

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn initialize_result(params: Option<&Value>) -> Value {
    let client_ver = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(|v| v.as_str());
    json!({
        "protocolVersion": negotiate_protocol(client_ver),
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
    })
}

fn tools_list_result(config: &Config) -> Value {
    let tools: Vec<Value> = build_tools(config)
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        })
        .collect();
    json!({ "tools": tools })
}

struct Session {
    deps: Arc<ServerDeps>,
    out: Arc<Out>,
    inflight: Arc<Mutex<HashMap<String, Abort>>>,
}

fn id_key(id: &Value) -> String {
    id.to_string()
}

fn handle_request(sess: &Session, msg: &Value) {
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = msg.get("id");
    let params = msg.get("params");

    match method {
        "initialize" => {
            if let Some(id) = id {
                sess.out.send(&rpc_result(id, initialize_result(params)));
            }
        }
        "notifications/initialized" => {}
        "ping" => {
            if let Some(id) = id {
                sess.out.send(&rpc_result(id, json!({})));
            }
        }
        "tools/list" => {
            if let Some(id) = id {
                sess.out
                    .send(&rpc_result(id, tools_list_result(&sess.deps.config)));
            }
        }
        "notifications/cancelled" => {
            if let Some(rid) = params.and_then(|p| p.get("requestId"))
                && let Some(flag) = sess.inflight.lock().unwrap().get(&id_key(rid))
            {
                flag.store(true, Ordering::SeqCst);
                sess.deps.registry.wake();
            }
        }
        "tools/call" => {
            let Some(id) = id.cloned() else {
                return;
            };
            let name = params
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let args = params.and_then(|p| p.get("arguments")).cloned();
            let progress_token = params
                .and_then(|p| p.get("_meta"))
                .and_then(|m| m.get("progressToken"))
                .cloned();
            let out = Arc::clone(&sess.out);
            let flag: Abort = Arc::new(AtomicBool::new(false));
            sess.inflight
                .lock()
                .unwrap()
                .insert(id_key(&id), Arc::clone(&flag));
            let extra = McpExtra {
                progress_token,
                send_notification: Some({
                    let out = Arc::clone(&out);
                    Arc::new(move |mut n: Value| {
                        if let Some(obj) = n.as_object_mut() {
                            obj.insert("jsonrpc".into(), json!("2.0"));
                        }
                        out.send(&n);
                    })
                }),
            };
            let sink = progress_sink_from(Some(&extra), || {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0)
            });
            let inflight = Arc::clone(&sess.inflight);
            let deps = Arc::clone(&sess.deps);
            // One thread per call, so a blocking cursor_wait never stalls other requests.
            std::thread::spawn(move || {
                let opts = WaitOpts {
                    sink: sink.as_ref(),
                    abort: Some(&flag),
                };
                let reply = match handle_call(&name, args.as_ref(), &deps, opts) {
                    Ok(v) => rpc_result(&id, json_content(&v)),
                    Err(e) => rpc_error(Some(&id), -32603, &e),
                };
                inflight.lock().unwrap().remove(&id_key(&id));
                // A cancelled request gets no reply: the client has already dropped its id.
                if !flag.load(Ordering::SeqCst) {
                    out.send(&reply);
                }
            });
        }
        "" => {
            if let Some(id) = id {
                sess.out
                    .send(&rpc_error(Some(id), -32600, "Invalid Request"));
            }
        }
        other => {
            if let Some(id) = id {
                sess.out.send(&rpc_error(
                    Some(id),
                    -32601,
                    &format!("Method not found: {other}"),
                ));
            }
        }
    }
}

pub fn build_deps() -> Result<ServerDeps, Box<dyn std::error::Error + Send + Sync>> {
    let bundled = BUNDLED_MODELS_JSON.to_string();
    let config = load_config(LoadConfigOpts {
        models_path: "<bundled>/models.json".into(),
        host_profile_path: None,
        read_file: Some(Box::new(move |path: &str| {
            if path == "<bundled>/models.json" {
                Ok(bundled.clone())
            } else {
                std::fs::read_to_string(path).map_err(|e| crate::config::IoErr {
                    code: if e.kind() == std::io::ErrorKind::NotFound {
                        Some("ENOENT".into())
                    } else {
                        None
                    },
                    message: e.to_string(),
                })
            }
        })),
    })?;
    let cli_config = load_cli_config(&default_cli_config_path(), None)?;
    let idle_ms = match config.profile.idle_ms {
        None => Some(300_000.0),
        Some(inner) => inner,
    };
    let tool_idle_ms = match config.profile.tool_idle_ms {
        None => Some(1_800_000.0),
        Some(inner) => inner,
    };
    let registry = JobRegistry::new(RegistryDeps::new(
        Arc::new(crate::backends::cursor::make_cursor_adapter()),
        config.profile.deadline_ms.unwrap_or(60_000.0),
        idle_ms,
        tool_idle_ms,
    ));
    Ok(ServerDeps {
        config,
        registry,
        cli_config,
        server_cwd: std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".into()),
    })
}

static SHUTDOWN_PIPE_W: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

extern "C" fn on_shutdown_signal(_: libc::c_int) {
    // Async-signal-safe: just wake the shutdown thread.
    let b = 0u8;
    unsafe {
        libc::write(
            SHUTDOWN_PIPE_W.load(Ordering::Relaxed),
            (&b as *const u8).cast(),
            1,
        )
    };
}

/// SIGTERM/SIGINT kill every child, then exit. Uses a handler + self-pipe rather than
/// blocking the signals for sigwait: a blocked mask is inherited by every child we spawn
/// (std's Command doesn't reset it), which would make the children unkillable by SIGTERM.
fn install_shutdown(registry: Arc<JobRegistry>) {
    let mut fds = [0 as libc::c_int; 2];
    unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        for fd in fds {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
    SHUTDOWN_PIPE_W.store(fds[1], Ordering::Relaxed);
    std::thread::spawn(move || {
        let mut b = 0u8;
        unsafe { libc::read(fds[0], (&mut b as *mut u8).cast(), 1) };
        registry.kill_all();
        unsafe { libc::_exit(0) };
    });
    let handler = on_shutdown_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
}

pub fn serve_stdio(deps: ServerDeps) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    install_shutdown(Arc::clone(&deps.registry));

    let sess = Session {
        deps: Arc::new(deps),
        out: Arc::new(Out(Mutex::new(std::io::stdout()))),
        inflight: Arc::new(Mutex::new(HashMap::new())),
    };
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle_request(&sess, &msg),
            Err(_) => {
                sess.out.send(&rpc_error(None, -32700, "Parse error"));
            }
        }
    }
    sess.deps.registry.kill_all();
    Ok(())
}

pub fn main() {
    match build_deps() {
        Ok(deps) => {
            if let Err(e) = serve_stdio(deps) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests;
