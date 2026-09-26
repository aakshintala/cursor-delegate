//! Ports of tests/job-registry.test.ts. The TS suite drives a fake clock; these run on real
//! time with short windows, so each timing assertion leaves a wide margin either side.

use super::*;
use crate::backends::types::Spawned;
use crate::output::derive_status;
use crate::status_record::{FileStatusRecordWriter, status_record_path};
use crate::types::{Capability, Isolation, RawCursorJson};
use std::sync::mpsc;
use std::thread::sleep;

pub(crate) fn spec_of(over: impl FnOnce(&mut JobSpec)) -> JobSpec {
    let mut s = JobSpec {
        bin: "cursor-agent".into(),
        argv: vec!["--print".into()],
        cwd: "/tmp".into(),
        model: "composer-2.5".into(),
        backend: "cursor".into(),
        is_write: false,
        path: None,
        head_before: None,
        gate: String::new(),
        allow_partial_commit: false,
        wait_ms: None,
        idle_ms: None,
        tool_idle_ms: None,
        background: None,
        price_map: HashMap::new(),
        downgraded: false,
        worktree_name: None,
        resume_context: ResumeContext {
            model: "composer-2.5".into(),
            require_non_claude: None,
            capability: Capability::Ask,
            allow_unsandboxed: false,
            isolation: Isolation::None,
            verify_commands: None,
            gate: String::new(),
            allow_partial_commit: false,
        },
    };
    over(&mut s);
    s
}

pub(crate) fn bg(s: &mut JobSpec) {
    s.background = Some(true);
}

enum Msg {
    Ev(Event),
    Finish(BackendResult),
}

/// One run of the fake backend, driven by the test.
pub(crate) struct FakeHandle {
    pub spec: JobSpec,
    tx: Mutex<mpsc::Sender<Msg>>,
    pub killed: Arc<Mutex<Vec<&'static str>>>,
}

impl FakeHandle {
    pub fn finish(&self, r: BackendResult) {
        let _ = self.tx.lock().unwrap().send(Msg::Finish(r));
    }
    pub fn progress(
        &self,
        tool: &str,
        tokens: f64,
        assistant: Option<&str>,
        files: &[&str],
        phase: Option<&str>,
    ) {
        let snap = ProgressSnapshotRaw {
            last_tool: Some(tool.into()),
            tokens_so_far: tokens,
            last_assistant: assistant.map(Into::into),
            files_touched: files.iter().map(|f| f.to_string()).collect(),
            phase: phase.map(Into::into),
        };
        let _ = self.tx.lock().unwrap().send(Msg::Ev(Event::Progress(snap)));
    }
    pub fn activity(&self) {
        let _ = self.tx.lock().unwrap().send(Msg::Ev(Event::Activity));
    }
    pub fn killed(&self) -> Vec<&'static str> {
        self.killed.lock().unwrap().clone()
    }
}

/// Controllable fake backend. Each `run` pushes a handle the test drives; `kill` records
/// SIGTERM and makes the child exit uncleanly, like the TS fake.
#[derive(Default)]
pub(crate) struct FakeBackend {
    handles: Mutex<Vec<Arc<FakeHandle>>>,
    /// When set, every run finishes immediately with this result.
    pub auto: Mutex<Option<BackendResult>>,
}

impl FakeBackend {
    pub fn handle(&self, i: usize) -> Arc<FakeHandle> {
        for _ in 0..500 {
            if let Some(h) = self.handles.lock().unwrap().get(i) {
                return Arc::clone(h);
            }
            sleep(Duration::from_millis(2));
        }
        panic!("no handle {i}");
    }
    pub fn count(&self) -> usize {
        self.handles.lock().unwrap().len()
    }
    pub fn last_spec(&self) -> JobSpec {
        self.handles
            .lock()
            .unwrap()
            .last()
            .expect("a run")
            .spec
            .clone()
    }
}

impl Backend for FakeBackend {
    fn run(&self, spec: &JobSpec) -> Spawned {
        let (tx, rx) = mpsc::channel();
        if let Some(r) = &*self.auto.lock().unwrap() {
            tx.send(Msg::Finish(r.clone())).unwrap();
        }
        let killed = Arc::new(Mutex::new(Vec::new()));
        let h = Arc::new(FakeHandle {
            spec: spec.clone(),
            tx: Mutex::new(tx.clone()),
            killed: Arc::clone(&killed),
        });
        self.handles.lock().unwrap().push(h);
        let tx = Mutex::new(tx);
        Spawned {
            kill: Box::new(move || {
                killed.lock().unwrap().push("SIGTERM");
                let _ = tx
                    .lock()
                    .unwrap()
                    .send(Msg::Finish(BackendResult::default()));
            }),
            drive: Box::new(move |on| {
                loop {
                    match rx.recv() {
                        Ok(Msg::Ev(e)) => on(e),
                        Ok(Msg::Finish(r)) => return r,
                        Err(_) => return BackendResult::default(),
                    }
                }
            }),
        }
    }
}

/// Builds a RunOutput from the raw result without touching git/gate.
pub(crate) fn fake_finalize(res: &BackendResult, ctx: &FinalizeCtx) -> RunOutput {
    let text = res.raw.result.clone().unwrap_or_default();
    RunOutput {
        status: derive_status(&text, res.raw.is_error, res.clean_exit),
        text,
        session_id: res.raw.session_id.clone(),
        backend: ctx.backend.clone(),
        model: ctx.model.clone(),
        usage: res.raw.usage.clone(),
        cost_usd: None,
        cost_estimated: true,
        duration_ms: res.raw.duration_ms,
        job_id: ctx.job_id.clone(),
        downgraded: None,
        stderr_tail: None,
        gate_result: None,
        change_set: None,
        concerns: None,
    }
}

pub(crate) fn result_of(text: &str, session: Option<&str>) -> BackendResult {
    BackendResult {
        raw: RawCursorJson {
            result: Some(text.into()),
            is_error: Some(false),
            session_id: session.map(Into::into),
            ..Default::default()
        },
        clean_exit: true,
        stderr: String::new(),
    }
}

pub(crate) fn done_ok() -> BackendResult {
    result_of("ok\nSTATUS: DONE", None)
}

#[derive(Default)]
struct Spy(Mutex<Vec<(String, serde_json::Value)>>);

impl StatusRecordWriter for Spy {
    fn write(&self, job_id: &str, record: &PollResult) {
        self.0
            .lock()
            .unwrap()
            .push((job_id.into(), serde_json::to_value(record).unwrap()));
    }
}

impl Spy {
    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    fn nth(&self, i: usize) -> (String, serde_json::Value) {
        self.0.lock().unwrap()[i].clone()
    }
    fn last(&self) -> (String, serde_json::Value) {
        self.0.lock().unwrap().last().unwrap().clone()
    }
}

struct Setup {
    reg: Arc<JobRegistry>,
    fake: Arc<FakeBackend>,
    spy: Arc<Spy>,
}

fn setup_with(f: impl FnOnce(&mut RegistryDeps)) -> Setup {
    let fake = Arc::new(FakeBackend::default());
    let spy = Arc::new(Spy::default());
    let mut deps = RegistryDeps::new(fake.clone(), 200.0, None, None);
    deps.finalize = Arc::new(fake_finalize);
    deps.finalize_stall = Arc::new(fake_finalize);
    deps.status_writer = spy.clone();
    f(&mut deps);
    Setup {
        reg: JobRegistry::new(deps),
        fake,
        spy,
    }
}

fn setup() -> Setup {
    setup_with(|_| {})
}

fn id_of(r: &DispatchResult) -> String {
    r.job_id().expect("job id").to_string()
}

/// Poll until `id` leaves RUNNING (or give up), returning the final status label.
pub(crate) fn settle(reg: &JobRegistry, id: &str) -> String {
    for _ in 0..500 {
        let p = reg.poll(id);
        if p.status_label() != "RUNNING" {
            return p.status_label().to_string();
        }
        sleep(Duration::from_millis(2));
    }
    "RUNNING".into()
}

fn status_of(v: &serde_json::Value) -> &str {
    v["status"].as_str().unwrap()
}

#[test]
fn heartbeat_refreshes_running_record_and_stops_at_retirement() {
    let s = setup_with(|d| d.heartbeat_ms = 100);
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    assert_eq!(s.spy.len(), 1);

    sleep(Duration::from_millis(150));
    assert_eq!(s.spy.len(), 2);
    let (jid, rec) = s.spy.nth(1);
    assert_eq!(jid, id);
    assert_eq!(status_of(&rec), "RUNNING");
    assert!(
        rec["lastHeartbeatAt"].is_u64(),
        "integral ms, like Date.now()"
    );

    // Re-arms while RUNNING...
    sleep(Duration::from_millis(100));
    assert_eq!(s.spy.len(), 3);

    // ...and stops once terminal.
    s.fake.handle(0).finish(done_ok());
    assert_eq!(settle(&s.reg, &id), "DONE");
    let after = s.spy.len();
    assert_eq!(status_of(&s.spy.last().1), "DONE");
    sleep(Duration::from_millis(300));
    assert_eq!(s.spy.len(), after);
}

#[test]
fn mark_superseded_writes_a_forwarding_pointer() {
    let s = setup();
    let old = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    s.fake
        .handle(0)
        .finish(result_of("what port?\nSTATUS: NEEDS_CONTEXT", None));
    assert_eq!(settle(&s.reg, &old), "NEEDS_CONTEXT");

    let res = s.reg.mark_superseded(&old, "new-id");
    let v = serde_json::to_value(&res).unwrap();
    assert_eq!(v["status"], "NEEDS_CONTEXT");
    assert_eq!(v["supersededBy"], "new-id");
    let (jid, rec) = s.spy.last();
    assert_eq!(jid, old);
    assert_eq!(rec["supersededBy"], "new-id");
}

#[test]
fn mark_superseded_on_unknown_job_is_not_found() {
    let s = setup();
    assert_eq!(s.reg.mark_superseded("a", "b"), "NOT_FOUND");
}

#[test]
fn status_persistence_writes_running_at_dispatch_and_terminal_at_completion() {
    let s = setup_with(|d| d.deadline_ms = 10_000.0);
    let reg = Arc::clone(&s.reg);
    let t = std::thread::spawn(move || reg.dispatch(spec_of(|_| {}), WaitOpts::default()));
    let h = s.fake.handle(0);
    assert_eq!(s.spy.len(), 1);
    let (id, rec) = s.spy.nth(0);
    let mut expected = serde_json::json!({
        "status": "RUNNING",
        "progress": {
            "lastTool": null, "tokensSoFar": 0, "elapsedMs": 0,
            "lastAssistant": null, "filesTouchedSoFar": [], "phase": null
        }
    });
    expected["lastHeartbeatAt"] = rec["lastHeartbeatAt"].clone();
    expected["progress"]["elapsedMs"] = rec["progress"]["elapsedMs"].clone();
    assert_eq!(rec, expected);

    h.finish(done_ok());
    let DispatchResult::Output(out) = t.join().unwrap() else {
        panic!("expected a RunOutput");
    };
    assert_eq!(s.spy.len(), 2);
    let (_, rec) = s.spy.nth(1);
    assert_eq!(rec, serde_json::to_value(s.reg.poll(&id)).unwrap());
    assert_eq!(rec["status"], "DONE");
    assert_eq!(rec["result"]["status"], "DONE");
    assert_eq!(rec["result"]["text"], out.text.as_str());
}

#[test]
fn finishing_within_the_deadline_returns_a_run_output() {
    let s = setup_with(|d| d.deadline_ms = 10_000.0);
    let reg = Arc::clone(&s.reg);
    let t = std::thread::spawn(move || reg.dispatch(spec_of(|_| {}), WaitOpts::default()));
    s.fake.handle(0).finish(done_ok());
    let DispatchResult::Output(out) = t.join().unwrap() else {
        panic!("expected a RunOutput");
    };
    assert_eq!(out.status, RunStatus::Done);
    assert_eq!(out.text, "ok\nSTATUS: DONE");
}

#[test]
fn exceeding_the_deadline_detaches_and_is_later_pollable() {
    let s = setup_with(|d| d.deadline_ms = 50.0);
    let r = s.reg.dispatch(spec_of(|_| {}), WaitOpts::default());
    assert_eq!(r.status_label(), "RUNNING");
    let id = id_of(&r);
    // The child is NOT killed by the deadline.
    assert!(s.fake.handle(0).killed().is_empty());
    s.fake.handle(0).finish(done_ok());
    assert_eq!(settle(&s.reg, &id), "DONE");
}

#[test]
fn background_dispatch_persists_start_and_terminal_records() {
    let s = setup();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    assert_eq!(s.spy.len(), 1);
    s.fake.handle(0).finish(done_ok());
    assert_eq!(settle(&s.reg, &id), "DONE");
    assert_eq!(s.spy.len(), 2);
    assert_eq!(status_of(&s.spy.nth(1).1), "DONE");
}

#[test]
fn idle_watchdog_writes_a_stalled_terminal_record() {
    let s = setup_with(|d| d.idle_ms = Some(50.0));
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    assert_eq!(s.spy.len(), 1);
    assert_eq!(settle(&s.reg, &id), "STALLED");
    assert_eq!(s.fake.handle(0).killed(), ["SIGTERM"]);
    assert_eq!(s.spy.len(), 2);
    assert_eq!(status_of(&s.spy.nth(1).1), "STALLED");
}

#[test]
fn cancel_persists_a_cancelled_terminal_record() {
    let s = setup();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    assert_eq!(s.spy.len(), 1);
    s.reg.cancel(&id);
    assert_eq!(s.spy.len(), 2);
    assert_eq!(status_of(&s.spy.nth(1).1), "CANCELLED");
    assert_eq!(s.fake.handle(0).killed(), ["SIGTERM"]);
}

fn write_to(path: &str) -> impl FnOnce(&mut JobSpec) {
    let path = path.to_string();
    move |s| {
        s.is_write = true;
        s.path = Some(path);
        s.background = Some(true);
    }
}

#[test]
fn busy_dispatch_writes_no_status_record() {
    let s = setup();
    s.reg
        .dispatch(spec_of(write_to("/repo")), WaitOpts::default());
    assert_eq!(s.spy.len(), 1);
    let r2 = s
        .reg
        .dispatch(spec_of(write_to("/repo")), WaitOpts::default());
    assert_eq!(r2.status_label(), "BUSY");
    assert_eq!(s.spy.len(), 1);
}

#[test]
fn a_panicking_status_writer_does_not_affect_completion() {
    struct Bad(Mutex<u32>);
    impl StatusRecordWriter for Bad {
        fn write(&self, _: &str, _: &PollResult) {
            let mut n = self.0.lock().unwrap();
            *n += 1;
            if *n == 2 {
                drop(n);
                panic!("boom");
            }
        }
    }
    let bad = Arc::new(Bad(Mutex::new(0)));
    let s = setup_with(|d| {
        d.status_writer = bad.clone();
        d.deadline_ms = 10_000.0;
    });
    let reg = Arc::clone(&s.reg);
    let t = std::thread::spawn(move || reg.dispatch(spec_of(|_| {}), WaitOpts::default()));
    s.fake.handle(0).finish(done_ok());
    let DispatchResult::Output(out) = t.join().unwrap() else {
        panic!("expected a RunOutput");
    };
    assert_eq!(out.status, RunStatus::Done);
    assert_eq!(*bad.0.lock().unwrap(), 2);
}

#[test]
fn progress_events_do_not_trigger_status_writes() {
    let s = setup();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    s.fake.handle(0).progress(
        "shell",
        42.0,
        Some("running the test suite now"),
        &["src/foo.rs"],
        Some("running_tool"),
    );
    sleep(Duration::from_millis(30));
    assert_eq!(s.spy.len(), 1);
    s.fake.handle(0).finish(done_ok());
    assert_eq!(settle(&s.reg, &id), "DONE");
    assert_eq!(s.spy.len(), 2);
}

#[test]
fn file_status_record_is_overwritten_from_running_to_terminal() {
    let s = setup_with(|d| d.status_writer = Arc::new(FileStatusRecordWriter));
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    let path = status_record_path(&id);
    let read = || -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    };
    assert_eq!(read()["status"], "RUNNING");
    s.fake.handle(0).finish(done_ok());
    assert_eq!(settle(&s.reg, &id), "DONE");
    let terminal = read();
    assert_eq!(terminal["status"], "DONE");
    let polled = serde_json::to_value(s.reg.poll(&id)).unwrap();
    assert_eq!(terminal["result"], polled["result"]);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn background_returns_immediately() {
    let s = setup_with(|d| d.deadline_ms = 10_000.0);
    let t = Instant::now();
    let r = s.reg.dispatch(spec_of(bg), WaitOpts::default());
    assert_eq!(r.status_label(), "RUNNING");
    assert!(t.elapsed() < Duration::from_millis(1000));
}

#[test]
fn a_second_write_to_a_locked_path_is_busy() {
    let s = setup();
    let r1 = s
        .reg
        .dispatch(spec_of(write_to("/repo")), WaitOpts::default());
    assert_eq!(r1.status_label(), "RUNNING");
    let r2 = s
        .reg
        .dispatch(spec_of(write_to("/repo")), WaitOpts::default());
    let DispatchResult::Busy {
        busy_path, job_id, ..
    } = r2
    else {
        panic!("expected BUSY");
    };
    assert_eq!(busy_path, "/repo");
    assert_eq!(job_id, id_of(&r1));
}

#[test]
fn the_lock_releases_when_the_holder_finishes() {
    let s = setup();
    let id = id_of(
        &s.reg
            .dispatch(spec_of(write_to("/repo")), WaitOpts::default()),
    );
    s.fake.handle(0).finish(done_ok());
    settle(&s.reg, &id);
    let r2 = s
        .reg
        .dispatch(spec_of(write_to("/repo")), WaitOpts::default());
    assert_eq!(r2.status_label(), "RUNNING");
}

#[test]
fn cancel_sigterms_the_child_and_marks_cancelled() {
    let s = setup();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    assert_eq!(s.reg.cancel(&id), "CANCELLED");
    assert_eq!(s.fake.handle(0).killed(), ["SIGTERM"]);
}

#[test]
fn idle_watchdog_sigterms_a_silent_job() {
    let s = setup_with(|d| d.idle_ms = Some(50.0));
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    assert_eq!(settle(&s.reg, &id), "STALLED");
    assert_eq!(s.fake.handle(0).killed(), ["SIGTERM"]);
}

#[test]
fn a_stalled_jobs_text_summarizes_last_known_progress() {
    let s = setup_with(|d| {
        d.idle_ms = Some(100.0);
        d.deadline_ms = 10_000.0;
    });
    let reg = Arc::clone(&s.reg);
    let t = std::thread::spawn(move || reg.dispatch(spec_of(|_| {}), WaitOpts::default()));
    // Not "running_tool" — keep this on the short idle window.
    s.fake.handle(0).progress(
        "shell",
        42.0,
        Some("running the test suite now"),
        &["src/foo.rs"],
        Some("thinking"),
    );
    let DispatchResult::Output(out) = t.join().unwrap() else {
        panic!("expected a RunOutput");
    };
    for needle in [
        "shell",
        "42 tokens",
        "src/foo.rs",
        "running the test suite now",
    ] {
        assert!(
            out.text.contains(needle),
            "{needle:?} missing from {:?}",
            out.text
        );
    }
}

#[test]
fn a_progress_event_rearms_the_idle_watchdog() {
    let s = setup_with(|d| d.idle_ms = Some(300.0));
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    sleep(Duration::from_millis(200));
    s.fake.handle(0).progress("shell", 1.0, None, &[], None);
    sleep(Duration::from_millis(200)); // 400 total, but only 200 since the event
    assert!(s.fake.handle(0).killed().is_empty());
    assert_eq!(s.reg.poll(&id), "RUNNING");
}

#[test]
fn a_per_call_idle_override_beats_the_server_default() {
    let s = setup_with(|d| d.idle_ms = Some(50.0));
    let id = id_of(&s.reg.dispatch(
        spec_of(|s| {
            bg(s);
            s.idle_ms = Some(Some(400.0));
        }),
        WaitOpts::default(),
    ));
    sleep(Duration::from_millis(150)); // would have STALLED under the server default
    assert!(s.fake.handle(0).killed().is_empty());
    assert_eq!(settle(&s.reg, &id), "STALLED");
    assert_eq!(s.fake.handle(0).killed(), ["SIGTERM"]);
}

#[test]
fn a_per_call_idle_null_disables_the_watchdog() {
    let s = setup_with(|d| d.idle_ms = Some(30.0));
    let id = id_of(&s.reg.dispatch(
        spec_of(|s| {
            bg(s);
            s.idle_ms = Some(None);
        }),
        WaitOpts::default(),
    ));
    sleep(Duration::from_millis(200));
    assert!(s.fake.handle(0).killed().is_empty());
    assert_eq!(s.reg.poll(&id), "RUNNING");
}

#[test]
fn a_tool_in_flight_uses_the_tool_idle_window() {
    let s = setup_with(|d| {
        d.idle_ms = Some(50.0);
        d.tool_idle_ms = Some(400.0);
    });
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    s.fake
        .handle(0)
        .progress("shell", 1.0, None, &[], Some("running_tool"));
    sleep(Duration::from_millis(150)); // past idle, but a tool is in flight
    assert!(s.fake.handle(0).killed().is_empty());
    assert_eq!(s.reg.poll(&id), "RUNNING");
    assert_eq!(settle(&s.reg, &id), "STALLED");
    assert_eq!(s.fake.handle(0).killed(), ["SIGTERM"]);
}

#[test]
fn leaving_the_tool_phase_reverts_to_the_short_window() {
    let s = setup_with(|d| {
        d.idle_ms = Some(80.0);
        d.tool_idle_ms = Some(10_000.0);
    });
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    s.fake
        .handle(0)
        .progress("shell", 1.0, None, &[], Some("running_tool"));
    sleep(Duration::from_millis(150)); // fine — still under the tool window
    assert_eq!(s.reg.poll(&id), "RUNNING");
    s.fake.handle(0).progress(
        "shell",
        2.0,
        Some("done with that"),
        &[],
        Some("responding"),
    );
    assert_eq!(settle(&s.reg, &id), "STALLED");
    assert_eq!(s.fake.handle(0).killed(), ["SIGTERM"]);
}

#[test]
fn a_per_call_tool_idle_override_applies_while_a_tool_is_in_flight() {
    let s = setup_with(|d| {
        d.idle_ms = Some(50.0);
        d.tool_idle_ms = Some(80.0);
    });
    let id = id_of(&s.reg.dispatch(
        spec_of(|s| {
            bg(s);
            s.tool_idle_ms = Some(Some(10_000.0));
        }),
        WaitOpts::default(),
    ));
    s.fake
        .handle(0)
        .progress("shell", 1.0, None, &[], Some("running_tool"));
    sleep(Duration::from_millis(200)); // past the server tool window, under the override
    assert!(s.fake.handle(0).killed().is_empty());
    assert_eq!(s.reg.poll(&id), "RUNNING");
}

#[test]
fn raw_activity_rearms_the_watchdog() {
    let s = setup_with(|d| d.idle_ms = Some(300.0));
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    sleep(Duration::from_millis(200));
    s.fake.handle(0).activity();
    sleep(Duration::from_millis(200));
    assert!(s.fake.handle(0).killed().is_empty());
    assert_eq!(s.reg.poll(&id), "RUNNING");
}

#[test]
fn poll_on_an_unknown_id_is_not_found() {
    assert_eq!(setup().reg.poll("nope"), "NOT_FOUND");
}

#[test]
fn wait_returns_when_the_job_completes() {
    let s = setup();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    let h = s.fake.handle(0);
    std::thread::spawn(move || {
        sleep(Duration::from_millis(30));
        h.finish(done_ok());
    });
    assert_eq!(s.reg.wait(&id, Some(10_000.0), WaitOpts::default()), "DONE");
}

#[test]
fn wait_returns_the_running_snapshot_on_timeout() {
    let s = setup();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    let t = Instant::now();
    // Clamped up to the 1000ms floor.
    assert_eq!(s.reg.wait(&id, Some(10.0), WaitOpts::default()), "RUNNING");
    assert!(t.elapsed() >= Duration::from_millis(1000));
}

#[test]
fn wait_returns_early_when_aborted() {
    let s = setup();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    let abort: Abort = Arc::new(AtomicBool::new(false));
    let (a, reg) = (Arc::clone(&abort), Arc::clone(&s.reg));
    std::thread::spawn(move || {
        sleep(Duration::from_millis(50));
        a.store(true, Ordering::SeqCst);
        reg.wake();
    });
    let t = Instant::now();
    let o = WaitOpts {
        sink: None,
        abort: Some(&abort),
    };
    assert_eq!(s.reg.wait(&id, Some(10_000.0), o), "RUNNING");
    assert!(t.elapsed() < Duration::from_millis(2000));
}

#[test]
fn wait_forwards_progress_to_the_sink() {
    let s = setup();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    let got: Arc<Mutex<Vec<ProgressUpdate>>> = Arc::default();
    let g = Arc::clone(&got);
    let sink: ProgressSink = Arc::new(move |u| g.lock().unwrap().push(u));
    let h = s.fake.handle(0);
    std::thread::spawn(move || {
        sleep(Duration::from_millis(30));
        h.progress("shell", 5.0, None, &[], Some("running_tool"));
        sleep(Duration::from_millis(30));
        h.finish(done_ok());
    });
    let o = WaitOpts {
        sink: Some(&sink),
        abort: None,
    };
    assert_eq!(s.reg.wait(&id, Some(10_000.0), o), "DONE");
    let got = got.lock().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].last_tool.as_deref(), Some("shell"));
    assert_eq!(got[0].job_tag, None);
}

#[test]
fn wait_any_returns_on_the_first_terminal_job() {
    let s = setup();
    let a = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    let b = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    let h = s.fake.handle(1);
    std::thread::spawn(move || {
        sleep(Duration::from_millis(30));
        h.finish(done_ok());
    });
    let res = s
        .reg
        .wait_any(&[a.clone(), b.clone()], Some(10_000.0), WaitOpts::default());
    assert_eq!(res.first_done.as_deref(), Some(b.as_str()));
    assert_eq!(res.jobs[&b], "DONE");
    assert_eq!(res.jobs[&a], "RUNNING");
}

#[test]
fn wait_all_returns_when_all_known_jobs_are_terminal() {
    let s = setup();
    let a = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    let b = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    let (h0, h1) = (s.fake.handle(0), s.fake.handle(1));
    std::thread::spawn(move || {
        sleep(Duration::from_millis(30));
        h0.finish(done_ok());
        h1.finish(done_ok());
    });
    let ids = [a.clone(), b.clone(), "unknown".to_string()];
    let res = s.reg.wait_all(&ids, Some(10_000.0), WaitOpts::default());
    assert!(res.all_done);
    assert_eq!(res.jobs[&a], "DONE");
    assert_eq!(res.jobs[&b], "DONE");
    assert!(!res.jobs.contains_key("unknown"));
}

#[test]
fn wait_all_with_empty_input_is_immediately_all_done() {
    let res = setup().reg.wait_all(&[], Some(1000.0), WaitOpts::default());
    assert_eq!(
        serde_json::to_value(res).unwrap(),
        serde_json::json!({ "jobs": {}, "allDone": true })
    );
}

#[test]
fn kill_all_sigterms_every_active_child() {
    let s = setup();
    s.reg.dispatch(spec_of(bg), WaitOpts::default());
    s.reg.dispatch(spec_of(bg), WaitOpts::default());
    s.reg.kill_all();
    assert_eq!(s.fake.handle(0).killed(), ["SIGTERM"]);
    assert_eq!(s.fake.handle(1).killed(), ["SIGTERM"]);
}

#[test]
fn child_exit_ends_the_idle_watchdog_during_a_slow_finalize() {
    let (unblock_tx, unblock_rx) = mpsc::channel::<()>();
    let unblock_rx = Mutex::new(unblock_rx);
    let s = setup_with(move |d| {
        d.idle_ms = Some(30.0);
        d.finalize = Arc::new(move |res, ctx| {
            let _ = unblock_rx.lock().unwrap().recv();
            fake_finalize(res, ctx)
        });
    });
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    s.fake.handle(0).progress("shell", 1.0, None, &[], None);
    s.fake.handle(0).finish(done_ok());
    sleep(Duration::from_millis(150));
    assert_eq!(s.reg.poll(&id), "RUNNING");
    assert!(s.fake.handle(0).killed().is_empty());
    unblock_tx.send(()).unwrap();
    assert_eq!(settle(&s.reg, &id), "DONE");
}

#[test]
fn cancel_skips_finalize_side_effects() {
    let gate_ran = Arc::new(AtomicBool::new(false));
    let g = Arc::clone(&gate_ran);
    let s = setup_with(move |d| {
        d.finalize = Arc::new(move |res, ctx| {
            if !ctx.gate.is_empty() {
                g.store(true, Ordering::SeqCst);
            }
            fake_finalize(res, ctx)
        });
    });
    let id = id_of(&s.reg.dispatch(
        spec_of(|s| {
            bg(s);
            s.gate = "make test".into();
        }),
        WaitOpts::default(),
    ));
    assert_eq!(s.reg.cancel(&id), "CANCELLED");
    assert!(!gate_ran.load(Ordering::SeqCst));
    assert_eq!(s.fake.handle(0).killed(), ["SIGTERM"]);
}

#[test]
fn foreground_needs_context_parks_the_job() {
    let s = setup_with(|d| d.deadline_ms = 10_000.0);
    let reg = Arc::clone(&s.reg);
    let t = std::thread::spawn(move || reg.dispatch(spec_of(|_| {}), WaitOpts::default()));
    s.fake.handle(0).finish(result_of(
        "Which API version should I target?\nSTATUS: NEEDS_CONTEXT",
        Some("sess-park-1"),
    ));
    let DispatchResult::Output(out) = t.join().unwrap() else {
        panic!("expected a RunOutput");
    };
    assert_eq!(out.status, RunStatus::NeedsContext);
    assert_eq!(out.session_id.as_deref(), Some("sess-park-1"));
    assert!(out.text.contains("Which API version"));
    let id = out.job_id.expect("job id");
    match s.reg.lookup_answer(&id) {
        AnswerLookup::Ok {
            session_id,
            resume_context,
        } => {
            assert_eq!(session_id, "sess-park-1");
            assert_eq!(resume_context.model, "composer-2.5");
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn lookup_answer_is_not_found_for_unknown_ids() {
    assert_eq!(setup().reg.lookup_answer("nope"), AnswerLookup::NotFound);
}

#[test]
fn lookup_answer_rejects_jobs_not_awaiting_an_answer() {
    let s = setup();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    assert_eq!(
        s.reg.lookup_answer(&id),
        AnswerLookup::NotAwaiting {
            status: JobStatus::Running
        }
    );
    s.fake.handle(0).finish(done_ok());
    settle(&s.reg, &id);
    assert_eq!(
        s.reg.lookup_answer(&id),
        AnswerLookup::NotAwaiting {
            status: JobStatus::Done
        }
    );
}

#[test]
fn completed_jobs_evict_past_100() {
    let s = setup();
    let ids: Vec<String> = (0..101)
        .map(|_| id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default())))
        .collect();
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        101
    );
    for (i, id) in ids.iter().enumerate() {
        s.fake.handle(i).finish(done_ok());
        settle(&s.reg, id);
    }
    assert_eq!(s.reg.poll(&ids[0]), "NOT_FOUND");
    assert_eq!(s.reg.poll(&ids[100]), "DONE");
}

#[test]
fn detaching_a_write_names_the_locked_tree() {
    let s = setup_with(|d| d.deadline_ms = 50.0);
    let detached = s.reg.dispatch(
        spec_of(|s| {
            s.is_write = true;
            s.path = Some("/repo".into());
        }),
        WaitOpts::default(),
    );
    let v = serde_json::to_value(&detached).unwrap();
    assert_eq!(v["status"], "RUNNING");
    assert_eq!(v["busyPath"], "/repo");

    let bg = s
        .reg
        .dispatch(spec_of(write_to("/other")), WaitOpts::default());
    let v = serde_json::to_value(&bg).unwrap();
    assert_eq!(v["status"], "RUNNING");
    assert_eq!(v["busyPath"], "/other");
}

#[test]
fn lookup_answer_rejects_needs_context_without_a_session() {
    let s = setup_with(|d| d.deadline_ms = 10_000.0);
    let reg = Arc::clone(&s.reg);
    let t = std::thread::spawn(move || reg.dispatch(spec_of(|_| {}), WaitOpts::default()));
    s.fake
        .handle(0)
        .finish(result_of("Need a choice\nSTATUS: NEEDS_CONTEXT", None));
    let DispatchResult::Output(out) = t.join().unwrap() else {
        panic!("expected a RunOutput");
    };
    assert_eq!(out.status, RunStatus::NeedsContext);
    assert_eq!(
        s.reg.lookup_answer(&out.job_id.unwrap()),
        AnswerLookup::NotAwaiting {
            status: JobStatus::NeedsContext
        }
    );
}

#[test]
fn cancel_during_finalizing_aborts_the_finalize_stage() {
    // Finalize hangs until ctx.signal is set — models the gate's abort support.
    let started = Arc::new(AtomicBool::new(false));
    let st = Arc::clone(&started);
    let s = setup_with(move |d| {
        d.finalize = Arc::new(move |res, ctx| {
            st.store(true, Ordering::SeqCst);
            let sig = ctx.signal.clone().unwrap();
            while !sig.load(Ordering::SeqCst) {
                sleep(Duration::from_millis(2));
            }
            let mut out = fake_finalize(res, ctx);
            out.status = RunStatus::Error;
            out.text = "finalize aborted".into();
            out
        });
    });
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    s.fake.handle(0).finish(done_ok());
    while !started.load(Ordering::SeqCst) {
        sleep(Duration::from_millis(2));
    }
    assert_eq!(s.reg.cancel(&id), "CANCELLED");
    // A normal finalize that was cancelled keeps its own text.
    let v = serde_json::to_value(s.reg.poll(&id)).unwrap();
    assert_eq!(v["result"]["text"], "finalize aborted");
}

#[test]
fn a_parked_job_survives_100_unrelated_completions() {
    let s = setup();
    let parked = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    s.fake
        .handle(0)
        .finish(result_of("what port?\nSTATUS: NEEDS_CONTEXT", Some("s-1")));
    settle(&s.reg, &parked);
    for i in 0..100 {
        let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
        s.fake.handle(i + 1).finish(done_ok());
        settle(&s.reg, &id);
    }
    assert_eq!(s.reg.poll(&parked), "NEEDS_CONTEXT");
    assert!(matches!(
        s.reg.lookup_answer(&parked),
        AnswerLookup::Ok { .. }
    ));
}

#[test]
fn an_answerable_job_expires_after_the_ttl() {
    let s = setup_with(|d| d.answerable_ttl_ms = 50);
    let parked = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    s.fake
        .handle(0)
        .finish(result_of("what port?\nSTATUS: NEEDS_CONTEXT", Some("s-2")));
    settle(&s.reg, &parked);
    assert!(matches!(
        s.reg.lookup_answer(&parked),
        AnswerLookup::Ok { .. }
    ));
    sleep(Duration::from_millis(80));
    assert_eq!(s.reg.lookup_answer(&parked), AnswerLookup::NotFound);
    assert_eq!(s.reg.poll(&parked), "NOT_FOUND");
}

#[test]
fn threads_exit_when_jobs_retire() {
    // Each job owns a driver and a watchdog thread; neither may outlive the job.
    let s = setup();
    let before = s.fake.count();
    let id = id_of(&s.reg.dispatch(spec_of(bg), WaitOpts::default()));
    s.fake.handle(before).finish(done_ok());
    settle(&s.reg, &id);
    sleep(Duration::from_millis(50));
    // The watchdog holds an Arc to the registry; once it exits only ours remains.
    assert_eq!(Arc::strong_count(&s.reg), 1);
}
