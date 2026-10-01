//! The `delegate` CLI: `run` starts a detached supervisor, `watch` reads job records.
//! The record file under `$TMPDIR/delegate-jobs/` is the only link between them.

use crate::backends::cursor::make_cursor_adapter;
use crate::capability::map_capability;
use crate::git::capture_head;
use crate::index::build_deps;
use crate::job_registry::{JobRegistry, RegistryDeps, WaitOpts};
use crate::lock::{self, AcquireError};
use crate::models::resolve_model;
use crate::prompt::status_block;
use crate::runner::build_argv;
use crate::status_record::{CliRecordWriter, cli_record_path, write_supervisor_died};
use crate::types::{Capability, JobSpec, ResumeContext};
use crate::util::{random_uuid, resolve_path};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: delegate run --model M [--capability read-only|read-write] [--cwd D] [--gate CMD] [--tool-idle-ms N] [--prompt-file F]
       delegate watch <jobId>... [--timeout S]
       delegate models
       delegate doctor";

/// Bad input: reason on stderr, exit 2.
struct Usage(String);

fn usage<T>(msg: impl Into<String>) -> Result<T, Usage> {
    Err(Usage(msg.into()))
}

pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        Some("watch") => watch(&args[1..]),
        Some("models") => crate::cli_info::models().map_err(Usage),
        Some("doctor") => crate::cli_info::doctor().map_err(Usage),
        Some("__supervise") => return supervise(&args[1..]),
        _ => usage(USAGE),
    };
    match res {
        Ok(code) => code,
        Err(Usage(m)) => {
            eprintln!("{m}");
            2
        }
    }
}

/// Splits `--flag value` pairs from positionals.
fn parse(args: &[String], flags: &[&str]) -> Result<(Vec<(String, String)>, Vec<String>), Usage> {
    let (mut kv, mut pos) = (vec![], vec![]);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a.starts_with("--") {
            if !flags.contains(&a.as_str()) {
                return usage(format!("unknown flag {a}\n{USAGE}"));
            }
            let Some(v) = it.next() else {
                return usage(format!("{a} needs a value"));
            };
            kv.push((a.clone(), v.clone()));
        } else {
            pos.push(a.clone());
        }
    }
    Ok((kv, pos))
}

fn flag<'a>(kv: &'a [(String, String)], name: &str) -> Option<&'a str> {
    kv.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn capability(name: &str) -> Result<(Capability, &'static str), Usage> {
    match name {
        "read-only" => Ok((Capability::Ask, "read-only")),
        "read-write" => Ok((Capability::WriteUnsandboxed, "read-write")),
        o => usage(format!(
            "unknown capability {o}: use read-only or read-write"
        )),
    }
}

fn run(args: &[String]) -> Result<i32, Usage> {
    let (kv, _) = parse(
        args,
        &[
            "--model",
            "--capability",
            "--cwd",
            "--gate",
            "--tool-idle-ms",
            "--prompt-file",
        ],
    )?;
    let Some(model) = flag(&kv, "--model") else {
        return usage("--model is required");
    };
    let cap_name = flag(&kv, "--capability").unwrap_or("read-only");
    capability(cap_name)?;
    let gate = flag(&kv, "--gate").unwrap_or("").to_string();
    let tool_idle_ms = match flag(&kv, "--tool-idle-ms") {
        None => None,
        Some(s) => match s.parse::<f64>() {
            Ok(n) if n.is_finite() && n > 0.0 => Some(n),
            _ => return usage(format!("invalid --tool-idle-ms {s}")),
        },
    };
    let deps = match build_deps() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return Ok(1);
        }
    };
    if let Err(e) = resolve_model(Some(model), false, &deps.config) {
        // An unknown id lists the valid ids; any other failure (e.g. a model on
        // a backend that is not implemented yet) prints the actual error.
        if e.downcast_ref::<crate::models::ModelNotAllowedError>()
            .is_some()
        {
            let mut ids: Vec<_> = deps.config.models.keys().cloned().collect();
            ids.sort();
            return usage(format!(
                "unknown model {model}; valid models: {}",
                ids.join(", ")
            ));
        }
        return usage(format!("{e}"));
    }
    let mut prompt = String::new();
    match flag(&kv, "--prompt-file") {
        Some(f) => prompt = std::fs::read_to_string(f).map_err(|e| Usage(format!("{f}: {e}")))?,
        None => {
            let _ = std::io::stdin().read_to_string(&mut prompt);
        }
    }
    if prompt.trim().is_empty() {
        return usage("prompt is empty");
    }
    let cwd = match flag(&kv, "--cwd") {
        Some(d) => resolve_path(d),
        None => std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .map_err(|e| Usage(e.to_string()))?,
    };
    if !std::path::Path::new(&cwd).is_dir() {
        return usage(format!("cwd {cwd} is not a directory"));
    }

    let id = random_uuid();
    // Read-only jobs take no lock. A read-write job locks before the supervisor exists, so a
    // second writer in this cwd is refused even if the first supervisor has not started.
    let held = if cap_name == "read-write" {
        match lock::try_acquire(&cwd, &id) {
            Ok(lock) => Some(lock),
            Err(AcquireError::Busy { holder }) => {
                eprintln!("BUSY {holder}");
                return Ok(3);
            }
            Err(AcquireError::Io(e)) => {
                eprintln!("cannot lock {cwd}: {e}");
                return Ok(1);
            }
        }
    } else {
        None
    };
    let record = cli_record_path(&id);
    let dir = record.parent().expect("record has a parent");
    std::fs::create_dir_all(dir).map_err(|e| Usage(format!("{}: {e}", dir.display())))?;
    let prompt_file = dir.join(format!("{id}.prompt"));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&prompt_file)
        .and_then(|mut f| f.write_all(prompt.as_bytes()));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&prompt_file);
        return usage(format!("{}: {e}", prompt_file.display()));
    }

    let mut supervise_args = vec![
        "__supervise".to_string(),
        id.clone(),
        model.to_string(),
        cap_name.to_string(),
        cwd.clone(),
    ];
    if !gate.is_empty() {
        supervise_args.push("--gate".into());
        supervise_args.push(gate);
    }
    if let Some(ms) = tool_idle_ms {
        supervise_args.push("--tool-idle-ms".into());
        // Shortest round-trip so the supervisor parses the same number back.
        supervise_args.push(ms.to_string());
    }
    if let Some(lock) = &held {
        supervise_args.push("--lock-fd".into());
        supervise_args.push(lock.fd().to_string());
    }
    // Own session and process group, stdio closed: the supervisor outlives this process.
    // `held` stays open across the spawn so the inherited fd remains locked.
    let mut child = unsafe {
        Command::new(std::env::current_exe().map_err(|e| Usage(e.to_string()))?)
            .args(&supervise_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .pre_exec(|| {
                libc::setsid();
                Ok(())
            })
            .spawn()
    }
    .map_err(|e| {
        let _ = std::fs::remove_file(&prompt_file);
        Usage(format!("cannot start supervisor: {e}"))
    })?;
    // The supervisor writes the first RUNNING record before the agent starts; wait for it.
    let t = Instant::now();
    while !record.exists() {
        if child.try_wait().ok().flatten().is_some() || t.elapsed() > Duration::from_secs(10) {
            let _ = std::fs::remove_file(&prompt_file);
            eprintln!("supervisor failed to write {}", record.display());
            return Ok(1);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("{id}");
    Ok(0)
}

/// The detached half of `run`: drives one job, keeping its record fresh until it ends.
fn supervise(args: &[String]) -> i32 {
    let fail = |m: String| {
        eprintln!("{m}");
        1
    };
    let (kv, pos) = match parse(args, &["--gate", "--tool-idle-ms", "--lock-fd"]) {
        Ok(v) => v,
        Err(Usage(m)) => return fail(m),
    };
    let [id, model, cap_name, cwd] = pos.as_slice() else {
        return 2;
    };
    // Hold the inherited lock until this process ends. CLOEXEC stops the agent and the gate
    // from keeping it after we die.
    let _lock = match flag(&kv, "--lock-fd") {
        None => None,
        Some(s) => {
            let Ok(fd) = s.parse::<i32>() else {
                return 2;
            };
            Some(unsafe { lock::adopt(fd) })
        }
    };
    let gate = flag(&kv, "--gate").unwrap_or("").to_string();
    let tool_idle_ms = match flag(&kv, "--tool-idle-ms") {
        None => None,
        Some(s) => match s.parse::<f64>() {
            Ok(n) if n.is_finite() && n > 0.0 => Some(n),
            _ => return 2,
        },
    };
    let (cap, cap_label) = match capability(cap_name) {
        Ok(c) => c,
        Err(Usage(m)) => return fail(m),
    };
    let deps = match build_deps() {
        Ok(d) => d,
        Err(e) => return fail(e.to_string()),
    };
    let config = deps.config;
    let prompt_file = cli_record_path(id).with_extension("prompt");
    let Ok(prompt) = std::fs::read_to_string(&prompt_file) else {
        return fail(format!("cannot read {}", prompt_file.display()));
    };
    let _ = std::fs::remove_file(&prompt_file);
    let prompt = format!("{prompt}\n\n---\n\n{}", status_block()).replace('\0', "");

    let capres = map_capability(cap, true);
    let argv = build_argv(model, &capres.flags, &[], None, &prompt);
    let spec = JobSpec {
        bin: crate::cursor_bin::resolve_cursor_bin(None),
        argv,
        cwd: cwd.clone(),
        model: model.clone(),
        backend: "cursor".into(),
        is_write: capres.is_write,
        path: Some(resolve_path(cwd)),
        head_before: capture_head(cwd, None),
        gate: gate.clone(),
        allow_partial_commit: false,
        wait_ms: None,
        idle_ms: None,
        tool_idle_ms: tool_idle_ms.map(Some),
        background: Some(true),
        price_map: config.price_map.clone(),
        downgraded: false,
        worktree_name: None,
        resume_context: ResumeContext {
            model: model.clone(),
            require_non_claude: None,
            capability: cap,
            allow_unsandboxed: true,
            isolation: crate::types::Isolation::None,
            verify_commands: None,
            gate: gate.clone(),
            allow_partial_commit: false,
        },
    };

    let idle = |v: Option<Option<f64>>, d| match v {
        None => Some(d),
        Some(inner) => inner,
    };
    let mut rd = RegistryDeps::new(
        Arc::new(make_cursor_adapter()),
        config.profile.deadline_ms.unwrap_or(60_000.0),
        idle(config.profile.idle_ms, 300_000.0),
        idle(config.profile.tool_idle_ms, 1_800_000.0),
    );
    if let Some(ms) = std::env::var("DELEGATE_HEARTBEAT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        rd.heartbeat_ms = ms;
    }
    // `DELEGATE_IDLE_MS`: test-only override of the model-idle window.
    if let Some(ms) = std::env::var("DELEGATE_IDLE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        rd.idle_ms = Some(ms);
    }
    rd.status_writer = Arc::new(CliRecordWriter {
        job_id: id.clone(),
        model: model.clone(),
        cwd: cwd.clone(),
        capability: cap_label,
        gate: gate.clone(),
        tool_idle_ms,
    });
    let registry = JobRegistry::new(rd);
    let Some(job) = registry
        .dispatch(spec, WaitOpts::default())
        .job_id()
        .map(str::to_string)
    else {
        return fail("another write job holds this cwd".into());
    };
    while registry.wait(&job, None, WaitOpts::default()) == "RUNNING" {}
    0
}

fn read_record(id: &str) -> Option<String> {
    std::fs::read_to_string(cli_record_path(id)).ok()
}

fn watch(args: &[String]) -> Result<i32, Usage> {
    let (kv, ids) = parse(args, &["--timeout"])?;
    if ids.is_empty() {
        return usage(USAGE);
    }
    let timeout = match flag(&kv, "--timeout") {
        None => None,
        Some(s) => match s.parse::<f64>() {
            Ok(t) if t >= 0.0 => Some(Duration::from_secs_f64(t)),
            _ => return usage(format!("invalid --timeout {s}")),
        },
    };
    // An id is a file stem; a separator would escape the jobs directory.
    if let Some(id) = ids
        .iter()
        .find(|i| i.contains('/') || read_record(i).is_none())
    {
        return usage(format!("unknown job {id}"));
    }
    let start = Instant::now();
    loop {
        for id in &ids {
            settle_dead_supervisor(id);
        }
        // A read can land on no file only if the record was deleted under us; treat as running.
        let recs: Vec<Option<serde_json::Value>> = ids
            .iter()
            .map(|i| read_record(i).and_then(|s| serde_json::from_str(&s).ok()))
            .collect();
        let all_done = recs
            .iter()
            .all(|r| r.as_ref().is_some_and(|v| v["status"] != "RUNNING"));
        let timed_out = timeout.is_some_and(|t| start.elapsed() >= t);
        if all_done || timed_out {
            for r in recs.iter().flatten() {
                println!("{r}");
            }
            return Ok(if all_done { 0 } else { 1 });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `kill(pid, 0)` fails with `ESRCH` only when the process is gone. `EPERM` means it exists.
fn process_missing(pid: i32) -> bool {
    let rc = unsafe { libc::kill(pid, 0) };
    rc != 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// A RUNNING record whose supervisor has died is terminal: rewrite it once and let the
/// caller print the ERROR record. Re-read after the liveness check so a supervisor that
/// finished and exited in between is not overwritten.
///
/// ponytail: pid-only liveness. A reused pid hides a dead supervisor; add a heartbeat-age check if that shows up.
fn settle_dead_supervisor(id: &str) {
    let Some(v) = read_record(id).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    else {
        return;
    };
    if v["status"] != "RUNNING" {
        return;
    }
    let Some(pid) = v["supervisorPid"].as_i64() else {
        return;
    };
    if pid <= 0 || pid > i32::MAX as i64 || !process_missing(pid as i32) {
        return;
    }
    let Some(v) = read_record(id).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    else {
        return;
    };
    if v["status"] != "RUNNING" {
        return;
    }
    if let Err(e) = write_supervisor_died(id, &v) {
        eprintln!(
            "cannot write status record {}: {e}",
            cli_record_path(id).display()
        );
    }
}
