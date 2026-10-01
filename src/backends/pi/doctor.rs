//! pi doctor probes: `pi --version`, and `pi auth check --model <id>`
//! for each configured pi model.

use super::resolve_bin;
use crate::cli_info::status_line;
use crate::doctor::{AgentCommandResult, RunDoctorOpts};
use crate::types::{DoctorBackendSection, DoctorReport};

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
        None => crate::doctor::default_run_agent_command(bin, args),
    };

    // Only this backend's models are checked. Others are skip lines in the CLI.
    let mut configured_ids: Vec<String> = opts
        .config
        .models
        .iter()
        .filter(|(_, e)| e.backend == "pi")
        .map(|(id, _)| id.clone())
        .collect();
    configured_ids.sort();

    if !bin_exists(&path) {
        report.failures.push(format!("pi not found at {path}"));
        report.sections.push(DoctorBackendSection {
            backend: "pi".into(),
            found: false,
            path: Some(path),
            ..Default::default()
        });
        return;
    }

    let (ver, ver_err) = probe_version(&path, &run_command);
    if let Some(e) = &ver_err {
        report.failures.push(format!("pi --version failed: {e}"));
    }
    let mut model_failures = Vec::new();
    for id in &configured_ids {
        let r = run_command(
            &path,
            &["auth".into(), "check".into(), "--model".into(), id.clone()],
        );
        if !r.ok {
            let err = command_err(&r, &format!("auth check --model {id}"));
            model_failures.push(format!("model {id} auth check failed: {err}"));
            report
                .warnings
                .push(format!("pi: model {id} auth check failed: {err}"));
        }
    }
    report.sections.push(DoctorBackendSection {
        backend: "pi".into(),
        found: true,
        path: Some(path),
        version: ver,
        version_error: ver_err,
        model_failures,
    });
}

pub(crate) fn lines(report: &DoctorReport) -> (String, bool) {
    // No section means no pi models in the table: nothing to say.
    let Some(sec) = report.sections.iter().find(|s| s.backend == "pi") else {
        return (String::new(), false);
    };
    let mut text = String::new();
    let mut failed = false;
    if !sec.found {
        text += &status_line("fail", "pi: pi not found");
        return (text, true);
    }
    let path = sec.path.as_deref().unwrap_or("?");
    match sec.version.as_deref() {
        Some(v) => {
            text += &status_line("ok", &format!("pi: pi {v} ({path})"));
        }
        None => {
            let e = sec.version_error.as_deref().unwrap_or("unknown error");
            text += &status_line("fail", &format!("pi: pi --version failed: {e}"));
            failed = true;
        }
    }
    for m in &sec.model_failures {
        text += &status_line("warn", &format!("pi: {m}"));
    }
    (text, failed)
}

fn command_err(r: &AgentCommandResult, what: &str) -> String {
    r.error.clone().unwrap_or_else(|| {
        let t = r.stderr.trim();
        if t.is_empty() {
            format!("pi {what} failed")
        } else {
            t.to_string()
        }
    })
}

pub(crate) fn probe_version(
    bin: &str,
    run_command: &dyn Fn(&str, &[String]) -> AgentCommandResult,
) -> (Option<String>, Option<String>) {
    let r = run_command(bin, &["--version".into()]);
    if !r.ok {
        return (None, Some(command_err(&r, "--version")));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Config, HostProfile, ModelEntry, Price};
    use std::collections::HashMap;

    fn config_with_pi() -> Config {
        let mut models: HashMap<String, ModelEntry> = HashMap::new();
        models.insert(
            "openai-codex/gpt-6-luna".into(),
            ModelEntry {
                label: "GPT-6 Luna".into(),
                backend: "pi".into(),
                price: Price {
                    input: 1.0,
                    output: 2.0,
                    cache_read: 0.0,
                    cache_write: 0.0,
                },
            },
        );
        Config {
            default: "openai-codex/gpt-6-luna".into(),
            price_map: models.iter().map(|(k, v)| (k.clone(), v.price)).collect(),
            models,
            profile: HostProfile::default(),
        }
    }

    fn ok_result(stdout: &str) -> AgentCommandResult {
        AgentCommandResult {
            ok: true,
            stdout: stdout.into(),
            stderr: String::new(),
            error: None,
        }
    }

    fn err_result() -> AgentCommandResult {
        AgentCommandResult {
            ok: false,
            stdout: String::new(),
            stderr: String::new(),
            error: Some("exit 1".into()),
        }
    }

    fn empty_report() -> DoctorReport {
        DoctorReport {
            ok: false,
            plugin: crate::types::DoctorPluginInfo {
                version: "1.0.0".into(),
            },
            agent: crate::types::DoctorAgentInfo {
                found: false,
                path: None,
                version: None,
                error: None,
            },
            account: crate::types::DoctorAccountInfo {
                logged_in: false,
                email: None,
                subscription: None,
                current_model: None,
                error: None,
            },
            model_menu: crate::types::DoctorModelMenuInfo {
                configured_ids: vec![],
                account_ids: None,
                missing_from_account: vec![],
                prices_checkable: false,
                note: String::new(),
                error: None,
            },
            sections: vec![],
            warnings: vec![],
            failures: vec![],
        }
    }

    #[test]
    fn fill_ok_and_auth_failure_is_a_warning() {
        let config = config_with_pi();
        let mut report = empty_report();
        let opts = RunDoctorOpts {
            config: &config,
            resolve_bin: Some(&|_| "/bin/pi".into()),
            bin_exists: Some(&|_| true),
            run_command: Some(&|_: &str, args: &[String]| {
                if args == ["--version"] {
                    ok_result("0.99.2\n")
                } else {
                    assert_eq!(
                        args,
                        &["auth", "check", "--model", "openai-codex/gpt-6-luna"]
                    );
                    err_result()
                }
            }),
            read_package_version: None,
        };
        fill(&mut report, &opts);
        assert!(report.failures.is_empty());
        assert_eq!(report.warnings.len(), 1);
        let (text, failed) = lines(&report);
        assert!(!failed);
        assert!(text.contains("ok    pi: pi 0.99.2 (/bin/pi)"), "{text}");
        assert!(
            text.contains("warn  pi: model openai-codex/gpt-6-luna auth check failed: exit 1"),
            "{text}"
        );
    }

    #[test]
    fn fill_missing_binary_is_a_failure() {
        let config = config_with_pi();
        let mut report = empty_report();
        let opts = RunDoctorOpts {
            config: &config,
            resolve_bin: Some(&|_| "/nowhere/pi".into()),
            bin_exists: Some(&|_| false),
            run_command: None,
            read_package_version: None,
        };
        fill(&mut report, &opts);
        assert_eq!(report.failures.len(), 1);
        let (text, failed) = lines(&report);
        assert!(failed);
        assert!(text.contains("fail  pi: pi not found"), "{text}");
    }

    #[test]
    fn lines_empty_without_a_pi_section() {
        let report = empty_report();
        assert_eq!(lines(&report), (String::new(), false));
    }
}
