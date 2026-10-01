use crate::types::PollResult;
use crate::util::{json_compact, random_uuid};
use std::fs;
use std::path::PathBuf;

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
        let file_path = status_record_path(job_id);
        if let Some(dir) = file_path.parent() {
            let _ = fs::create_dir_all(dir);
            let tmp = dir.join(format!(
                ".{}.{}.{}.tmp",
                job_id,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            ));
            // unique-enough tmp even if millis collide
            let tmp = if tmp.exists() {
                dir.join(format!(".{}.{}.tmp", job_id, random_uuid()))
            } else {
                tmp
            };
            if fs::write(&tmp, json_compact(record)).is_ok() {
                let _ = fs::rename(&tmp, &file_path);
            } else {
                let _ = fs::remove_file(&tmp);
            }
        }
    }
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
/// ponytail: the cursor-agent child is orphaned on that exit; supervisor death handling is #9.
pub struct CliRecordWriter {
    pub job_id: String,
    pub model: String,
    pub cwd: String,
    pub capability: &'static str,
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
            },
        };
        let path = cli_record_path(&self.job_id);
        let tmp = path.with_extension(format!("{}.tmp", random_uuid()));
        let res = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&tmp, json_compact(&rec)))
            .and_then(|_| fs::rename(&tmp, &path));
        if let Err(e) = res {
            eprintln!("cannot write status record {}: {e}", path.display());
            std::process::exit(1);
        }
    }
}
