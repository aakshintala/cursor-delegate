//! The `delegate` CLI: `run` starts a detached supervisor, `watch` reads job records.
//! The record file under `$TMPDIR/delegate-jobs/` is the only link between them.

use crate::backends::cursor::make_cursor_adapter;
use crate::capability::map_capability;
use crate::git::capture_head;
use crate::index::build_deps;
use crate::job_registry::{JobRegistry, RegistryDeps, WaitOpts};
use crate::models::resolve_model;
use crate::prompt::status_block;
use crate::runner::build_argv;
use crate::status_record::{CliRecordWriter, cli_record_path};
use crate::types::{Capability, JobSpec, ResumeContext};
use crate::util::{random_uuid, resolve_path};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: delegate run --model M [--capability read-only|read-write] [--cwd D] [--prompt-file F]\n       delegate watch <jobId>... [--timeout S]";

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
    let (kv, _) = parse(args, &["--model", "--capability", "--cwd", "--prompt-file"])?;
    let Some(model) = flag(&kv, "--model") else {
        return usage("--model is required");
    };
    let cap_name = flag(&kv, "--capability").unwrap_or("read-only");
    capability(cap_name)?;
    let deps = match build_deps() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return Ok(1);
        }
    };
    if resolve_model(Some(model), false, &deps.config).is_err() {
        let mut ids: Vec<_> = deps.config.models.keys().cloned().collect();
        ids.sort();
        return usage(format!(
            "unknown model {model}; valid models: {}",
            ids.join(", ")
        ));
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

    // Own session and process group, stdio closed: the supervisor outlives this process.
    let mut child = unsafe {
        Command::new(std::env::current_exe().map_err(|e| Usage(e.to_string()))?)
            .args(["__supervise", &id, model, cap_name, &cwd])
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
    let [id, model, cap_name, cwd] = args else {
        return 2;
    };
    let fail = |m: String| {
        eprintln!("{m}");
        1
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
        gate: String::new(),
        allow_partial_commit: false,
        wait_ms: None,
        idle_ms: None,
        tool_idle_ms: None,
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
            gate: String::new(),
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
    rd.status_writer = Arc::new(CliRecordWriter {
        job_id: id.clone(),
        model: model.clone(),
        cwd: cwd.clone(),
        capability: cap_label,
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
