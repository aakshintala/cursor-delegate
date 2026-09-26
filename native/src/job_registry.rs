//! The stateful core: spawn via the backend, race the deadline, detach to a jobId, track
//! progress, run the idle watchdog and heartbeat, serialize same-path writes, and serve
//! poll/cancel/wait.
//!
//! Concurrency model: one `Mutex<State>` and one `Condvar`. Every state change that a waiter
//! could care about (progress, retirement, abort) does `notify_all`; every waiter re-checks
//! its own predicate. Each job owns two threads: a driver (pumps the child, then finalizes)
//! and a watchdog (idle kill + heartbeat). Status records are written under the lock so a
//! stale RUNNING record can never overwrite a terminal one.

use crate::backends::types::{Backend, BackendResult, Event, ProgressSnapshotRaw};
use crate::finalize::{finalize_run, finalize_stall};
use crate::progress::{ProgressSink, ProgressUpdate};
use crate::status_record::{StatusRecordWriter, file_status_record_writer};
use crate::types::{
    DispatchResult, FinalizeCtx, JobSpec, JobStatus, PollResult, ProgressSnapshot, ResumeContext,
    RunOutput, RunStatus,
};
use crate::util::{Abort, clamp_wait, random_uuid};
use indexmap::IndexMap;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const COMPLETED_CAP: usize = 100;
/// A parked NEEDS_CONTEXT job must not become unanswerable just because 100 unrelated jobs
/// completed. Terminal-but-not-answerable jobs evict oldest-first; answerable ones are kept
/// until this TTL expires, then lazily dropped.
const ANSWERABLE_TTL_MS: u64 = 24 * 60 * 60 * 1000;
const DEFAULT_WAIT_TIMEOUT: f64 = 120_000.0;
/// How often a RUNNING job's status record is refreshed on disk.
const HEARTBEAT_MS: u64 = 30_000;

pub type FinalizeFn = Arc<dyn Fn(&BackendResult, &FinalizeCtx) -> RunOutput + Send + Sync>;

pub struct RegistryDeps {
    pub backend: Arc<dyn Backend>,
    pub deadline_ms: f64,
    pub idle_ms: Option<f64>,
    /// Wider idle window applied while a tool call is in flight (`phase == "running_tool"`).
    pub tool_idle_ms: Option<f64>,
    pub finalize: FinalizeFn,
    pub finalize_stall: FinalizeFn,
    pub status_writer: Arc<dyn StatusRecordWriter>,
    pub heartbeat_ms: u64,
    pub answerable_ttl_ms: u64,
}

impl RegistryDeps {
    pub fn new(
        backend: Arc<dyn Backend>,
        deadline_ms: f64,
        idle_ms: Option<f64>,
        tool_idle_ms: Option<f64>,
    ) -> Self {
        Self {
            backend,
            deadline_ms,
            idle_ms,
            tool_idle_ms,
            finalize: Arc::new(finalize_run),
            finalize_stall: Arc::new(finalize_stall),
            status_writer: Arc::new(file_status_record_writer()),
            heartbeat_ms: HEARTBEAT_MS,
            answerable_ttl_ms: ANSWERABLE_TTL_MS,
        }
    }
}

/// Progress sink and cancellation for one blocking call.
#[derive(Clone, Copy, Default)]
pub struct WaitOpts<'a> {
    pub sink: Option<&'a ProgressSink>,
    pub abort: Option<&'a Abort>,
}

#[derive(Debug, PartialEq)]
pub enum AnswerLookup {
    Ok {
        session_id: String,
        resume_context: ResumeContext,
    },
    NotFound,
    NotAwaiting {
        status: JobStatus,
    },
}

#[derive(Debug, Serialize)]
pub struct WaitAnyResult {
    pub jobs: IndexMap<String, PollResult>,
    #[serde(rename = "firstDone", skip_serializing_if = "Option::is_none")]
    pub first_done: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WaitAllResult {
    pub jobs: IndexMap<String, PollResult>,
    #[serde(rename = "allDone")]
    pub all_done: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Running,
    /// Child exited; finalize (git delta, gate) in progress. Status is still RUNNING.
    Finalizing,
    Terminal,
}

struct Job {
    status: JobStatus,
    stage: Stage,
    spec: Arc<JobSpec>,
    kill: Arc<dyn Fn() + Send + Sync>,
    /// Set by cancel/shutdown to escalate into a hung gate during finalizing.
    finalize_abort: Abort,
    started: Instant,
    progress: ProgressSnapshotRaw,
    /// Bumped on every progress event so waiters can forward new snapshots to their sinks.
    progress_seq: u64,
    last_event: Instant,
    termination: Option<JobStatus>,
    superseded_by: Option<String>,
    retired_at: Option<Instant>,
    output: Option<RunOutput>,
}

impl Job {
    fn answerable(&self) -> bool {
        self.status == JobStatus::NeedsContext
            && self.output.as_ref().is_some_and(|o| o.session_id.is_some())
    }
}

#[derive(Default)]
struct State {
    jobs: HashMap<String, Job>,
    /// Retired job ids, oldest first.
    retired: VecDeque<String>,
    path_lock: HashMap<String, String>,
}

pub struct JobRegistry {
    st: Mutex<State>,
    cv: Condvar,
    deps: RegistryDeps,
}

fn ms(v: f64) -> Duration {
    Duration::from_millis(v.max(0.0) as u64)
}

fn epoch_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

fn detach_result(spec: &JobSpec, job_id: &str) -> DispatchResult {
    // A write names the locked tree so the caller knows which path is held.
    DispatchResult::Detached {
        status: "RUNNING",
        job_id: job_id.to_string(),
        busy_path: if spec.is_write {
            spec.path.clone()
        } else {
            None
        },
    }
}

/// A CANCELLED/STALLED job never gets a terminal `result` line, so there is no `text` to
/// report from the backend. Build one from the live progress the registry already tracked.
fn describe_stall_progress(job: &Job) -> String {
    let verb = if job.termination == Some(JobStatus::Cancelled) {
        "Cancelled"
    } else {
        "Idle watchdog killed this job"
    };
    let p = &job.progress;
    let mut parts = vec![format!(
        "{verb} after {}s.",
        (job.started.elapsed().as_millis() as f64 / 1000.0).round()
    )];
    if let Some(phase) = &p.phase {
        parts.push(format!("Last phase: {phase}."));
    }
    if let Some(tool) = &p.last_tool {
        parts.push(format!("Last tool: {tool}."));
    }
    if p.tokens_so_far > 0.0 {
        parts.push(format!(
            "{} tokens streamed before the kill.",
            p.tokens_so_far
        ));
    }
    if let Some(a) = &p.last_assistant {
        parts.push(format!("Last assistant text: \"{a}\""));
    }
    if !p.files_touched.is_empty() {
        parts.push(format!("Files touched: {}.", p.files_touched.join(", ")));
    }
    parts.join(" ")
}

fn poll_locked(st: &State, job_id: &str) -> PollResult {
    let Some(job) = st.jobs.get(job_id) else {
        return PollResult::not_found();
    };
    if job.status == JobStatus::Running {
        let p = &job.progress;
        return PollResult::Running {
            status: "RUNNING",
            last_heartbeat_at: epoch_ms(),
            superseded_by: job.superseded_by.clone(),
            progress: ProgressSnapshot {
                last_tool: p.last_tool.clone(),
                tokens_so_far: p.tokens_so_far,
                elapsed_ms: job.started.elapsed().as_millis() as f64,
                last_assistant: p.last_assistant.clone(),
                files_touched_so_far: p.files_touched.clone(),
                phase: p.phase.clone(),
            },
        };
    }
    PollResult::Terminal {
        status: job.status,
        result: job.output.clone().expect("terminal job has output"),
        superseded_by: job.superseded_by.clone(),
    }
}

impl JobRegistry {
    pub fn new(deps: RegistryDeps) -> Arc<Self> {
        Arc::new(Self {
            st: Mutex::new(State::default()),
            cv: Condvar::new(),
            deps,
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Best-effort: a misbehaving writer must never affect the job.
    fn write_record(&self, st: &State, job_id: &str) {
        let record = poll_locked(st, job_id);
        let w = &self.deps.status_writer;
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.write(job_id, &record)));
    }

    /// Wake every blocked waiter so it re-checks its abort flag.
    pub fn wake(&self) {
        let _g = self.lock();
        self.cv.notify_all();
    }

    pub fn dispatch(self: &Arc<Self>, spec: JobSpec, o: WaitOpts<'_>) -> DispatchResult {
        let spec = Arc::new(spec);
        let lock_path = spec.path.clone().filter(|_| spec.is_write);
        let job_id = random_uuid();
        let drive;
        {
            let mut st = self.lock();
            // Same-path write serialization (no queue).
            if let Some(holder) = lock_path.as_ref().and_then(|p| st.path_lock.get(p)) {
                return DispatchResult::Busy {
                    status: "BUSY",
                    job_id: holder.clone(),
                    busy_path: lock_path.clone().unwrap_or_default(),
                };
            }
            let spawned = self.deps.backend.run(&spec);
            drive = spawned.drive;
            let now = Instant::now();
            st.jobs.insert(
                job_id.clone(),
                Job {
                    status: JobStatus::Running,
                    stage: Stage::Running,
                    spec: Arc::clone(&spec),
                    kill: Arc::from(spawned.kill),
                    finalize_abort: Arc::new(AtomicBool::new(false)),
                    started: now,
                    progress: ProgressSnapshotRaw::default(),
                    progress_seq: 0,
                    last_event: now,
                    termination: None,
                    superseded_by: None,
                    retired_at: None,
                    output: None,
                },
            );
            self.write_record(&st, &job_id);
            if let Some(p) = &lock_path {
                st.path_lock.insert(p.clone(), job_id.clone());
            }
        }

        let (me, id) = (Arc::clone(self), job_id.clone());
        std::thread::spawn(move || {
            let res = drive(&|e| me.on_event(&id, e));
            me.finalize_job(&id, res);
        });
        let (me, id) = (Arc::clone(self), job_id.clone());
        std::thread::spawn(move || me.watchdog(&id));

        // Background -> detach immediately, dropping the caller sink.
        if spec.background == Some(true) {
            return detach_result(&spec, &job_id);
        }
        let deadline = spec
            .wait_ms
            .map(clamp_wait)
            .unwrap_or(self.deps.deadline_ms);
        let ids = [job_id.clone()];
        let st = self.block(&ids, ms(deadline), o, false, |st| {
            st.jobs
                .get(&job_id)
                .is_none_or(|j| j.stage == Stage::Terminal)
        });
        match st.jobs.get(&job_id) {
            Some(j) if j.stage == Stage::Terminal => {
                DispatchResult::Output(j.output.clone().expect("terminal job has output"))
            }
            _ => detach_result(&spec, &job_id),
        }
    }

    fn on_event(&self, job_id: &str, e: Event) {
        let mut st = self.lock();
        let Some(job) = st.jobs.get_mut(job_id) else {
            return;
        };
        job.last_event = Instant::now();
        if let Event::Progress(snap) = e {
            job.progress = snap;
            job.progress_seq += 1;
            // Phase may have changed the idle window; waiters may have sinks to feed.
            self.cv.notify_all();
        }
    }

    /// Idle watchdog + heartbeat for one job; exits when the job retires.
    fn watchdog(&self, job_id: &str) {
        let heartbeat = Duration::from_millis(self.deps.heartbeat_ms);
        let mut next_heartbeat = Instant::now() + heartbeat;
        let mut st = self.lock();
        loop {
            let Some(job) = st.jobs.get_mut(job_id) else {
                return;
            };
            if job.stage == Stage::Terminal {
                return;
            }
            let now = Instant::now();
            let mut wake_at = next_heartbeat;
            // Tiered by phase: a tool call in flight can go silent for a long time
            // legitimately (a build, a test suite); no tool in flight and no event means we're
            // waiting on the model itself, where a short silence really is anomalous.
            if job.stage == Stage::Running && job.termination.is_none() {
                let spec = &job.spec;
                let window = if job.progress.phase.as_deref() == Some("running_tool") {
                    spec.tool_idle_ms.unwrap_or(self.deps.tool_idle_ms)
                } else {
                    spec.idle_ms.unwrap_or(self.deps.idle_ms)
                };
                if let Some(w) = window {
                    let due = job.last_event + ms(w);
                    if now >= due {
                        job.termination = Some(JobStatus::Stalled);
                        (job.kill)();
                    } else {
                        wake_at = wake_at.min(due);
                    }
                }
            }
            if now >= next_heartbeat {
                // Refresh the on-disk record so a frozen one (dead server) is detectable.
                self.write_record(&st, job_id);
                next_heartbeat = now + heartbeat;
                wake_at = wake_at.min(next_heartbeat);
            }
            st = self
                .cv
                .wait_timeout(st, wake_at.saturating_duration_since(now))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn finalize_job(&self, job_id: &str, res: BackendResult) {
        let (spec, termination, abort) = {
            let mut st = self.lock();
            let Some(job) = st.jobs.get_mut(job_id) else {
                return;
            };
            job.stage = Stage::Finalizing;
            (
                Arc::clone(&job.spec),
                job.termination,
                Arc::clone(&job.finalize_abort),
            )
        };
        let ctx = FinalizeCtx {
            cwd: spec.cwd.clone(),
            head_before: spec.head_before.clone(),
            is_write: spec.is_write,
            gate: spec.gate.clone(),
            allow_partial_commit: spec.allow_partial_commit,
            model: spec.model.clone(),
            backend: spec.backend.clone(),
            price_map: spec.price_map.clone(),
            job_id: Some(job_id.to_string()),
            downgraded: spec.downgraded,
            run_gate: None,
            signal: Some(abort),
            git_delta: None,
            worktree_name: spec.worktree_name.clone(),
            resolve_worktree_path: None,
        };
        // Cancel/stall: finalize_stall skips the gate (meaningless against a killed run) but
        // still computes the change-set, since whatever the agent already wrote is what the
        // caller needs to decide whether to keep, discard, or redispatch.
        let f = if termination.is_some() {
            &self.deps.finalize_stall
        } else {
            &self.deps.finalize
        };
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&res, &ctx)))
            .unwrap_or_else(|_| RunOutput {
                status: RunStatus::Error,
                text: "finalize panicked".into(),
                session_id: None,
                backend: spec.backend.clone(),
                model: spec.model.clone(),
                usage: None,
                cost_usd: None,
                cost_estimated: true,
                duration_ms: None,
                job_id: Some(job_id.to_string()),
                downgraded: None,
                stderr_tail: None,
                gate_result: None,
                change_set: None,
                concerns: None,
            });
        let mut st = self.lock();
        let mut out = out;
        if termination.is_some()
            && let Some(job) = st.jobs.get(job_id)
        {
            // No terminal `result` line to report from: describe the last known progress.
            out.text = describe_stall_progress(job);
        }
        self.retire(&mut st, job_id, out);
        self.cv.notify_all();
    }

    fn retire(&self, st: &mut State, job_id: &str, out: RunOutput) {
        let Some(job) = st.jobs.get_mut(job_id) else {
            return;
        };
        job.status = job
            .termination
            .unwrap_or_else(|| JobStatus::from_run(out.status));
        job.stage = Stage::Terminal;
        job.output = Some(out);
        job.retired_at = Some(Instant::now());
        job.kill = Arc::new(|| {});
        let lock_path = job.spec.path.clone().filter(|_| job.spec.is_write);
        if let Some(p) = lock_path
            && st.path_lock.get(&p).is_some_and(|h| h == job_id)
        {
            st.path_lock.remove(&p);
        }
        st.retired.push_back(job_id.to_string());
        let mut i = 0;
        while st.retired.len() > COMPLETED_CAP && i < st.retired.len() {
            let id = &st.retired[i];
            // Answerable jobs outlive the cap (up to the TTL); everything else evicts oldest-first.
            if st.jobs.get(id).is_some_and(Job::answerable) {
                i += 1;
            } else {
                let id = st.retired.remove(i).expect("index in range");
                st.jobs.remove(&id);
            }
        }
        self.write_record(st, job_id);
    }

    /// Block until `done`, the timeout, or abort — forwarding new progress to the sink.
    fn block(
        &self,
        ids: &[String],
        timeout: Duration,
        o: WaitOpts<'_>,
        tag: bool,
        mut done: impl FnMut(&State) -> bool,
    ) -> MutexGuard<'_, State> {
        let deadline = Instant::now() + timeout;
        let mut st = self.lock();
        // Sinks only see progress that happens after the wait starts.
        let mut seen: Vec<u64> = ids
            .iter()
            .map(|id| st.jobs.get(id).map_or(0, |j| j.progress_seq))
            .collect();
        loop {
            if done(&st) || o.abort.is_some_and(|a| a.load(Ordering::SeqCst)) {
                return st;
            }
            let now = Instant::now();
            if now >= deadline {
                return st;
            }
            let mut updates = Vec::new();
            if o.sink.is_some() {
                for (i, id) in ids.iter().enumerate() {
                    let Some(j) = st.jobs.get(id) else { continue };
                    if j.status == JobStatus::Running && j.progress_seq > seen[i] {
                        seen[i] = j.progress_seq;
                        updates.push(ProgressUpdate {
                            last_tool: j.progress.last_tool.clone(),
                            tokens_so_far: j.progress.tokens_so_far,
                            elapsed_ms: j.started.elapsed().as_millis() as f64,
                            phase: j.progress.phase.clone(),
                            job_tag: tag.then(|| id.chars().take(6).collect()),
                        });
                    }
                }
            }
            if let (Some(sink), false) = (o.sink, updates.is_empty()) {
                // The sink writes to stdout; never hold the registry lock across that.
                drop(st);
                for u in updates {
                    sink(u);
                }
                st = self.lock();
                continue;
            }
            st = self
                .cv
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    pub fn poll(&self, job_id: &str) -> PollResult {
        poll_locked(&self.lock(), job_id)
    }

    /// Record that `old_id` continues under `new_id` (resume path) so watchers can follow it.
    pub fn mark_superseded(&self, old_id: &str, new_id: &str) -> PollResult {
        let mut st = self.lock();
        let Some(job) = st.jobs.get_mut(old_id) else {
            return PollResult::not_found();
        };
        job.superseded_by = Some(new_id.to_string());
        self.write_record(&st, old_id);
        poll_locked(&st, old_id)
    }

    pub fn lookup_answer(&self, job_id: &str) -> AnswerLookup {
        let mut st = self.lock();
        let ttl = Duration::from_millis(self.deps.answerable_ttl_ms);
        let expired = st.jobs.get(job_id).is_some_and(|j| {
            j.status == JobStatus::NeedsContext && j.retired_at.is_some_and(|t| t.elapsed() > ttl)
        });
        if expired {
            // Answerable retention expired: behave exactly as if evicted.
            st.jobs.remove(job_id);
            st.retired.retain(|id| id != job_id);
        }
        let Some(job) = st.jobs.get(job_id) else {
            return AnswerLookup::NotFound;
        };
        let session_id = job.output.as_ref().and_then(|o| o.session_id.clone());
        match session_id {
            Some(session_id) if job.status == JobStatus::NeedsContext => AnswerLookup::Ok {
                session_id,
                resume_context: job.spec.resume_context.clone(),
            },
            _ => AnswerLookup::NotAwaiting { status: job.status },
        }
    }

    pub fn cancel(&self, job_id: &str) -> PollResult {
        let mut st = self.lock();
        let Some(job) = st
            .jobs
            .get_mut(job_id)
            .filter(|j| j.stage != Stage::Terminal)
        else {
            return poll_locked(&st, job_id);
        };
        job.termination = Some(JobStatus::Cancelled);
        job.finalize_abort.store(true, Ordering::SeqCst);
        (job.kill)();
        while st
            .jobs
            .get(job_id)
            .is_some_and(|j| j.stage != Stage::Terminal)
        {
            st = self.cv.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        poll_locked(&st, job_id)
    }

    pub fn wait(&self, job_id: &str, timeout_ms: Option<f64>, o: WaitOpts<'_>) -> PollResult {
        let timeout = ms(clamp_wait(timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT)));
        let ids = [job_id.to_string()];
        let st = self.block(&ids, timeout, o, false, |st| {
            st.jobs
                .get(job_id)
                .is_none_or(|j| j.status != JobStatus::Running)
        });
        poll_locked(&st, job_id)
    }

    pub fn wait_any(
        &self,
        ids: &[String],
        timeout_ms: Option<f64>,
        o: WaitOpts<'_>,
    ) -> WaitAnyResult {
        let timeout = ms(clamp_wait(timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT)));
        let first_done = |st: &State| {
            ids.iter()
                .find(|id| {
                    st.jobs
                        .get(*id)
                        .is_some_and(|j| j.status != JobStatus::Running)
                })
                .cloned()
        };
        let st = self.block(ids, timeout, o, true, |st| {
            // Only known jobs count; with none running there is nothing to wait for.
            first_done(st).is_some() || !ids.iter().any(|id| st.jobs.contains_key(id))
        });
        WaitAnyResult {
            first_done: first_done(&st),
            jobs: ids
                .iter()
                .filter(|id| st.jobs.contains_key(*id))
                .map(|id| (id.clone(), poll_locked(&st, id)))
                .collect(),
        }
    }

    pub fn wait_all(
        &self,
        ids: &[String],
        timeout_ms: Option<f64>,
        o: WaitOpts<'_>,
    ) -> WaitAllResult {
        let timeout = ms(clamp_wait(timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT)));
        // NOT_FOUND ids are not blocking.
        let all_done = |st: &State| {
            ids.iter().all(|id| {
                st.jobs
                    .get(id)
                    .is_none_or(|j| j.status != JobStatus::Running)
            })
        };
        let st = self.block(ids, timeout, o, true, all_done);
        WaitAllResult {
            all_done: all_done(&st),
            jobs: ids
                .iter()
                .filter(|id| st.jobs.contains_key(*id))
                .map(|id| (id.clone(), poll_locked(&st, id)))
                .collect(),
        }
    }

    /// Shutdown: SIGTERM every active child and abort any finalize in flight.
    pub fn kill_all(&self) {
        let st = self.lock();
        for job in st.jobs.values().filter(|j| j.stage != Stage::Terminal) {
            job.finalize_abort.store(true, Ordering::SeqCst);
            (job.kill)();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
