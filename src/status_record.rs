use crate::types::PollResult;
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
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
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

/// `PollResult` plus what a watcher needs to find and resume the job, plus the resume
/// chain links: `resumedFrom` (this job continues that one) and `supersededBy` (written
/// into the old record once the new job is spawned). The flattened `poll` already carries
/// `supersededBy` when the registry set it; the file rewrite for a long-gone supervisor
/// inserts the same top-level key.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliRecord {
    #[serde(flatten)]
    pub poll: PollResult,
    pub supervisor_pid: u32,
    pub resume: CliResume,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_from: Option<String>,
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
    pub resumed_from: Option<String>,
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
            resumed_from: self.resumed_from.clone(),
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
    let mut rec = prior.clone();
    let Some(obj) = rec.as_object_mut() else {
        return Err(std::io::Error::other("status record is not an object"));
    };
    obj.insert("status".into(), "ERROR".into());
    obj.remove("lastHeartbeatAt");
    obj.remove("progress");
    let resume = &prior["resume"];
    obj.insert(
        "result".into(),
        serde_json::json!({
            "status": "ERROR",
            "text": "supervisor died",
            "sessionId": resume["sessionId"].clone(),
            "backend": "cursor",
            "model": resume["model"].as_str().unwrap_or(""),
            "usage": null,
            "costUsd": null,
            "costEstimated": false,
            "durationMs": null,
            "jobId": job_id,
        }),
    );
    write_atomic(&cli_record_path(job_id), &json_compact(&rec))
}

/// A RUNNING record whose supervisor is gone or had to be SIGKILLed: nobody is left to
/// finalize, so `cancel` writes the CANCELLED terminal record itself. Mutates a copy of
/// the prior record: the status and result flip, the session (which never produced a
/// result) is nulled, and everything else — resume, pids, chain links — is kept.
pub fn write_cancelled(job_id: &str, prior: &serde_json::Value) -> std::io::Result<()> {
    let mut rec = prior.clone();
    let Some(obj) = rec.as_object_mut() else {
        return Err(std::io::Error::other("status record is not an object"));
    };
    let model = obj
        .get("resume")
        .and_then(|r| r.get("model"))
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    obj.insert("status".into(), "CANCELLED".into());
    obj.remove("lastHeartbeatAt");
    obj.remove("progress");
    obj.insert(
        "result".into(),
        serde_json::json!({
            "status": "CANCELLED",
            "text": "Cancelled by user.",
            "sessionId": null,
            "backend": "cursor",
            "model": model,
            "usage": null,
            "costUsd": null,
            "costEstimated": true,
            "durationMs": null,
            "jobId": job_id,
        }),
    );
    if let Some(resume) = obj.get_mut("resume").and_then(|r| r.as_object_mut()) {
        resume.insert("sessionId".into(), serde_json::Value::Null);
    }
    write_atomic(&cli_record_path(job_id), &json_compact(&rec))
}
