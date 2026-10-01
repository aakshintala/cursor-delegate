//! Human-readable `delegate models` and `delegate doctor` output.

use crate::doctor::{RunDoctorOpts, run_doctor};
use crate::index::build_deps;
use crate::types::Config;

/// `delegate models`: an aligned table of id, label, backend and $/1M prices.
pub fn models() -> Result<i32, String> {
    let deps = build_deps().map_err(|e| e.to_string())?;
    print!("{}", models_table(&deps.config));
    Ok(0)
}

/// `delegate doctor`: one ok/warn/skip/fail line per check. Exit 1 on any fail.
pub fn doctor() -> Result<i32, String> {
    let deps = build_deps().map_err(|e| e.to_string())?;
    let (text, code) = doctor_text(&deps.config);
    print!("{text}");
    Ok(code)
}

fn status_line(status: &str, msg: &str) -> String {
    format!("{status:<5} {msg}\n")
}

/// Rows sort by backend, then by id. A `*` marks the default model.
pub fn models_table(config: &Config) -> String {
    let mut rows: Vec<(&String, &crate::types::ModelEntry)> = config.models.iter().collect();
    rows.sort_by(|a, b| (&a.1.backend, a.0).cmp(&(&b.1.backend, b.0)));
    let ins: Vec<String> = rows
        .iter()
        .map(|(_, e)| format!("{:.2}", e.price.input))
        .collect();
    let outs: Vec<String> = rows
        .iter()
        .map(|(_, e)| format!("{:.2}", e.price.output))
        .collect();
    let id_w = rows
        .iter()
        .map(|(id, _)| id.len())
        .max()
        .unwrap_or(0)
        .max("ID".len());
    let label_w = rows
        .iter()
        .map(|(_, e)| e.label.len())
        .max()
        .unwrap_or(0)
        .max("LABEL".len());
    let backend_w = rows
        .iter()
        .map(|(_, e)| e.backend.len())
        .max()
        .unwrap_or(0)
        .max("BACKEND".len());
    let in_w = ins
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0)
        .max("$IN/1M".len());
    let out_w = outs
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0)
        .max("$OUT/1M".len());

    let mut out = format!(
        "  {0:<id_w$}  {1:<label_w$}  {2:<backend_w$}  {3:>in_w$}  {4:>out_w$}\n",
        "ID", "LABEL", "BACKEND", "$IN/1M", "$OUT/1M"
    );
    for (i, (id, entry)) in rows.iter().enumerate() {
        let mark = if *id == &config.default { "*" } else { " " };
        out += &format!(
            "{mark} {0:<id_w$}  {1:<label_w$}  {2:<backend_w$}  {3:>in_w$}  {4:>out_w$}\n",
            id, entry.label, entry.backend, ins[i], outs[i]
        );
    }
    out
}

pub fn doctor_text(config: &Config) -> (String, i32) {
    let report = run_doctor(RunDoctorOpts {
        config,
        resolve_bin: None,
        bin_exists: None,
        run_command: None,
        read_package_version: None,
        check_plugin_registration: None,
    });
    let mut text = String::new();
    let mut failed = false;

    if report.plugin.version == "unknown" {
        text += &status_line("fail", "delegate: version unknown");
        failed = true;
    } else {
        text += &status_line("ok", &format!("delegate {}", report.plugin.version));
    }

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

    for backend in ["pi", "claude"] {
        let n = config
            .models
            .values()
            .filter(|e| e.backend == backend)
            .count();
        if n > 0 {
            let word = if n == 1 { "model" } else { "models" };
            text += &status_line(
                "skip",
                &format!("{backend}: backend not implemented ({n} {word})"),
            );
        }
    }

    (text, i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelEntry, Price};
    use std::collections::HashMap;

    fn config() -> Config {
        let mut models: HashMap<String, ModelEntry> = HashMap::new();
        for (id, label, backend) in [
            ("b-2", "B Two", "pi"),
            ("a-1", "A One", "claude"),
            ("composer-2.5", "Composer 2.5", "cursor"),
        ] {
            models.insert(
                id.into(),
                ModelEntry {
                    label: label.into(),
                    backend: backend.into(),
                    price: Price {
                        input: 1.0,
                        output: 2.0,
                        cache_read: 0.0,
                        cache_write: 0.0,
                    },
                },
            );
        }
        Config {
            default: "composer-2.5".into(),
            price_map: models.iter().map(|(k, v)| (k.clone(), v.price)).collect(),
            models,
            profile: crate::types::HostProfile::default(),
        }
    }

    #[test]
    fn table_sorts_and_marks_default() {
        let t = models_table(&config());
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("  ID"));
        let ids: Vec<&str> = lines[1..]
            .iter()
            .map(|l| l[2..].split_whitespace().next().unwrap())
            .collect();
        assert_eq!(ids, ["a-1", "composer-2.5", "b-2"]);
        assert!(lines[2].starts_with('*'));
        assert!(!lines[1].starts_with('*') && !lines[3].starts_with('*'));
    }
}
