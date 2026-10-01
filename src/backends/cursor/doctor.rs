//! cursor-agent doctor probes: `--version`, `about`, and `models`/`--list-models`.

use super::resolve_bin;
use crate::cli_info::status_line;
use crate::doctor::{AgentCommandResult, RunDoctorOpts, command_err, default_run_agent_command};
use crate::types::{DoctorAccountInfo, DoctorAgentInfo, DoctorModelMenuInfo, DoctorReport};

const PRICES_NOTE: &str =
    "Prices are not checkable via the CLI (about/models/--list-models return ids/labels only).";

pub(crate) fn fill(report: &mut DoctorReport, opts: &RunDoctorOpts<'_>) {
    let path = match opts.resolve_bin {
        Some(f) => f(None),
        None => resolve_bin(None),
    };
    let bin_exists = |p: &str| match opts.bin_exists {
        Some(f) => f(p),
        None => std::path::Path::new(p).exists(),
    };
    let run_command = |bin: &str, args: &[String]| match opts.run_command {
        Some(f) => f(bin, args),
        None => default_run_agent_command(bin, args),
    };

    // Only this backend's models are checked. Others are skip lines in the CLI.
    let configured_ids: Vec<String> = opts
        .config
        .models
        .iter()
        .filter(|(_, e)| e.backend == "cursor")
        .map(|(id, _)| id.clone())
        .collect();

    if !bin_exists(&path) {
        report
            .failures
            .push(format!("cursor-agent not found at {path}"));
        let mut ids = configured_ids;
        ids.sort();
        report.agent = DoctorAgentInfo {
            found: false,
            path: Some(path.clone()),
            version: None,
            error: Some(format!("cursor-agent not found at {path}")),
        };
        report.account = DoctorAccountInfo {
            logged_in: false,
            email: None,
            subscription: None,
            current_model: None,
            error: Some("skipped: cursor-agent not found".into()),
        };
        report.model_menu = DoctorModelMenuInfo {
            configured_ids: ids,
            account_ids: None,
            missing_from_account: vec![],
            prices_checkable: false,
            note: PRICES_NOTE.into(),
            error: Some("skipped: cursor-agent not found".into()),
        };
        return;
    }

    let (ver, ver_err) = probe_agent_version(&path, &run_command);
    if let Some(e) = &ver_err {
        report
            .failures
            .push(format!("cursor-agent --version failed: {e}"));
    }
    let account = probe_account(&path, &run_command);
    if !account.logged_in {
        report.failures.push(match &account.error {
            Some(e) => format!("not logged in to cursor-agent: {e}"),
            None => "not logged in to cursor-agent (about missing email)".into(),
        });
    }
    let model_menu = probe_model_menu(&path, &configured_ids, &run_command);
    if let Some(e) = &model_menu.error {
        report
            .warnings
            .push(format!("model menu check failed: {e}"));
    }
    for id in &model_menu.missing_from_account {
        report
            .warnings
            .push(format!("configured model not on account: {id}"));
    }
    report.agent = DoctorAgentInfo {
        found: true,
        path: Some(path),
        version: ver,
        error: ver_err,
    };
    report.account = account;
    report.model_menu = model_menu;
}

pub(crate) fn lines(report: &DoctorReport) -> (String, bool) {
    let mut text = String::new();
    let mut failed = false;
    if !report.agent.found {
        text += &status_line("fail", "cursor: cursor-agent not found");
        failed = true;
    } else {
        let path = report.agent.path.as_deref().unwrap_or("?");
        match report.agent.version.as_deref() {
            Some(v) => {
                text += &status_line("ok", &format!("cursor: cursor-agent {v} ({path})"));
            }
            None => {
                let e = report.agent.error.as_deref().unwrap_or("unknown error");
                text += &status_line(
                    "fail",
                    &format!("cursor: cursor-agent --version failed: {e}"),
                );
                failed = true;
            }
        }
        if report.account.logged_in {
            text += &status_line("ok", "cursor: logged in");
        } else {
            let e = report
                .account
                .error
                .as_deref()
                .unwrap_or("no email in about output");
            text += &status_line("fail", &format!("cursor: not logged in: {e}"));
            failed = true;
        }
        if let Some(e) = report.model_menu.error.as_deref() {
            text += &status_line("warn", &format!("cursor: model list check failed: {e}"));
        }
        for id in &report.model_menu.missing_from_account {
            text += &status_line(
                "warn",
                &format!("cursor: model {id} missing from cursor-agent --list-models"),
            );
        }
    }
    (text, failed)
}

pub(crate) fn parse_about(stdout: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut fields = std::collections::HashMap::new();
    for line in stdout.split('\n') {
        let line = line.trim_end_matches('\r');
        if let Some((k, v)) = parse_about_line(line) {
            fields.insert(k.to_lowercase(), v);
        }
    }
    let get = |keys: &[&str]| -> Option<String> {
        for k in keys {
            if let Some(v) = fields.get(*k)
                && !v.is_empty()
            {
                return Some(v.clone());
            }
        }
        None
    };
    (
        get(&["user email", "email"]),
        get(&["subscription tier", "subscription", "plan", "tier"]),
        get(&["model", "current model"]),
    )
}

fn parse_about_line(line: &str) -> Option<(String, String)> {
    // /^(\S.*?\S)\s{2,}(\S.*?)\s*$/
    let line = line.trim_end();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            let start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i - start >= 2 && start > 0 && i < chars.len() && !chars[i].is_whitespace() {
                let field: String = chars[..start].iter().collect();
                let value: String = chars[i..].iter().collect();
                let field = field.trim();
                if !field.is_empty() {
                    return Some((field.to_string(), value.trim_end().to_string()));
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

pub(crate) fn parse_models_list(stdout: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in stdout.split('\n') {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.to_lowercase().starts_with("available models") {
            continue;
        }
        if trimmed.chars().all(|c| c == '-') && !trimmed.is_empty() {
            continue;
        }
        if let Some(id) = model_id_prefix(trimmed)
            && seen.insert(id.to_string())
        {
            ids.push(id.to_string());
        }
    }
    ids
}

fn model_id_prefix(s: &str) -> Option<&str> {
    let mut chars = s.chars();
    let first = chars.next()?;
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return None;
    }
    let mut i = first.len_utf8();
    for c in chars {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' || c == '-' {
            i += c.len_utf8();
        } else {
            break;
        }
    }
    Some(&s[..i])
}

pub(crate) fn diff_configured_models(
    configured_ids: &[String],
    account_ids: &[String],
) -> Vec<String> {
    let account: std::collections::HashSet<&String> = account_ids.iter().collect();
    let mut missing: Vec<String> = configured_ids
        .iter()
        .filter(|id| !account.contains(id))
        .cloned()
        .collect();
    missing.sort();
    missing
}

pub(crate) fn probe_agent_version(
    bin: &str,
    run_command: &dyn Fn(&str, &[String]) -> AgentCommandResult,
) -> (Option<String>, Option<String>) {
    let r = run_command(bin, &["--version".into()]);
    if !r.ok {
        return (None, Some(command_err("cursor-agent", &r, "--version")));
    }
    let version = r.stdout.trim();
    (
        if version.is_empty() {
            None
        } else {
            Some(version.to_string())
        },
        None,
    )
}

pub(crate) fn probe_account(
    bin: &str,
    run_command: &dyn Fn(&str, &[String]) -> AgentCommandResult,
) -> DoctorAccountInfo {
    let r = run_command(bin, &["about".into()]);
    if !r.ok {
        let err = command_err("cursor-agent", &r, "about");
        return DoctorAccountInfo {
            logged_in: false,
            email: None,
            subscription: None,
            current_model: None,
            error: Some(err),
        };
    }
    let (email, subscription, current_model) = parse_about(&r.stdout);
    DoctorAccountInfo {
        logged_in: email.is_some(),
        email,
        subscription,
        current_model,
        error: None,
    }
}

pub(crate) fn probe_model_menu(
    bin: &str,
    configured_ids: &[String],
    run_command: &dyn Fn(&str, &[String]) -> AgentCommandResult,
) -> DoctorModelMenuInfo {
    let mut sorted = configured_ids.to_vec();
    sorted.sort();
    let base_note = PRICES_NOTE.to_string();
    let mut list = run_command(bin, &["models".into()]);
    if !list.ok {
        list = run_command(bin, &["--list-models".into()]);
    }
    if !list.ok {
        let err = command_err("cursor-agent", &list, "models/--list-models");
        return DoctorModelMenuInfo {
            configured_ids: sorted,
            account_ids: None,
            missing_from_account: vec![],
            prices_checkable: false,
            note: base_note,
            error: Some(err),
        };
    }
    let account_ids = parse_models_list(&list.stdout);
    let missing = diff_configured_models(&sorted, &account_ids);
    DoctorModelMenuInfo {
        configured_ids: sorted,
        account_ids: Some(account_ids),
        missing_from_account: missing,
        prices_checkable: false,
        note: base_note,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    include!("doctor_tests.rs");
}
