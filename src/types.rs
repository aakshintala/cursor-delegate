use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Ask,
    Plan,
    Write,
    #[serde(rename = "write-unsandboxed")]
    WriteUnsandboxed,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Plan => "plan",
            Self::Write => "write",
            Self::WriteUnsandboxed => "write-unsandboxed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ask" => Some(Self::Ask),
            "plan" => Some(Self::Plan),
            "write" => Some(Self::Write),
            "write-unsandboxed" => Some(Self::WriteUnsandboxed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Isolation {
    None,
    CallerProvided {
        path: String,
    },
    BackendProvided {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        base: Option<String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct RunInput {
    pub prompt: String,
    pub model: Option<String>,
    pub require_non_claude: Option<bool>,
    pub capability: Option<Capability>,
    pub allow_unsandboxed: Option<bool>,
    pub session: Option<String>,
    pub isolation: Option<Isolation>,
    pub verify_commands: Option<Vec<String>>,
    pub gate: Option<String>,
    pub allow_partial_commit: Option<bool>,
    pub wait_ms: Option<f64>,
    pub idle_ms: Option<Option<f64>>,
    pub tool_idle_ms: Option<Option<f64>>,
    pub background: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(serialize_with = "crate::util::js_num")]
    #[serde(default)]
    pub input_tokens: f64,
    #[serde(serialize_with = "crate::util::js_num")]
    #[serde(default)]
    pub output_tokens: f64,
    #[serde(serialize_with = "crate::util::js_num")]
    #[serde(default)]
    pub cache_read_tokens: f64,
    #[serde(serialize_with = "crate::util::js_num")]
    #[serde(default)]
    pub cache_write_tokens: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    #[serde(rename = "DONE")]
    Done,
    #[serde(rename = "DONE_WITH_CONCERNS")]
    DoneWithConcerns,
    #[serde(rename = "BLOCKED")]
    Blocked,
    #[serde(rename = "NEEDS_CONTEXT")]
    NeedsContext,
    #[serde(rename = "ERROR")]
    Error,
}

pub const RUN_STATUSES: [&str; 5] = [
    "DONE",
    "DONE_WITH_CONCERNS",
    "BLOCKED",
    "NEEDS_CONTEXT",
    "ERROR",
];

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Done => "DONE",
            Self::DoneWithConcerns => "DONE_WITH_CONCERNS",
            Self::Blocked => "BLOCKED",
            Self::NeedsContext => "NEEDS_CONTEXT",
            Self::Error => "ERROR",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "DONE" => Some(Self::Done),
            "DONE_WITH_CONCERNS" => Some(Self::DoneWithConcerns),
            "BLOCKED" => Some(Self::Blocked),
            "NEEDS_CONTEXT" => Some(Self::NeedsContext),
            "ERROR" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub head_before: Option<String>,
    pub head_after: Option<String>,
    pub new_commits: Vec<String>,
    pub files_changed: Vec<String>,
    pub diffstat: String,
    pub uncommitted_files: Vec<String>,
    pub dirty_after: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateResult {
    pub command: String,
    pub exit_code: i32,
    pub passed: bool,
    pub output_tail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutput {
    pub status: RunStatus,
    pub text: String,
    pub session_id: Option<String>,
    pub backend: String,
    pub model: String,
    pub usage: Option<Usage>,
    #[serde(serialize_with = "crate::util::js_num_opt")]
    pub cost_usd: Option<f64>,
    pub cost_estimated: bool,
    #[serde(serialize_with = "crate::util::js_num_opt")]
    pub duration_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downgraded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_result: Option<GateResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_set: Option<ChangeSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concerns: Option<Vec<String>>,
}

/// Each field decodes on its own: one malformed field reads as absent instead of voiding the rest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawCursorJson {
    #[serde(
        default,
        deserialize_with = "crate::util::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub r#type: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::util::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub subtype: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::util::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_error: Option<bool>,
    #[serde(
        default,
        deserialize_with = "crate::util::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_ms: Option<f64>,
    #[serde(
        default,
        deserialize_with = "crate::util::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_api_ms: Option<f64>,
    #[serde(
        default,
        deserialize_with = "crate::util::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub result: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::util::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub session_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::util::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub request_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::util::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    #[serde(rename = "cacheRead")]
    pub cache_read: f64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: f64,
}

pub type PriceMap = std::collections::HashMap<String, Price>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub label: String,
    pub family: String,
    pub price: Price,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedModel {
    pub model: String,
    pub family: String,
    pub price: Price,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostProfile {
    pub default: Option<String>,
    pub models: Option<std::collections::HashMap<String, ModelEntry>>,
    pub required_deny: Option<Vec<String>>,
    pub prompt_preamble: Option<String>,
    pub verify_commands: Option<Vec<String>>,
    pub gate: Option<String>,
    pub deadline_ms: Option<f64>,
    pub idle_ms: Option<Option<f64>>,
    pub tool_idle_ms: Option<Option<f64>>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub default: String,
    pub models: std::collections::HashMap<String, ModelEntry>,
    pub price_map: PriceMap,
    pub profile: HostProfile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorPluginInfo {
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorAgentInfo {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorAccountInfo {
    pub logged_in: bool,
    pub email: Option<String>,
    pub subscription: Option<String>,
    pub current_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorModelMenuInfo {
    pub configured_ids: Vec<String>,
    pub account_ids: Option<Vec<String>>,
    pub missing_from_account: Vec<String>,
    pub prices_checkable: bool,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRegistrationCheck {
    pub enabled: bool,
    pub reachable: bool,
    pub resolves_to_plugin_install: bool,
    pub legacy_absent: bool,
    pub ok: bool,
    pub detail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub ok: bool,
    pub plugin: DoctorPluginInfo,
    pub agent: DoctorAgentInfo,
    pub account: DoctorAccountInfo,
    pub model_menu: DoctorModelMenuInfo,
    pub plugin_registration: PluginRegistrationCheck,
    pub warnings: Vec<String>,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeContext {
    pub model: String,
    pub require_non_claude: Option<bool>,
    pub capability: Capability,
    pub allow_unsandboxed: bool,
    pub isolation: Isolation,
    pub verify_commands: Option<Vec<String>>,
    pub gate: String,
    pub allow_partial_commit: bool,
}

#[derive(Debug, Clone)]
pub struct JobSpec {
    pub bin: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub model: String,
    pub backend: String,
    pub is_write: bool,
    pub path: Option<String>,
    pub head_before: Option<String>,
    pub gate: String,
    pub allow_partial_commit: bool,
    pub wait_ms: Option<f64>,
    pub idle_ms: Option<Option<f64>>,
    pub tool_idle_ms: Option<Option<f64>>,
    pub background: Option<bool>,
    pub price_map: PriceMap,
    pub downgraded: bool,
    pub worktree_name: Option<String>,
    pub resume_context: ResumeContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    #[serde(rename = "RUNNING")]
    Running,
    #[serde(rename = "DONE")]
    Done,
    #[serde(rename = "DONE_WITH_CONCERNS")]
    DoneWithConcerns,
    #[serde(rename = "BLOCKED")]
    Blocked,
    #[serde(rename = "NEEDS_CONTEXT")]
    NeedsContext,
    #[serde(rename = "ERROR")]
    Error,
    #[serde(rename = "CANCELLED")]
    Cancelled,
    #[serde(rename = "STALLED")]
    Stalled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Done => "DONE",
            Self::DoneWithConcerns => "DONE_WITH_CONCERNS",
            Self::Blocked => "BLOCKED",
            Self::NeedsContext => "NEEDS_CONTEXT",
            Self::Error => "ERROR",
            Self::Cancelled => "CANCELLED",
            Self::Stalled => "STALLED",
        }
    }

    pub fn from_run(s: RunStatus) -> Self {
        match s {
            RunStatus::Done => Self::Done,
            RunStatus::DoneWithConcerns => Self::DoneWithConcerns,
            RunStatus::Blocked => Self::Blocked,
            RunStatus::NeedsContext => Self::NeedsContext,
            RunStatus::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressSnapshot {
    pub last_tool: Option<String>,
    #[serde(serialize_with = "crate::util::js_num")]
    pub tokens_so_far: f64,
    #[serde(serialize_with = "crate::util::js_num")]
    pub elapsed_ms: f64,
    pub last_assistant: Option<String>,
    pub files_touched_so_far: Vec<String>,
    pub phase: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PollResult {
    Running {
        status: &'static str,
        #[serde(rename = "lastHeartbeatAt", serialize_with = "crate::util::js_num")]
        last_heartbeat_at: f64,
        #[serde(rename = "supersededBy", skip_serializing_if = "Option::is_none")]
        superseded_by: Option<String>,
        progress: ProgressSnapshot,
    },
    Terminal {
        status: JobStatus,
        result: RunOutput,
        #[serde(rename = "supersededBy", skip_serializing_if = "Option::is_none")]
        superseded_by: Option<String>,
    },
    NotFound {
        status: &'static str,
    },
}

impl PollResult {
    pub fn not_found() -> Self {
        Self::NotFound {
            status: "NOT_FOUND",
        }
    }

    pub fn status_label(&self) -> &str {
        match self {
            Self::Running { .. } => "RUNNING",
            Self::Terminal { status, .. } => status.as_str(),
            Self::NotFound { .. } => "NOT_FOUND",
        }
    }
}

impl PartialEq<&str> for PollResult {
    fn eq(&self, other: &&str) -> bool {
        self.status_label() == *other
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum DispatchResult {
    Output(RunOutput),
    Detached {
        status: &'static str,
        #[serde(rename = "jobId")]
        job_id: String,
        #[serde(rename = "busyPath", skip_serializing_if = "Option::is_none")]
        busy_path: Option<String>,
    },
    Busy {
        status: &'static str,
        #[serde(rename = "jobId")]
        job_id: String,
        #[serde(rename = "busyPath")]
        busy_path: String,
    },
}

impl DispatchResult {
    pub fn status_label(&self) -> &str {
        match self {
            Self::Output(o) => o.status.as_str(),
            Self::Detached { status, .. } => status,
            Self::Busy { status, .. } => status,
        }
    }

    pub fn job_id(&self) -> Option<&str> {
        match self {
            Self::Output(o) => o.job_id.as_deref(),
            Self::Detached { job_id, .. } => Some(job_id),
            Self::Busy { job_id, .. } => Some(job_id),
        }
    }
}

pub struct FinalizeCtx {
    pub cwd: String,
    pub head_before: Option<String>,
    pub is_write: bool,
    pub gate: String,
    pub allow_partial_commit: bool,
    pub model: String,
    pub backend: String,
    pub price_map: PriceMap,
    pub job_id: Option<String>,
    pub downgraded: bool,
    pub run_gate:
        Option<Box<dyn Fn(&str, &str, Option<&crate::util::Abort>) -> GateResult + Send + Sync>>,
    pub signal: Option<crate::util::Abort>,
    pub git_delta: Option<Box<dyn Fn(&str, Option<&str>) -> Option<ChangeSet> + Send + Sync>>,
    pub worktree_name: Option<String>,
    pub resolve_worktree_path:
        Option<Box<dyn Fn(&str, Option<&str>) -> Option<String> + Send + Sync>>,
}
