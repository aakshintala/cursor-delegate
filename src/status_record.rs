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
