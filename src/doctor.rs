use crate::types::{
    Config, DoctorAccountInfo, DoctorAgentInfo, DoctorModelMenuInfo, DoctorPluginInfo, DoctorReport,
};
use std::process::Command;
use std::time::Duration;

const MAX_BUFFER: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct AgentCommandResult {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

pub type RunAgentCommandFn = Box<dyn Fn(&str, &[String]) -> AgentCommandResult + Send + Sync>;

/// The failure text for a backend probe: the spawn error, else stderr, else
/// a generic `<bin> <what> failed`. Shared by the backend doctors.
pub(crate) fn command_err(bin: &str, r: &AgentCommandResult, what: &str) -> String {
    r.error.clone().unwrap_or_else(|| {
        let t = r.stderr.trim();
        if t.is_empty() {
            format!("{bin} {what} failed")
        } else {
            t.to_string()
        }
    })
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

pub fn default_read_package_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").into())
}

pub struct RunDoctorOpts<'a> {
    pub config: &'a Config,
    pub resolve_bin: Option<&'a dyn Fn(Option<&str>) -> String>,
    pub bin_exists: Option<&'a dyn Fn(&str) -> bool>,
    pub run_command: Option<&'a (dyn Fn(&str, &[String]) -> AgentCommandResult + Sync)>,
    pub read_package_version: Option<&'a dyn Fn() -> Result<String, String>>,
}

pub fn run_doctor(opts: RunDoctorOpts<'_>) -> DoctorReport {
    let read_pkg = || {
        if let Some(f) = opts.read_package_version {
            f()
        } else {
            default_read_package_version()
        }
    };

    let warnings = Vec::new();
    let mut failures = Vec::new();
    let mut plugin_version = "unknown".to_string();
    match read_pkg() {
        Ok(v) => plugin_version = v,
        Err(e) => failures.push(format!("failed to read plugin version: {e}")),
    }

    let mut report = DoctorReport {
        ok: false,
        plugin: DoctorPluginInfo {
            version: plugin_version,
        },
        agent: DoctorAgentInfo {
            found: false,
            path: None,
            version: None,
            error: None,
        },
        account: DoctorAccountInfo {
            logged_in: false,
            email: None,
            subscription: None,
            current_model: None,
            error: None,
        },
        model_menu: DoctorModelMenuInfo {
            configured_ids: vec![],
            account_ids: None,
            missing_from_account: vec![],
            prices_checkable: false,
            note: String::new(),
            error: None,
        },
        sections: vec![],
        warnings,
        failures,
    };

    // Every implemented backend that has models in the table. `from_name`
    // None (claude until #17) is a skip in the CLI.
    for backend in crate::backends::Backend::implemented_in(opts.config) {
        backend.fill_doctor(&mut report, &opts);
    }
    report.ok = report.failures.is_empty();
    report
}
