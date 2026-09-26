use crate::types::{JobSpec, RawCursorJson};

/// What the backend returns when the child exits.
#[derive(Debug, Clone, Default)]
pub struct BackendResult {
    pub raw: RawCursorJson,
    pub clean_exit: bool,
    pub stderr: String,
}

/// Snapshot of live progress fields carried by each `Event::Progress`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProgressSnapshotRaw {
    pub last_tool: Option<String>,
    pub tokens_so_far: f64,
    pub last_assistant: Option<String>,
    pub files_touched: Vec<String>,
    pub phase: Option<String>,
}

/// Liveness and progress signals from a running child. `Activity` fires on any raw stdout
/// chunk, even one that doesn't complete a parseable line, so the idle watchdog can treat
/// it as liveness without waiting for a full semantic event.
#[derive(Debug, Clone)]
pub enum Event {
    Progress(ProgressSnapshotRaw),
    Stderr,
    Activity,
}

pub type EventFn<'a> = &'a (dyn Fn(Event) + Sync);

/// A spawned run. `kill` sends SIGTERM and may be called from any thread, any number of
/// times. `drive` blocks the calling thread until the child exits, reporting events.
pub struct Spawned {
    pub kill: Box<dyn Fn() + Send + Sync>,
    pub drive: Box<dyn FnOnce(EventFn<'_>) -> BackendResult + Send>,
}

pub trait Backend: Send + Sync {
    fn run(&self, spec: &JobSpec) -> Spawned;
}
