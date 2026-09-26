use crate::backends::types::BackendResult;
use crate::types::{RUN_STATUSES, RunOutput, RunStatus, Usage};

pub fn derive_status(text: &str, raw_is_error: Option<bool>, clean_exit: bool) -> RunStatus {
    let lines: Vec<&str> = text
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .collect();
    let last = lines.last().copied().unwrap_or("");
    // JS split('\n') keeps `\r` on CRLF lines; match that, then the regex.
    let last = last.trim_end_matches('\r');
    if let Some(caps) = status_line(last)
        && RUN_STATUSES.contains(&caps)
        && let Some(s) = RunStatus::parse(caps)
    {
        return s;
    }
    if raw_is_error == Some(false) && clean_exit {
        return RunStatus::Done;
    }
    RunStatus::Error
}

fn status_line(last: &str) -> Option<&str> {
    // /^STATUS:\s*([A-Z_]+)\s*$/
    let rest = last.strip_prefix("STATUS:")?;
    let rest = rest.trim_start_matches([' ', '\t']);
    let token = rest.trim_end();
    if token.chars().all(|c| c.is_ascii_uppercase() || c == '_') && !token.is_empty() {
        Some(token)
    } else {
        None
    }
}

pub fn to_run_output(
    res: &BackendResult,
    model: &str,
    backend: &str,
    usage: Option<Usage>,
    cost_usd: Option<f64>,
) -> RunOutput {
    let text = res.raw.result.clone().unwrap_or_default();
    RunOutput {
        status: derive_status(&text, res.raw.is_error, res.clean_exit),
        text,
        session_id: res.raw.session_id.clone(),
        backend: backend.to_string(),
        model: model.to_string(),
        usage,
        cost_usd,
        cost_estimated: true,
        duration_ms: res.raw.duration_ms,
        job_id: None,
        downgraded: None,
        stderr_tail: None,
        gate_result: None,
        change_set: None,
        concerns: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::types::BackendResult;
    use crate::types::RawCursorJson;

    #[test]
    fn explicit_trailing_status_wins() {
        assert_eq!(
            derive_status("work\nSTATUS: BLOCKED", Some(false), true),
            RunStatus::Blocked
        );
        assert_eq!(
            derive_status("STATUS: NEEDS_CONTEXT\n", Some(false), true),
            RunStatus::NeedsContext
        );
    }

    #[test]
    fn only_final_status_line_wins() {
        assert_eq!(
            derive_status(
                "STATUS: NEEDS_CONTEXT\nmore work\nSTATUS: DONE",
                Some(false),
                true
            ),
            RunStatus::Done
        );
        assert_eq!(
            derive_status(
                "mentioned STATUS: NEEDS_CONTEXT in prose\nall good",
                Some(false),
                true
            ),
            RunStatus::Done
        );
    }

    #[test]
    fn unknown_status_token_falls_through() {
        assert_eq!(
            derive_status("STATUS: WHATEVER", Some(false), true),
            RunStatus::Done
        );
    }

    #[test]
    fn clean_exit_no_error_is_done() {
        assert_eq!(derive_status("done", Some(false), true), RunStatus::Done);
    }

    #[test]
    fn error_or_non_clean_is_error() {
        assert_eq!(derive_status("oops", Some(true), true), RunStatus::Error);
        assert_eq!(derive_status("oops", Some(false), false), RunStatus::Error);
    }

    #[test]
    fn precedence_table() {
        for s in [
            "DONE",
            "DONE_WITH_CONCERNS",
            "BLOCKED",
            "NEEDS_CONTEXT",
            "ERROR",
        ] {
            assert_eq!(
                derive_status(&format!("work\nSTATUS: {s}"), Some(true), false).as_str(),
                s
            );
        }
    }

    #[test]
    fn explicit_status_wins_even_when_is_error() {
        assert_eq!(
            derive_status("partial\nSTATUS: DONE", Some(true), true),
            RunStatus::Done
        );
    }

    #[test]
    fn trailing_blank_and_crlf() {
        assert_eq!(
            derive_status("work\nSTATUS: BLOCKED\n\n  \n", Some(false), true),
            RunStatus::Blocked
        );
        assert_eq!(
            derive_status("work\r\nSTATUS: DONE\r\n", Some(false), true),
            RunStatus::Done
        );
    }

    #[test]
    fn empty_text_falls_through() {
        assert_eq!(derive_status("", Some(false), true), RunStatus::Done);
        assert_eq!(derive_status("   \n  ", Some(false), true), RunStatus::Done);
        assert_eq!(derive_status("", Some(true), false), RunStatus::Error);
    }

    #[test]
    fn to_run_output_maps_raw() {
        let res = BackendResult {
            raw: RawCursorJson {
                result: Some("hi\nSTATUS: DONE".into()),
                session_id: Some("s1".into()),
                duration_ms: Some(1234.0),
                ..RawCursorJson::default()
            },
            clean_exit: true,
            stderr: String::new(),
        };
        let out = to_run_output(&res, "composer-2.5", "cursor", None, None);
        assert_eq!(out.status, RunStatus::Done);
        assert_eq!(out.text, "hi\nSTATUS: DONE");
        assert_eq!(out.session_id.as_deref(), Some("s1"));
        assert_eq!(out.duration_ms, Some(1234.0));
        assert!(out.cost_estimated);
        assert_eq!(out.backend, "cursor");
    }
}
