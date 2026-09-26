use super::*;
use crate::job_registry::RegistryDeps;
use crate::job_registry::tests::{FakeBackend, bg, done_ok, fake_finalize, settle, spec_of};
use crate::status_record::NoopStatusWriter;
use crate::tool_schemas::build_tools;
use crate::types::{Config, HostProfile, ModelEntry, Price};
use serde_json::json;
use std::collections::HashMap;

fn config() -> Config {
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
    let price_map = models.iter().map(|(k, v)| (k.clone(), v.price)).collect();
    Config {
        default: "composer-2.5".into(),
        models,
        price_map,
        profile: HostProfile::default(),
    }
}

/// Server deps over the real registry and a fake backend that finishes runs immediately.
fn deps() -> (ServerDeps, Arc<FakeBackend>) {
    let fake = Arc::new(FakeBackend::default());
    *fake.auto.lock().unwrap() = Some(done_ok());
    let mut rd = RegistryDeps::new(fake.clone(), 10_000.0, None, None);
    rd.finalize = Arc::new(fake_finalize);
    rd.finalize_stall = Arc::new(fake_finalize);
    rd.status_writer = Arc::new(NoopStatusWriter);
    let d = ServerDeps {
        config: config(),
        registry: JobRegistry::new(rd),
        cli_config: None,
        server_cwd: "/srv".into(),
    };
    (d, fake)
}

#[test]
fn exposes_eight_tools() {
    let tools = build_tools(&config());
    assert_eq!(tools.len(), 8);
    let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "cursor_run",
            "cursor_poll",
            "cursor_cancel",
            "cursor_wait",
            "cursor_wait_any",
            "cursor_wait_all",
            "cursor_answer",
            "doctor",
        ]
    );
}

#[test]
fn cursor_run_requires_prompt() {
    let (d, _) = deps();
    let e = handle_call("cursor_run", Some(&json!({})), &d, Default::default()).unwrap_err();
    assert!(e.contains("prompt"));
}

#[test]
fn cursor_run_rejects_bad_types() {
    let (d, _) = deps();
    let e = handle_call(
        "cursor_run",
        Some(&json!({"prompt":"x","waitMs":"nope"})),
        &d,
        Default::default(),
    )
    .unwrap_err();
    assert!(e.contains("waitMs"));
    let e = handle_call(
        "cursor_run",
        Some(&json!({"prompt":"x","background":"false"})),
        &d,
        Default::default(),
    )
    .unwrap_err();
    assert!(e.contains("background"));
}

#[test]
fn cursor_run_rejects_capability_and_isolation() {
    let (d, _) = deps();
    let e = handle_call(
        "cursor_run",
        Some(&json!({"prompt":"x","capability":"fly"})),
        &d,
        Default::default(),
    )
    .unwrap_err();
    assert!(e.contains("capability"));
    let e = handle_call(
        "cursor_run",
        Some(&json!({"prompt":"x","isolation":{"type":"Nope"}})),
        &d,
        Default::default(),
    )
    .unwrap_err();
    assert!(e.contains("isolation"));
}

#[test]
fn routes_each_tool_to_the_registry() {
    let (d, fake) = deps();
    let call =
        |name: &str, args: Value| handle_call(name, Some(&args), &d, Default::default()).unwrap();

    let run = call("cursor_run", json!({"prompt":"x"}));
    assert_eq!(run["status"], "DONE");
    assert_eq!(fake.count(), 1);
    let id = run["jobId"].as_str().unwrap().to_string();

    assert_eq!(call("cursor_poll", json!({"jobId": id}))["status"], "DONE");
    assert_eq!(
        call("cursor_poll", json!({"jobId":"a"}))["status"],
        "NOT_FOUND"
    );
    assert_eq!(call("cursor_wait", json!({"jobId": id}))["status"], "DONE");
    assert_eq!(
        call("cursor_wait_any", json!({"jobIds":[id]}))["firstDone"],
        id.as_str()
    );
    assert_eq!(
        call("cursor_wait_all", json!({"jobIds":[id]}))["allDone"],
        true
    );

    *fake.auto.lock().unwrap() = None;
    let bg_id = d
        .registry
        .dispatch(spec_of(bg), Default::default())
        .job_id()
        .unwrap()
        .to_string();
    assert_eq!(
        call("cursor_cancel", json!({"jobId": bg_id}))["status"],
        "CANCELLED"
    );
    assert_eq!(settle(&d.registry, &bg_id), "CANCELLED");
}

#[test]
fn unknown_tool_throws() {
    let (d, _) = deps();
    let e = handle_call("nope", Some(&json!({})), &d, Default::default()).unwrap_err();
    assert!(e.contains("unknown tool"));
}

#[test]
fn cursor_answer_requires_fields() {
    let (d, _) = deps();
    let e = handle_call(
        "cursor_answer",
        Some(&json!({"answer":"x"})),
        &d,
        Default::default(),
    )
    .unwrap_err();
    assert!(e.contains("jobId"));
    let e = handle_call(
        "cursor_answer",
        Some(&json!({"jobId":"j1"})),
        &d,
        Default::default(),
    )
    .unwrap_err();
    assert!(e.contains("answer"));
}

#[test]
fn cursor_answer_not_found() {
    let (d, _) = deps();
    let res = handle_call(
        "cursor_answer",
        Some(&json!({"jobId":"missing","answer":"because v2"})),
        &d,
        Default::default(),
    )
    .unwrap();
    assert_eq!(res, json!({"status":"NOT_FOUND"}));
}

#[test]
fn doctor_without_args() {
    let (d, _) = deps();
    let prev = std::env::var("CURSOR_AGENT_BIN").ok();
    unsafe {
        std::env::set_var(
            "CURSOR_AGENT_BIN",
            "/nonexistent/cursor-agent-for-doctor-test",
        )
    };
    let report = handle_call("doctor", Some(&json!({})), &d, Default::default()).unwrap();
    match prev {
        Some(v) => unsafe { std::env::set_var("CURSOR_AGENT_BIN", v) },
        None => unsafe { std::env::remove_var("CURSOR_AGENT_BIN") },
    }
    assert_eq!(report["agent"]["found"], false);
    assert_eq!(report["ok"], false);
    assert_eq!(report["modelMenu"]["pricesCheckable"], false);
    assert!(
        report["modelMenu"]["note"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("not checkable")
    );
    assert!(report["plugin"]["version"].is_string());
    let failures = report["failures"].as_array().unwrap();
    assert!(
        failures
            .iter()
            .any(|f| f.as_str().unwrap().to_lowercase().contains("not found"))
    );
}

#[test]
fn cursor_answer_not_awaiting() {
    let (d, _) = deps();
    let id = handle_call(
        "cursor_run",
        Some(&json!({"prompt":"x"})),
        &d,
        Default::default(),
    )
    .unwrap()["jobId"]
        .as_str()
        .unwrap()
        .to_string();
    let e = handle_call(
        "cursor_answer",
        Some(&json!({"jobId": id, "answer":"x"})),
        &d,
        Default::default(),
    )
    .unwrap_err();
    assert!(e.contains("job is not awaiting an answer"));
}

#[test]
fn transport_smoke_list_and_poll() {
    let (d, _) = deps();
    let listed = tools_list_result(&d.config);
    assert_eq!(listed["tools"].as_array().unwrap().len(), 8);
    let poll = handle_call(
        "cursor_poll",
        Some(&json!({"jobId":"missing"})),
        &d,
        Default::default(),
    )
    .unwrap();
    let wrapped = json_content(&poll);
    assert_eq!(wrapped["content"][0]["type"], "text");
    let parsed: Value =
        serde_json::from_str(wrapped["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed, json!({"status":"NOT_FOUND"}));
    let e = handle_call("nope", Some(&json!({})), &d, Default::default()).unwrap_err();
    assert!(e.contains("unknown tool"));
}

#[test]
fn wait_rejects_non_finite_timeout() {
    let (d, _) = deps();
    for name in ["cursor_wait", "cursor_wait_any", "cursor_wait_all"] {
        let args = if name == "cursor_wait" {
            json!({"jobId":"j","timeoutMs":"5000"})
        } else {
            json!({"jobIds":["j"],"timeoutMs":"5000"})
        };
        let e = handle_call(name, Some(&args), &d, Default::default()).unwrap_err();
        assert!(e.contains("timeoutMs"), "{name}: {e}");
    }
}
