use crate::types::{JobStatus, PollResult, RunOutput, RunStatus};
use crate::util::{json_compact, random_uuid};
use std::fs;
use std::path::{Path, PathBuf};

pub trait StatusRecordWriter: Send + Sync {
    fn write(&self, job_id: &str, record: &PollResult);
}

pub fn status_record_path(job_id: &str) -> PathBuf {
    std::env::temp_dir()
        .join("cursor-delegate-jobs")
        .join(format!("{job_id}.json"))
}

pub struct FileStatusRecordWriter;

impl StatusRecordWriter for FileStatusRecordWriter {
    fn write(&self, job_id: &str, record: &PollResult) {
        let _ = write_atomic(&status_record_path(job_id), &json_compact(record));
    }
}

/// Write to a unique tmp sibling, then rename over `path`, so readers never see a partial file.
fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp", random_uuid()));
    fs::write(&tmp, contents)
        .and_then(|_| fs::rename(&tmp, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
}

pub fn file_status_record_writer() -> FileStatusRecordWriter {
    FileStatusRecordWriter
}

pub struct NoopStatusWriter;

impl StatusRecordWriter for NoopStatusWriter {
    fn write(&self, _job_id: &str, _record: &PollResult) {}
}

/// Where the `delegate` CLI keeps one job's record (the only source of truth for that job).
pub fn cli_record_path(job_id: &str) -> PathBuf {
    std::env::temp_dir()
        .join("delegate-jobs")
        .join(format!("{job_id}.json"))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliResume {
    pub model: String,
    pub cwd: String,
    pub capability: &'static str,
    pub session_id: Option<String>,
    pub gate: String,
    #[serde(serialize_with = "crate::util::js_num_opt")]
    pub tool_idle_ms: Option<f64>,
}

/// `PollResult` plus what a watcher needs to find and resume the job.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliRecord {
    #[serde(flatten)]
    pub poll: PollResult,
    pub supervisor_pid: u32,
    pub resume: CliResume,
}

/// Writes the CLI record for the one job this supervisor runs. A failed write exits the
/// supervisor: the file is the only truth, so a job that cannot report is not worth running.
/// ponytail: the cursor-agent child is orphaned on that exit; `watch` reports the dead supervisor.
pub struct CliRecordWriter {
    pub job_id: String,
    pub model: String,
    pub cwd: String,
    pub capability: &'static str,
    pub gate: String,
    pub tool_idle_ms: Option<f64>,
}

impl StatusRecordWriter for CliRecordWriter {
    // The registry mints its own id; the CLI id is the one the caller holds.
    fn write(&self, _registry_id: &str, record: &PollResult) {
        let mut poll = record.clone();
        let mut session_id = None;
        if let PollResult::Terminal { result, .. } = &mut poll {
            result.job_id = Some(self.job_id.clone());
            session_id = result.session_id.clone();
        }
        let rec = CliRecord {
            poll,
            supervisor_pid: std::process::id(),
            resume: CliResume {
                model: self.model.clone(),
                cwd: self.cwd.clone(),
                capability: self.capability,
                session_id,
                gate: self.gate.clone(),
                tool_idle_ms: self.tool_idle_ms,
            },
        };
        let path = cli_record_path(&self.job_id);
        let res = write_atomic(&path, &json_compact(&rec));
        if let Err(e) = res {
            eprintln!("cannot write status record {}: {e}", path.display());
            std::process::exit(1);
        }
    }
}

/// A RUNNING record whose supervisor is gone. Keeps resume so a later command can still
/// see what the job was; the result text is the whole reason.
pub fn write_supervisor_died(job_id: &str, prior: &serde_json::Value) -> std::io::Result<()> {
    let resume_in = &prior["resume"];
    let capability = if resume_in["capability"] == "read-write" {
        "read-write"
    } else {
        "read-only"
    };
    let session_id = resume_in["sessionId"].as_str().map(str::to_string);
    let model = resume_in["model"].as_str().unwrap_or("").to_string();
    let rec = CliRecord {
        poll: PollResult::Terminal {
            status: JobStatus::Error,
            result: RunOutput {
                status: RunStatus::Error,
                text: "supervisor died".into(),
                session_id: session_id.clone(),
                backend: "cursor".into(),
                model: model.clone(),
                usage: None,
                cost_usd: None,
                cost_estimated: false,
                duration_ms: None,
                job_id: Some(job_id.to_string()),
                downgraded: None,
                stderr_tail: None,
                gate_result: None,
                change_set: None,
                concerns: None,
            },
            superseded_by: None,
        },
        supervisor_pid: prior["supervisorPid"].as_u64().unwrap_or(0) as u32,
        resume: CliResume {
            model,
            cwd: resume_in["cwd"].as_str().unwrap_or("").to_string(),
            capability,
            session_id,
            gate: resume_in["gate"].as_str().unwrap_or("").to_string(),
            tool_idle_ms: resume_in["toolIdleMs"].as_f64(),
        },
    };
    write_atomic(&cli_record_path(job_id), &json_compact(&rec))
}
