use crate::types::{
    Config, DoctorAccountInfo, DoctorAgentInfo, DoctorModelMenuInfo, DoctorPluginInfo, DoctorReport,
};
use std::process::Command;
use std::time::Duration;

const MAX_BUFFER: usize = 2 * 1024 * 1024;
const PRICES_NOTE: &str =
    "Prices are not checkable via the CLI (about/models/--list-models return ids/labels only).";

#[derive(Clone, Debug)]
pub struct AgentCommandResult {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

pub type RunAgentCommandFn = Box<dyn Fn(&str, &[String]) -> AgentCommandResult + Send + Sync>;

pub fn parse_about(stdout: &str) -> (Option<String>, Option<String>, Option<String>) {
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

pub fn parse_models_list(stdout: &str) -> Vec<String> {
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

pub fn diff_configured_models(configured_ids: &[String], account_ids: &[String]) -> Vec<String> {
    let account: std::collections::HashSet<&String> = account_ids.iter().collect();
    let mut missing: Vec<String> = configured_ids
        .iter()
        .filter(|id| !account.contains(id))
        .cloned()
        .collect();
    missing.sort();
    missing
}

pub fn default_run_agent_command(bin: &str, args: &[String]) -> AgentCommandResult {
    let child = match Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return AgentCommandResult {
                ok: false,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(e.to_string()),
            };
        }
    };
    let pid = child.id() as i32;
    // Once the child is reaped its PID can be reused, so the timer must not fire after that.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let timer_done = std::sync::Arc::clone(&done);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(15_000));
        if !timer_done.load(std::sync::atomic::Ordering::SeqCst) {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
    });
    let output = child.wait_with_output();
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    match output {
        Ok(out) => {
            let mut stdout = out.stdout;
            let mut stderr = out.stderr;
            if stdout.len() > MAX_BUFFER {
                stdout.truncate(MAX_BUFFER);
            }
            if stderr.len() > MAX_BUFFER {
                stderr.truncate(MAX_BUFFER);
            }
            let stdout = String::from_utf8_lossy(&stdout).into_owned();
            let stderr = String::from_utf8_lossy(&stderr).into_owned();
            if out.status.success() {
                AgentCommandResult {
                    ok: true,
                    stdout,
                    stderr,
                    error: None,
                }
            } else {
                AgentCommandResult {
                    ok: false,
                    stdout,
                    stderr: stderr.clone(),
                    error: Some(
                        out.status
                            .code()
                            .map(|c| format!("exit {c}"))
                            .unwrap_or_else(|| "command failed".into()),
                    ),
                }
            }
        }
        Err(e) => AgentCommandResult {
            ok: false,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(e.to_string()),
        },
    }
}

pub fn probe_agent_version(
    bin: &str,
    run_command: &dyn Fn(&str, &[String]) -> AgentCommandResult,
) -> (Option<String>, Option<String>) {
    let r = run_command(bin, &["--version".into()]);
    if !r.ok {
        let err = r.error.clone().unwrap_or_else(|| {
            let t = r.stderr.trim();
            if t.is_empty() {
                "cursor-agent --version failed".into()
            } else {
                t.to_string()
            }
        });
        return (None, Some(err));
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

pub fn probe_account(
    bin: &str,
    run_command: &dyn Fn(&str, &[String]) -> AgentCommandResult,
) -> DoctorAccountInfo {
    let r = run_command(bin, &["about".into()]);
    if !r.ok {
        let err = r.error.clone().unwrap_or_else(|| {
            let t = r.stderr.trim();
            if t.is_empty() {
                "cursor-agent about failed".into()
            } else {
                t.to_string()
            }
        });
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

pub fn probe_model_menu(
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
        let err = list.error.clone().unwrap_or_else(|| {
            let t = list.stderr.trim();
            if t.is_empty() {
                "cursor-agent models/--list-models failed".into()
            } else {
                t.to_string()
            }
        });
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

pub fn default_read_package_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").into())
}

pub struct RunDoctorOpts<'a> {
    pub config: &'a Config,
    pub resolve_bin: Option<&'a dyn Fn(Option<&str>) -> String>,
    pub bin_exists: Option<&'a dyn Fn(&str) -> bool>,
    pub run_command: Option<&'a dyn Fn(&str, &[String]) -> AgentCommandResult>,
    pub read_package_version: Option<&'a dyn Fn() -> Result<String, String>>,
}

pub fn run_doctor(opts: RunDoctorOpts<'_>) -> DoctorReport {
    let resolve_bin = |o: Option<&str>| {
        if let Some(f) = opts.resolve_bin {
            f(o)
        } else {
            crate::cursor_bin::resolve_cursor_bin(o)
        }
    };
    let bin_exists = |p: &str| {
        if let Some(f) = opts.bin_exists {
            f(p)
        } else {
            std::path::Path::new(p).exists()
        }
    };
    let run_command = |bin: &str, args: &[String]| {
        if let Some(f) = opts.run_command {
            f(bin, args)
        } else {
            default_run_agent_command(bin, args)
        }
    };
    let read_pkg = || {
        if let Some(f) = opts.read_package_version {
            f()
        } else {
            default_read_package_version()
        }
    };

    let mut warnings = Vec::new();
    let mut failures = Vec::new();
    let mut plugin_version = "unknown".to_string();
    match read_pkg() {
        Ok(v) => plugin_version = v,
        Err(e) => failures.push(format!("failed to read plugin version: {e}")),
    }

    let path = resolve_bin(None);
    // Only cursor-backend models are checked against cursor-agent. Models on
    // backends that are not implemented yet are reported as skips by the CLI.
    let configured_ids: Vec<String> = opts
        .config
        .models
        .iter()
        .filter(|(_, e)| e.backend == "cursor")
        .map(|(id, _)| id.clone())
        .collect();

    if !bin_exists(&path) {
        failures.push(format!("cursor-agent not found at {path}"));
        return DoctorReport {
            ok: false,
            plugin: DoctorPluginInfo {
                version: plugin_version,
            },
            agent: DoctorAgentInfo {
                found: false,
                path: Some(path.clone()),
                version: None,
                error: Some(format!("cursor-agent not found at {path}")),
            },
            account: DoctorAccountInfo {
                logged_in: false,
                email: None,
                subscription: None,
                current_model: None,
                error: Some("skipped: cursor-agent not found".into()),
            },
            model_menu: DoctorModelMenuInfo {
                configured_ids: {
                    let mut c = configured_ids;
                    c.sort();
                    c
                },
                account_ids: None,
                missing_from_account: vec![],
                prices_checkable: false,
                note: PRICES_NOTE.into(),
                error: Some("skipped: cursor-agent not found".into()),
            },
            warnings,
            failures,
        };
    }

    let (ver, ver_err) = probe_agent_version(&path, &run_command);
    if let Some(e) = &ver_err {
        failures.push(format!("cursor-agent --version failed: {e}"));
    }
    let account = probe_account(&path, &run_command);
    if !account.logged_in {
        failures.push(match &account.error {
            Some(e) => format!("not logged in to cursor-agent: {e}"),
            None => "not logged in to cursor-agent (about missing email)".into(),
        });
    }
    let model_menu = probe_model_menu(&path, &configured_ids, &run_command);
    if let Some(e) = &model_menu.error {
        warnings.push(format!("model menu check failed: {e}"));
    }
    for id in &model_menu.missing_from_account {
        warnings.push(format!("configured model not on account: {id}"));
    }

    DoctorReport {
        ok: failures.is_empty(),
        plugin: DoctorPluginInfo {
            version: plugin_version,
        },
        agent: DoctorAgentInfo {
            found: true,
            path: Some(path),
            version: ver,
            error: ver_err,
        },
        account,
        model_menu,
        warnings,
        failures,
    }
}

#[cfg(test)]
mod tests {
    include!("doctor_tests.rs");
}
