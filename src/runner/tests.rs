use super::*;
use crate::job_registry::RegistryDeps;
use crate::job_registry::tests::{
    FakeBackend, bg, done_ok, fake_finalize, result_of, settle, spec_of,
};
use crate::models::NonClaudeViolationError;
use crate::safety::DenyListError;
use crate::status_record::NoopStatusWriter;
use crate::types::{HostProfile, ModelEntry, Price};
use std::collections::HashMap;
use std::sync::Mutex;

fn price(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Price {
    Price {
        input,
        output,
        cache_read,
        cache_write,
    }
}

fn config() -> Config {
    let mut models: HashMap<String, ModelEntry> = HashMap::new();
    models.insert(
        "composer-2.5".into(),
        ModelEntry {
            label: "Composer 2.5".into(),
            family: "composer".into(),
            price: price(0.5, 2.5, 0.2, 0.0),
        },
    );
    models.insert(
        "grok-4.5-xhigh".into(),
        ModelEntry {
            label: "Grok 4.5".into(),
            family: "grok".into(),
            price: price(2.0, 6.0, 0.5, 0.0),
        },
    );
    models.insert(
        "claude-sonnet-4".into(),
        ModelEntry {
            label: "Claude Sonnet 4".into(),
            family: "claude".into(),
            price: price(3.0, 15.0, 0.3, 0.0),
        },
    );
    let price_map = models.iter().map(|(k, v)| (k.clone(), v.price)).collect();
    Config {
        default: "composer-2.5".into(),
        models,
        price_map,
        profile: HostProfile {
            prompt_preamble: Some("PREAMBLE".into()),
            required_deny: Some(vec!["rm -rf /".into()]),
            gate: Some("make ci".into()),
            ..HostProfile::default()
        },
    }
}

/// The real registry over a fake backend that finishes every run immediately.
struct Harness {
    registry: Arc<JobRegistry>,
    fake: Arc<FakeBackend>,
}

impl Harness {
    fn new() -> Self {
        Self::with_deadline(10_000.0)
    }
    fn with_deadline(deadline_ms: f64) -> Self {
        let fake = Arc::new(FakeBackend::default());
        *fake.auto.lock().unwrap() = Some(done_ok());
        let mut deps = RegistryDeps::new(fake.clone(), deadline_ms, None, None);
        deps.finalize = Arc::new(fake_finalize);
        deps.finalize_stall = Arc::new(fake_finalize);
        deps.status_writer = Arc::new(NoopStatusWriter);
        Self {
            registry: JobRegistry::new(deps),
            fake,
        }
    }
    fn last(&self) -> JobSpec {
        self.fake.last_spec()
    }
    fn calls(&self) -> usize {
        self.fake.count()
    }
}

fn deps_with(h: &Harness, cli_deny: Option<Vec<String>>) -> RunnerDeps {
    RunnerDeps {
        config: config(),
        registry: Arc::clone(&h.registry),
        cli_config: cli_deny.map(|d| crate::safety::CliConfig {
            permissions: Some(crate::safety::CliPermissions { deny: Some(d) }),
        }),
        server_cwd: "/srv".into(),
        resolve_bin: Some(Arc::new(|_| "cursor-agent".into())),
        capture_head: Some(Arc::new(|_, _| Some("HEAD0".into()))),
    }
}

#[test]
fn build_argv_prompt_last() {
    let argv = build_argv("m", &["--mode".into(), "ask".into()], &[], None, "hello");
    assert_eq!(
        argv,
        [
            "--print",
            "--output-format",
            "stream-json",
            "--trust",
            "--approve-mcps",
            "--model",
            "m",
            "--mode",
            "ask",
            "--",
            "hello",
        ]
    );
    assert_eq!(argv.last().unwrap(), "hello");
}

#[test]
fn build_argv_dash_prompt() {
    let argv = build_argv("m", &[], &[], None, "--help");
    let sep = argv.iter().position(|s| s == "--").unwrap();
    assert_eq!(argv[sep + 1], "--help");
}

#[test]
fn build_argv_resume() {
    let argv = build_argv("m", &[], &[], Some("sess-1"), "p");
    let i = argv.iter().position(|s| s == "--resume").unwrap();
    assert_eq!(argv[i + 1], "sess-1");
}

#[test]
fn ask_run_builds_spec() {
    let reg = Harness::new();
    run_delegation(
        RunInput {
            prompt: "do it".into(),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec!["rm -rf /".into()])),
        Default::default(),
    )
    .unwrap();
    let spec = reg.last();
    assert_eq!(spec.model, "composer-2.5");
    assert!(!spec.is_write);
    assert_eq!(spec.cwd, "/srv");
    assert_eq!(spec.gate, "make ci");
    assert_eq!(spec.head_before.as_deref(), Some("HEAD0"));
    let prompt = spec.argv.last().unwrap();
    assert!(prompt.contains("PREAMBLE"));
    assert!(prompt.contains("do it"));
}

#[test]
fn write_with_deny_list() {
    let reg = Harness::new();
    run_delegation(
        RunInput {
            prompt: "edit".into(),
            capability: Some(Capability::Write),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec!["rm -rf /".into()])),
        Default::default(),
    )
    .unwrap();
    assert!(reg.last().is_write);
}

#[test]
fn write_missing_deny_throws_before_dispatch() {
    let reg = Harness::new();
    let e = run_delegation(
        RunInput {
            prompt: "edit".into(),
            capability: Some(Capability::Write),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec![])),
        Default::default(),
    )
    .unwrap_err();
    assert!(e.downcast_ref::<DenyListError>().is_some());
    assert_eq!(reg.calls(), 0);
}

#[test]
fn ask_missing_deny_throws() {
    let reg = Harness::new();
    let e = run_delegation(
        RunInput {
            prompt: "look".into(),
            capability: Some(Capability::Ask),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec![])),
        Default::default(),
    )
    .unwrap_err();
    assert!(e.downcast_ref::<DenyListError>().is_some());
}

#[test]
fn plan_missing_deny_throws() {
    let reg = Harness::new();
    let e = run_delegation(
        RunInput {
            prompt: "plan it".into(),
            capability: Some(Capability::Plan),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec![])),
        Default::default(),
    )
    .unwrap_err();
    assert!(e.downcast_ref::<DenyListError>().is_some());
}

#[test]
fn write_unsandboxed_downgrades() {
    let reg = Harness::new();
    run_delegation(
        RunInput {
            prompt: "edit".into(),
            capability: Some(Capability::WriteUnsandboxed),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec!["rm -rf /".into()])),
        Default::default(),
    )
    .unwrap();
    let spec = reg.last();
    assert!(spec.downgraded);
    assert!(spec.argv.iter().any(|s| s == "enabled"));
    assert!(!spec.argv.iter().any(|s| s == "disabled"));
    assert!(spec.is_write);
    assert_eq!(spec.resume_context.capability, Capability::WriteUnsandboxed);
    assert!(!spec.resume_context.allow_unsandboxed);
}

#[test]
fn write_unsandboxed_with_signal() {
    let reg = Harness::new();
    run_delegation(
        RunInput {
            prompt: "edit".into(),
            capability: Some(Capability::WriteUnsandboxed),
            allow_unsandboxed: Some(true),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec!["rm -rf /".into()])),
        Default::default(),
    )
    .unwrap();
    let spec = reg.last();
    assert!(!spec.downgraded);
    assert!(spec.argv.iter().any(|s| s == "disabled"));
    assert!(spec.resume_context.allow_unsandboxed);
}

#[test]
fn foreground_needs_context_keeps_its_job_id() {
    let reg = Harness::new();
    *reg.fake.auto.lock().unwrap() = Some(result_of(
        "which version?\nSTATUS: NEEDS_CONTEXT",
        Some("sess-1"),
    ));
    let res = run_delegation(
        RunInput {
            prompt: "do it".into(),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec!["rm -rf /".into()])),
        Default::default(),
    )
    .unwrap();
    assert_eq!(res.status_label(), "NEEDS_CONTEXT");
    let id = res.job_id().expect("answerable job id");
    assert!(matches!(
        reg.registry.lookup_answer(id),
        AnswerLookup::Ok { .. }
    ));
}

#[test]
fn require_non_claude_throws() {
    let reg = Harness::new();
    let e = run_delegation(
        RunInput {
            prompt: "x".into(),
            model: Some("claude-sonnet-4".into()),
            require_non_claude: Some(true),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec![])),
        Default::default(),
    )
    .unwrap_err();
    assert!(e.downcast_ref::<NonClaudeViolationError>().is_some());
}

#[test]
fn caller_provided_sets_cwd_and_workspace() {
    let reg = Harness::new();
    run_delegation(
        RunInput {
            prompt: "edit".into(),
            capability: Some(Capability::Write),
            isolation: Some(Isolation::CallerProvided {
                path: "/repo".into(),
            }),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec!["rm -rf /".into()])),
        Default::default(),
    )
    .unwrap();
    let spec = reg.last();
    assert_eq!(spec.cwd, "/repo");
    assert_eq!(spec.path.as_deref(), Some("/repo"));
    assert!(spec.argv.iter().any(|s| s == "--workspace"));
}

#[test]
fn backend_provided_captures_base() {
    let reg = Harness::new();
    let calls: Arc<Mutex<Vec<(String, Option<String>)>>> = Arc::new(Mutex::new(vec![]));
    let calls2 = Arc::clone(&calls);
    let mut deps = deps_with(&reg, Some(vec!["rm -rf /".into()]));
    deps.capture_head = Some(Arc::new(move |cwd, r#ref| {
        calls2
            .lock()
            .unwrap()
            .push((cwd.to_string(), r#ref.map(|s| s.to_string())));
        Some("BASE0".into())
    }));
    run_delegation(
        RunInput {
            prompt: "edit".into(),
            capability: Some(Capability::Write),
            isolation: Some(Isolation::BackendProvided {
                name: Some("wt-1".into()),
                base: Some("main".into()),
            }),
            ..Default::default()
        },
        &deps,
        Default::default(),
    )
    .unwrap();
    let spec = reg.last();
    assert_eq!(
        *calls.lock().unwrap(),
        vec![("/srv".into(), Some("main".into()))]
    );
    assert_eq!(spec.head_before.as_deref(), Some("BASE0"));
    assert_eq!(spec.worktree_name.as_deref(), Some("wt-1"));
}

#[test]
fn backend_provided_no_base_is_head() {
    let reg = Harness::new();
    let calls: Arc<Mutex<Vec<(String, Option<String>)>>> = Arc::new(Mutex::new(vec![]));
    let calls2 = Arc::clone(&calls);
    let mut deps = deps_with(&reg, Some(vec!["rm -rf /".into()]));
    deps.capture_head = Some(Arc::new(move |cwd, r#ref| {
        calls2
            .lock()
            .unwrap()
            .push((cwd.to_string(), r#ref.map(|s| s.to_string())));
        Some("HEAD0".into())
    }));
    run_delegation(
        RunInput {
            prompt: "edit".into(),
            capability: Some(Capability::Write),
            isolation: Some(Isolation::BackendProvided {
                name: None,
                base: None,
            }),
            ..Default::default()
        },
        &deps,
        Default::default(),
    )
    .unwrap();
    assert_eq!(*calls.lock().unwrap(), vec![("/srv".into(), None)]);
}

#[test]
fn per_call_gate_and_verify() {
    let reg = Harness::new();
    run_delegation(
        RunInput {
            prompt: "p".into(),
            gate: Some("custom-gate".into()),
            verify_commands: Some(vec!["x test".into()]),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec!["rm -rf /".into()])),
        Default::default(),
    )
    .unwrap();
    let spec = reg.last();
    assert_eq!(spec.gate, "custom-gate");
    assert!(
        spec.argv
            .last()
            .unwrap()
            .contains("ONLY verification commands")
    );
}

#[test]
fn resume_context_captures() {
    let reg = Harness::new();
    run_delegation(
        RunInput {
            prompt: "plan it".into(),
            model: Some("grok-4.5-xhigh".into()),
            require_non_claude: Some(true),
            capability: Some(Capability::Plan),
            isolation: Some(Isolation::CallerProvided {
                path: "/repo".into(),
            }),
            verify_commands: Some(vec!["x test".into()]),
            gate: Some("custom-gate".into()),
            allow_partial_commit: Some(true),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec!["rm -rf /".into()])),
        Default::default(),
    )
    .unwrap();
    let ctx = reg.last().resume_context;
    assert_eq!(
        ctx,
        ResumeContext {
            model: "grok-4.5-xhigh".into(),
            require_non_claude: Some(true),
            capability: Capability::Plan,
            allow_unsandboxed: false,
            isolation: Isolation::CallerProvided {
                path: "/repo".into(),
            },
            verify_commands: Some(vec!["x test".into()]),
            gate: "custom-gate".into(),
            allow_partial_commit: true,
        }
    );
}

#[test]
fn resume_context_defaults() {
    let reg = Harness::new();
    run_delegation(
        RunInput {
            prompt: "do it".into(),
            ..Default::default()
        },
        &deps_with(&reg, Some(vec!["rm -rf /".into()])),
        Default::default(),
    )
    .unwrap();
    let ctx = reg.last().resume_context;
    assert_eq!(ctx.model, "composer-2.5");
    assert_eq!(ctx.capability, Capability::Ask);
    assert!(!ctx.allow_unsandboxed);
    assert_eq!(ctx.isolation, Isolation::None);
    assert_eq!(ctx.gate, "make ci");
    assert!(!ctx.allow_partial_commit);
    assert!(ctx.verify_commands.is_none());
    assert!(ctx.require_non_claude.is_none());
}

fn parked_ctx() -> ResumeContext {
    ResumeContext {
        model: "grok-4.5-xhigh".into(),
        require_non_claude: Some(true),
        capability: Capability::Write,
        allow_unsandboxed: false,
        isolation: Isolation::CallerProvided {
            path: "/repo".into(),
        },
        verify_commands: Some(vec!["x test".into()]),
        gate: "custom-gate".into(),
        allow_partial_commit: false,
    }
}

/// Park a job whose resume context is `parked_ctx()` and whose session is `sess-9`.
fn park(reg: &Harness) -> String {
    *reg.fake.auto.lock().unwrap() = Some(result_of(
        "which version?\nSTATUS: NEEDS_CONTEXT",
        Some("sess-9"),
    ));
    let r = reg.registry.dispatch(
        spec_of(|s| {
            bg(s);
            s.resume_context = parked_ctx();
        }),
        Default::default(),
    );
    let id = r.job_id().unwrap().to_string();
    assert_eq!(settle(&reg.registry, &id), "NEEDS_CONTEXT");
    id
}

#[test]
fn answer_resumes_with_context() {
    // A short deadline so the resumed run (left running) detaches to a new jobId.
    let reg = Harness::with_deadline(50.0);
    let parked = park(&reg);
    *reg.fake.auto.lock().unwrap() = None;
    let deps = deps_with(&reg, Some(vec!["rm -rf /".into()]));
    let res = answer_delegation(&parked, "use v2", &deps, Default::default()).unwrap();
    assert_eq!(res.status_label(), "RUNNING");
    let spec = reg.last();
    let i = spec.argv.iter().position(|s| s == "--resume").unwrap();
    assert_eq!(spec.argv[i + 1], "sess-9");
    assert!(spec.argv.last().unwrap().contains("use v2"));
    assert_eq!(spec.model, "grok-4.5-xhigh");
    assert!(spec.is_write);
    assert_eq!(spec.cwd, "/repo");
    assert_eq!(spec.path.as_deref(), Some("/repo"));
    assert_eq!(spec.gate, "custom-gate");
    assert_eq!(spec.resume_context, parked_ctx());
    let v = serde_json::to_value(reg.registry.poll(&parked)).unwrap();
    assert_eq!(v["status"], "NEEDS_CONTEXT");
    assert_eq!(v["supersededBy"], res.job_id().unwrap());
}

#[test]
fn answer_not_found() {
    let reg = Harness::new();
    let e = answer_delegation(
        "missing",
        "x",
        &deps_with(&reg, Some(vec![])),
        Default::default(),
    )
    .unwrap_err();
    assert!(e.downcast_ref::<AnswerError>().is_some());
    assert_eq!(e.to_string(), "NOT_FOUND");
}

#[test]
fn answer_not_awaiting() {
    let reg = Harness::new();
    let r = reg.registry.dispatch(spec_of(bg), Default::default());
    let id = r.job_id().unwrap().to_string();
    assert_eq!(settle(&reg.registry, &id), "DONE");
    let e = answer_delegation(&id, "x", &deps_with(&reg, Some(vec![])), Default::default())
        .unwrap_err();
    assert_eq!(e.to_string(), "job is not awaiting an answer");
    assert_eq!(reg.calls(), 1);
}
