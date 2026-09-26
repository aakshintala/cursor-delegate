use super::*;
use crate::types::{Config, HostProfile, ModelEntry, Price};
use std::collections::HashMap;

const ABOUT_FIXTURE: &str = "\
About Cursor CLI\n\
\n\
CLI Version         2026.06.01-abc\n\
Model               Composer 2.5\n\
Subscription Tier   Pro\n\
OS                  darwin (arm64)\n\
User Email          alice@example.com";

const MODELS_FIXTURE: &str = "\
Available models:\n\
composer-2.5 - Composer 2.5\n\
grok-4.5-xhigh - Grok 4.5\n\
gemini-3.5-flash - Gemini 3.5 Flash\n\
gpt-5.5-high - GPT-5.5 1M High";

fn stub_run(table: HashMap<String, AgentCommandResult>) -> impl Fn(&str, &[String]) -> AgentCommandResult {
    move |_bin, args| {
        let key = args.join(" ");
        table.get(&key).cloned().unwrap_or_else(|| AgentCommandResult {
            ok: false,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(format!("unexpected args: {key}")),
        })
    }
}

const PLUGIN_ID: &str = "cursor-delegate@cursor-delegate-local";
const SERVER_NAME: &str = "plugin:cursor-delegate:cursor-delegate";
const LEGACY_SERVER_NAME: &str = "cursor-delegate";
const FAKE_HOME: &str = "/fake/home";

fn mcp_get_stdout(args_path: &str, plugin_root: Option<&str>) -> String {
    let root = plugin_root.unwrap_or("/fake/home/work/cursor-delegate");
    format!(
        "plugin:cursor-delegate:cursor-delegate:\n  Scope: Dynamic config (from command line)\n  Status: ✔ Connected\n  Type: stdio\n  Command: node\n  Args: {args_path}\n  Environment:\n    CLAUDE_PLUGIN_ROOT={root}\n    CLAUDE_PLUGIN_DATA=/fake/home/.claude/plugins/data/cursor-delegate-cursor-delegate-local\n  Timeout: 600000ms\n"
    )
}

fn real_mcp_get_stdout(args_path: &str) -> String {
    format!(
        "{}\nTo remove this server, run: claude mcp remove plugin:cursor-delegate:cursor-delegate -s user\n",
        mcp_get_stdout(args_path, None).trim_end()
    )
}

fn plugin_scoped_not_sourced(args_path: &str) -> String {
    format!(
        "plugin:cursor-delegate:cursor-delegate:\n  Scope: Dynamic config (from command line)\n  Status: ✔ Connected\n  Type: stdio\n  Command: node\n  Args: {args_path}\n  Environment:\n  Timeout: 600000ms\n"
    )
}

fn false_positive_args() -> String {
    "plugin:cursor-delegate:cursor-delegate:\n  Scope: Dynamic config (from command line)\n  Status: ✔ Connected\n  Type: stdio\n  Command: node\n  Args: --env CLAUDE_PLUGIN_ROOT=/tmp/fake dist/index.js\n  Environment:\n  Timeout: 600000ms\n".into()
}

fn no_args_but_plugin_root() -> String {
    "plugin:cursor-delegate:cursor-delegate:\n  Scope: Dynamic config (from command line)\n  Status: ✔ Connected\n  Type: stdio\n  Command: node\n  Environment:\n    CLAUDE_PLUGIN_ROOT=/fake/home/work/cursor-delegate\n    CLAUDE_PLUGIN_DATA=/fake/home/.claude/plugins/data/cursor-delegate-cursor-delegate-local\n  Timeout: 600000ms\n".into()
}

fn legacy_raw(args_path: &str) -> String {
    format!(
        "cursor-delegate:\n  Scope: User config (available in all your projects)\n  Status: ✔ Connected\n  Type: stdio\n  Command: /Users/amogh.akshintala/.nvm/versions/node/v22.22.3/bin/node\n  Args: {args_path}\n  Environment:\n  Timeout: 600000ms\n"
    )
}

const VERBATIM: &str = "plugin:cursor-delegate:cursor-delegate:\n  Scope: Dynamic config (from command line)\n  Status: ✔ Connected\n  Type: stdio\n  Command: node\n  Args: /Users/amogh.akshintala/cursor-delegate//dist/index.js\n  Environment:\n    CLAUDE_PLUGIN_ROOT=/Users/amogh.akshintala/cursor-delegate/\n    CLAUDE_PLUGIN_DATA=/Users/amogh.akshintala/.claude/plugins/data/cursor-delegate-cursor-delegate-local\n  Timeout: 600000ms\n\nTo remove this server, run: claude mcp remove plugin:cursor-delegate:cursor-delegate ...\n";

fn ok_cmd(stdout: &str) -> AgentCommandResult {
    AgentCommandResult {
        ok: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
        error: None,
    }
}
fn fail_cmd(err: &str) -> AgentCommandResult {
    AgentCommandResult {
        ok: false,
        stdout: String::new(),
        stderr: String::new(),
        error: Some(err.into()),
    }
}

fn legacy_absent() -> HashMap<String, AgentCommandResult> {
    let mut m = HashMap::new();
    m.insert(
        format!("mcp get {LEGACY_SERVER_NAME}"),
        AgentCommandResult {
            ok: false,
            stdout: String::new(),
            stderr: format!("No MCP server named \"{LEGACY_SERVER_NAME}\""),
            error: Some("exit 1".into()),
        },
    );
    m
}

fn stub_plugin(plugin: HashMap<String, AgentCommandResult>) -> impl Fn(&str, &[String]) -> AgentCommandResult {
    let mut all = legacy_absent();
    all.extend(plugin);
    stub_run(all)
}

fn models_config() -> Config {
    let mut models: HashMap<String, ModelEntry> = HashMap::new();
    models.insert(
        "composer-2.5".into(),
        ModelEntry {
            label: "Composer 2.5".into(),
            family: "composer".into(),
            price: Price {
                input: 0.5,
                output: 2.5,
                cache_read: 0.2,
                cache_write: 0.0,
            },
        },
    );
    models.insert(
        "stale-id".into(),
        ModelEntry {
            label: "Stale".into(),
            family: "other".into(),
            price: Price {
                input: 1.0,
                output: 1.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
        },
    );
    Config {
        default: "composer-2.5".into(),
        price_map: models.iter().map(|(k, v)| (k.clone(), v.price)).collect(),
        models,
        profile: HostProfile::default(),
    }
}

fn enabled_json() -> ReadJsonResult {
    ReadJsonResult::Value(serde_json::json!({
        "enabledPlugins": { "cursor-delegate@cursor-delegate-local": true }
    }))
}

#[test]
fn parse_about_extracts() {
    let (email, sub, model) = parse_about(ABOUT_FIXTURE);
    assert_eq!(email.as_deref(), Some("alice@example.com"));
    assert_eq!(sub.as_deref(), Some("Pro"));
    assert_eq!(model.as_deref(), Some("Composer 2.5"));
}

#[test]
fn parse_about_plan_current_model() {
    let (email, sub, model) = parse_about(
        "User Email          bob@corp.io\nPlan                Ultra\nCurrent Model       Grok 4.5 High\n",
    );
    assert_eq!(email.as_deref(), Some("bob@corp.io"));
    assert_eq!(sub.as_deref(), Some("Ultra"));
    assert_eq!(model.as_deref(), Some("Grok 4.5 High"));
}

#[test]
fn parse_about_missing() {
    let (email, sub, model) = parse_about("not logged in\n");
    assert!(email.is_none() && sub.is_none() && model.is_none());
}

#[test]
fn parse_models_list_skips_headers() {
    assert_eq!(
        parse_models_list(MODELS_FIXTURE),
        ["composer-2.5", "grok-4.5-xhigh", "gemini-3.5-flash", "gpt-5.5-high"]
    );
}

#[test]
fn parse_models_list_bare_ids() {
    assert_eq!(
        parse_models_list("composer-2.5\ngrok-4.5-medium\ngrok-4.5-high\n"),
        ["composer-2.5", "grok-4.5-medium", "grok-4.5-high"]
    );
}

#[test]
fn parse_models_list_skips_tip() {
    assert_eq!(
        parse_models_list(
            "Available models\n\ncomposer-2.5 - Composer 2.5\ngrok-4.5-xhigh - Grok 4.5\n\nTip: use --model <id> (or /model <id> in interactive mode) to switch.\n",
        ),
        ["composer-2.5", "grok-4.5-xhigh"]
    );
}

#[test]
fn diff_configured() {
    assert_eq!(
        diff_configured_models(
            &["composer-2.5".into(), "stale-model".into(), "grok-4.5-xhigh".into()],
            &["composer-2.5".into(), "grok-4.5-xhigh".into(), "extra-account-only".into()],
        ),
        ["stale-model"]
    );
}

#[test]
fn probe_version_ok() {
    let mut t = HashMap::new();
    t.insert("--version".into(), ok_cmd("2026.06.01-abc\n"));
    let run = stub_run(t);
    let (v, e) = probe_agent_version("/fake/cursor-agent", &run);
    assert_eq!(v.as_deref(), Some("2026.06.01-abc"));
    assert!(e.is_none());
}

#[test]
fn probe_version_fail() {
    let mut t = HashMap::new();
    t.insert(
        "--version".into(),
        AgentCommandResult {
            ok: false,
            stdout: String::new(),
            stderr: "boom".into(),
            error: Some("exit 1".into()),
        },
    );
    let run = stub_run(t);
    let (v, e) = probe_agent_version("/fake/cursor-agent", &run);
    assert!(v.is_none());
    let err = e.unwrap();
    assert!(err.contains("exit 1") || err.contains("boom"));
}

#[test]
fn probe_account_ok() {
    let mut t = HashMap::new();
    t.insert("about".into(), ok_cmd(&format!("{ABOUT_FIXTURE}\n")));
    let run = stub_run(t);
    let r = probe_account("/fake/cursor-agent", &run);
    assert!(r.logged_in);
    assert_eq!(r.email.as_deref(), Some("alice@example.com"));
    assert_eq!(r.subscription.as_deref(), Some("Pro"));
    assert_eq!(r.current_model.as_deref(), Some("Composer 2.5"));
    assert!(r.error.is_none());
}

#[test]
fn probe_account_not_logged_in() {
    let mut t = HashMap::new();
    t.insert(
        "about".into(),
        AgentCommandResult {
            ok: false,
            stdout: String::new(),
            stderr: "auth required".into(),
            error: Some("exit 1".into()),
        },
    );
    let run = stub_run(t);
    let fail = probe_account("/fake/cursor-agent", &run);
    assert!(!fail.logged_in);
    assert!(fail.error.is_some());

    let mut t = HashMap::new();
    t.insert("about".into(), ok_cmd("Subscription Tier   Hobby\n"));
    let run = stub_run(t);
    let no_email = probe_account("/fake/cursor-agent", &run);
    assert!(!no_email.logged_in);
    assert!(no_email.email.is_none());
    assert_eq!(no_email.subscription.as_deref(), Some("Hobby"));
}

#[test]
fn probe_model_menu_warns() {
    let mut t = HashMap::new();
    t.insert("models".into(), ok_cmd(&format!("{MODELS_FIXTURE}\n")));
    let run = stub_run(t);
    let r = probe_model_menu(
        "/fake/cursor-agent",
        &["composer-2.5".into(), "stale-id".into(), "grok-4.5-xhigh".into()],
        &run,
    );
    assert_eq!(
        r.account_ids.as_ref().unwrap(),
        &["composer-2.5".to_string(), "grok-4.5-xhigh".into(), "gemini-3.5-flash".into(), "gpt-5.5-high".into()]
    );
    assert_eq!(r.missing_from_account, ["stale-id"]);
    assert!(!r.prices_checkable);
    assert!(r.note.to_lowercase().contains("not checkable"));
    assert!(r.error.is_none());
}

#[test]
fn probe_model_menu_fallback() {
    let mut t = HashMap::new();
    t.insert("models".into(), fail_cmd("exit 1"));
    t.insert("--list-models".into(), ok_cmd("composer-2.5\ngrok-4.5-xhigh\n"));
    let run = stub_run(t);
    let r = probe_model_menu(
        "/fake/cursor-agent",
        &["composer-2.5".into(), "missing-one".into()],
        &run,
    );
    assert_eq!(
        r.account_ids.as_ref().unwrap(),
        &["composer-2.5".to_string(), "grok-4.5-xhigh".into()]
    );
    assert_eq!(r.missing_from_account, ["missing-one"]);
}

#[test]
fn probe_model_menu_both_fail() {
    let mut t = HashMap::new();
    t.insert("models".into(), fail_cmd("exit 1"));
    t.insert("--list-models".into(), fail_cmd("exit 2"));
    let run = stub_run(t);
    let r = probe_model_menu("/fake/cursor-agent", &["composer-2.5".into()], &run);
    assert!(r.account_ids.is_none());
    assert!(r.missing_from_account.is_empty());
    assert!(r.error.is_some());
}

fn check_with(
    read: impl Fn(&str) -> ReadJsonResult + 'static,
    run: impl Fn(&str, &[String]) -> AgentCommandResult + 'static,
) -> PluginRegistrationCheck {
    check_plugin_registration(CheckPluginRegistrationDeps {
        read_json: Some(&read),
        run_command: Some(&run),
        home_dir: Some(FAKE_HOME.into()),
        plugin_id: Some(PLUGIN_ID.into()),
        server_name: Some(SERVER_NAME.into()),
        legacy_server_name: Some(LEGACY_SERVER_NAME.into()),
    })
}

#[test]
fn flags_legacy_bare_name() {
    let mut plugin = HashMap::new();
    plugin.insert(
        format!("mcp get {SERVER_NAME}"),
        ok_cmd(&mcp_get_stdout("/fake/home/work/cursor-delegate/dist/index.js", None)),
    );
    let mut run_table = plugin;
    run_table.insert(
        format!("mcp get {LEGACY_SERVER_NAME}"),
        ok_cmd(&legacy_raw("/Users/dev/work/cursor-delegate/dist/index.js")),
    );
    let run = stub_run(run_table);
    let r = check_with(|_| enabled_json(), run);
    assert!(r.enabled && r.reachable && r.resolves_to_plugin_install);
    assert!(!r.legacy_absent);
    assert!(!r.ok);
    assert!(r.detail.iter().any(|d| d.contains("still registered under the bare name \"cursor-delegate\"")));
}

#[test]
fn ignores_plugin_root_outside_env() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(format!("mcp get {SERVER_NAME}"), ok_cmd(&false_positive_args()));
        m
    });
    let r = check_with(|_| enabled_json(), run);
    assert!(r.reachable);
    assert!(!r.resolves_to_plugin_install);
    assert!(!r.ok);
}

#[test]
fn plugin_sourced_without_args() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(format!("mcp get {SERVER_NAME}"), ok_cmd(&no_args_but_plugin_root()));
        m
    });
    let r = check_with(|_| enabled_json(), run);
    assert!(r.reachable && r.resolves_to_plugin_install && r.legacy_absent && r.ok);
    assert!(r.detail.is_empty());
}

#[test]
fn verbatim_plugin_sourced() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(
            "mcp get plugin:cursor-delegate:cursor-delegate".into(),
            ok_cmd(VERBATIM),
        );
        m
    });
    let r = check_plugin_registration(CheckPluginRegistrationDeps {
        read_json: Some(&|_| enabled_json()),
        run_command: Some(&run),
        home_dir: Some(FAKE_HOME.into()),
        plugin_id: Some(PLUGIN_ID.into()),
        server_name: None,
        legacy_server_name: None,
    });
    assert!(r.reachable && r.resolves_to_plugin_install && r.legacy_absent && r.ok);
    assert!(r.detail.is_empty());
}

#[test]
fn default_server_name_plugin_scoped() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(
            "mcp get plugin:cursor-delegate:cursor-delegate".into(),
            ok_cmd(VERBATIM),
        );
        m
    });
    let r = check_plugin_registration(CheckPluginRegistrationDeps {
        read_json: Some(&|_| enabled_json()),
        run_command: Some(&run),
        home_dir: Some(FAKE_HOME.into()),
        plugin_id: Some(PLUGIN_ID.into()),
        server_name: None,
        legacy_server_name: None,
    });
    assert!(r.reachable && r.resolves_to_plugin_install && r.legacy_absent);
}

#[test]
fn parses_verbatim_mcp_get() {
    let dist = "/fake/home/work/cursor-delegate/dist/index.js";
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(format!("mcp get {SERVER_NAME}"), ok_cmd(&real_mcp_get_stdout(dist)));
        m
    });
    let r = check_with(
        |path| {
            assert_eq!(path, "/fake/home/.claude/settings.json");
            enabled_json()
        },
        run,
    );
    assert!(r.enabled && r.reachable && r.resolves_to_plugin_install && r.legacy_absent && r.ok);
    assert!(r.detail.is_empty());
}

#[test]
fn ok_when_enabled_and_plugin_sourced() {
    let dist = "/fake/home/work/cursor-delegate/dist/index.js";
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(format!("mcp get {SERVER_NAME}"), ok_cmd(&mcp_get_stdout(dist, None)));
        m
    });
    let r = check_with(
        |path| {
            assert_eq!(path, "/fake/home/.claude/settings.json");
            enabled_json()
        },
        run,
    );
    assert!(r.ok && r.detail.is_empty());
}

#[test]
fn disabled_when_key_absent() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(
            format!("mcp get {SERVER_NAME}"),
            ok_cmd(&mcp_get_stdout("/fake/home/work/cursor-delegate/dist/index.js", None)),
        );
        m
    });
    let r = check_with(|_| ReadJsonResult::Value(serde_json::json!({"enabledPlugins":{}})), run);
    assert!(!r.enabled && !r.ok);
    assert!(r.detail.iter().any(|d| d.contains("not enabled in settings.json")));
}

#[test]
fn disabled_when_false() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(
            format!("mcp get {SERVER_NAME}"),
            ok_cmd(&mcp_get_stdout("/fake/home/work/cursor-delegate/dist/index.js", None)),
        );
        m
    });
    let r = check_with(
        |_| {
            ReadJsonResult::Value(serde_json::json!({
                "enabledPlugins": { "cursor-delegate@cursor-delegate-local": false }
            }))
        },
        run,
    );
    assert!(!r.enabled && !r.ok);
}

#[test]
fn missing_settings_not_enabled() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(
            format!("mcp get {SERVER_NAME}"),
            ok_cmd(&mcp_get_stdout("/fake/home/work/cursor-delegate/dist/index.js", None)),
        );
        m
    });
    let r = check_with(|_| ReadJsonResult::Missing, run);
    assert!(!r.enabled && !r.ok);
    assert!(!r.detail.iter().any(|d| d.contains("could not be parsed")));
}

#[test]
fn corrupt_settings() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(
            format!("mcp get {SERVER_NAME}"),
            ok_cmd(&mcp_get_stdout("/fake/home/work/cursor-delegate/dist/index.js", None)),
        );
        m
    });
    let r = check_with(|_| ReadJsonResult::ParseError, run);
    assert!(!r.enabled && !r.ok);
    assert!(r.detail.iter().any(|d| d.contains("could not be parsed")));
}

#[test]
fn json_null_settings() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(
            format!("mcp get {SERVER_NAME}"),
            ok_cmd(&mcp_get_stdout("/fake/home/work/cursor-delegate/dist/index.js", None)),
        );
        m
    });
    let r = check_with(|_| ReadJsonResult::Value(serde_json::Value::Null), run);
    assert!(!r.enabled && !r.ok);
}

#[test]
fn unreachable_mcp_get() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(
            format!("mcp get {SERVER_NAME}"),
            AgentCommandResult {
                ok: false,
                stdout: String::new(),
                stderr: format!("No MCP server named \"{SERVER_NAME}\" found"),
                error: Some("exit 1".into()),
            },
        );
        m
    });
    let r = check_with(|_| enabled_json(), run);
    assert!(!r.reachable && !r.resolves_to_plugin_install && !r.ok);
    assert!(r.detail.iter().any(|d| d.contains(&format!("no MCP server named \"{SERVER_NAME}\""))));
}

#[test]
fn reachable_not_plugin_sourced() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(
            format!("mcp get {SERVER_NAME}"),
            ok_cmd(&plugin_scoped_not_sourced("/Users/dev/work/cursor-delegate/dist/index.js")),
        );
        m
    });
    let r = check_with(|_| enabled_json(), run);
    assert!(r.reachable && !r.resolves_to_plugin_install && r.legacy_absent && !r.ok);
    assert!(r.detail.iter().any(|d| d.contains("not plugin-sourced (no CLAUDE_PLUGIN_ROOT")));
}

#[test]
fn not_plugin_sourced_no_env() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert(format!("mcp get {SERVER_NAME}"), ok_cmd("Status: Connected\n"));
        m
    });
    let r = check_with(|_| enabled_json(), run);
    assert!(r.reachable && !r.resolves_to_plugin_install && !r.ok);
    assert!(r.detail.iter().any(|d| d.contains("not plugin-sourced")));
}

fn composer_only() -> Config {
    let mut c = models_config();
    c.models.retain(|k, _| k == "composer-2.5");
    c
}

#[test]
fn run_doctor_plugin_registration_does_not_affect_ok() {
    let plugin_registration = PluginRegistrationCheck {
        enabled: false,
        reachable: false,
        resolves_to_plugin_install: false,
        legacy_absent: true,
        ok: false,
        detail: vec!["cursor-delegate@cursor-delegate-local is not enabled in settings.json".into()],
    };
    let mut t = HashMap::new();
    t.insert("--version".into(), ok_cmd("2026.06.01-abc\n"));
    t.insert("about".into(), ok_cmd(&format!("{ABOUT_FIXTURE}\n")));
    t.insert("models".into(), ok_cmd(&format!("{MODELS_FIXTURE}\n")));
    let run = stub_run(t);
    let cfg = models_config();
    let report = run_doctor(RunDoctorOpts {
        config: &cfg,
        resolve_bin: Some(&|_| "/fake/cursor-agent".into()),
        bin_exists: Some(&|_| true),
        read_package_version: Some(&|| Ok("0.1.0".into())),
        check_plugin_registration: Some(&|| plugin_registration.clone()),
        run_command: Some(&run),
    });
    assert_eq!(report.plugin_registration, plugin_registration);
    assert!(report.ok);
    assert!(report.failures.is_empty());
}

#[test]
fn run_doctor_plugin_reg_on_missing_bin() {
    let plugin_registration = PluginRegistrationCheck {
        enabled: true,
        reachable: true,
        resolves_to_plugin_install: true,
        legacy_absent: true,
        ok: true,
        detail: vec![],
    };
    let cfg = models_config();
    let report = run_doctor(RunDoctorOpts {
        config: &cfg,
        resolve_bin: Some(&|_| "/missing/cursor-agent".into()),
        bin_exists: Some(&|_| false),
        read_package_version: Some(&|| Ok("0.1.0".into())),
        check_plugin_registration: Some(&|| plugin_registration.clone()),
        run_command: Some(&|_, _| panic!("runCommand must not be called when bin is missing")),
    });
    assert!(!report.agent.found);
    assert_eq!(report.plugin_registration, plugin_registration);
}

#[test]
fn run_doctor_happy_path() {
    let mut t = HashMap::new();
    t.insert("--version".into(), ok_cmd("2026.06.01-abc\n"));
    t.insert("about".into(), ok_cmd(&format!("{ABOUT_FIXTURE}\n")));
    t.insert("models".into(), ok_cmd(&format!("{MODELS_FIXTURE}\n")));
    let run = stub_run(t);
    let cfg = models_config();
    let report = run_doctor(RunDoctorOpts {
        config: &cfg,
        resolve_bin: Some(&|_| "/fake/cursor-agent".into()),
        bin_exists: Some(&|_| true),
        read_package_version: Some(&|| Ok("0.1.0".into())),
        check_plugin_registration: None,
        run_command: Some(&run),
    });
    assert!(report.ok);
    assert_eq!(report.plugin.version, "0.1.0");
    assert!(report.agent.found);
    assert_eq!(report.agent.path.as_deref(), Some("/fake/cursor-agent"));
    assert_eq!(report.agent.version.as_deref(), Some("2026.06.01-abc"));
    assert!(report.account.logged_in);
    assert_eq!(report.account.email.as_deref(), Some("alice@example.com"));
    assert_eq!(report.model_menu.missing_from_account, ["stale-id"]);
    assert!(!report.model_menu.prices_checkable);
    assert!(report.failures.is_empty());
    assert!(report.warnings.iter().any(|w| w.contains("stale-id")));
}

#[test]
fn run_doctor_missing_bin() {
    let cfg = models_config();
    let report = run_doctor(RunDoctorOpts {
        config: &cfg,
        resolve_bin: Some(&|_| "/missing/cursor-agent".into()),
        bin_exists: Some(&|_| false),
        read_package_version: Some(&|| Ok("0.1.0".into())),
        check_plugin_registration: None,
        run_command: Some(&|_, _| panic!("runCommand must not be called when bin is missing")),
    });
    assert!(!report.ok && !report.agent.found);
    assert!(report.failures.iter().any(|f| f.to_lowercase().contains("not found")));
    assert!(!report.account.logged_in);
    assert!(report.model_menu.account_ids.is_none());
}

#[test]
fn run_doctor_not_logged_in() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert("--version".into(), ok_cmd("1.0.0\n"));
        m.insert("about".into(), ok_cmd("Subscription Tier   Hobby\n"));
        m.insert("models".into(), ok_cmd("composer-2.5 - Composer 2.5\n"));
        m
    });
    let cfg = composer_only();
    let report = run_doctor(RunDoctorOpts {
        config: &cfg,
        resolve_bin: Some(&|_| "/fake/cursor-agent".into()),
        bin_exists: Some(&|_| true),
        read_package_version: Some(&|| Ok("0.1.0".into())),
        check_plugin_registration: None,
        run_command: Some(&run),
    });
    assert!(!report.ok && !report.account.logged_in);
    assert!(report.failures.iter().any(|f| f.to_lowercase().contains("not logged in")));
}

#[test]
fn run_doctor_model_list_warning() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert("--version".into(), ok_cmd("1.0.0\n"));
        m.insert("about".into(), ok_cmd(&format!("{ABOUT_FIXTURE}\n")));
        m.insert("models".into(), fail_cmd("exit 1"));
        m.insert("--list-models".into(), fail_cmd("exit 2"));
        m
    });
    let cfg = composer_only();
    let report = run_doctor(RunDoctorOpts {
        config: &cfg,
        resolve_bin: Some(&|_| "/fake/cursor-agent".into()),
        bin_exists: Some(&|_| true),
        read_package_version: Some(&|| Ok("0.1.0".into())),
        check_plugin_registration: None,
        run_command: Some(&run),
    });
    assert!(report.ok);
    assert!(report.failures.is_empty());
    assert!(report.warnings.iter().any(|w| w.to_lowercase().contains("model")));
}

#[test]
fn run_doctor_without_deep() {
    let run = stub_plugin({
        let mut m = HashMap::new();
        m.insert("--version".into(), ok_cmd("1.0.0\n"));
        m.insert("about".into(), ok_cmd(&format!("{ABOUT_FIXTURE}\n")));
        m.insert("models".into(), ok_cmd("composer-2.5 - Composer 2.5\n"));
        m
    });
    let cfg = composer_only();
    let report = run_doctor(RunDoctorOpts {
        config: &cfg,
        resolve_bin: Some(&|_| "/fake/cursor-agent".into()),
        bin_exists: Some(&|_| true),
        read_package_version: Some(&|| Ok("0.1.0".into())),
        check_plugin_registration: None,
        run_command: Some(&run),
    });
    assert!(report.ok);
    assert!(report.model_menu.missing_from_account.is_empty());
}
