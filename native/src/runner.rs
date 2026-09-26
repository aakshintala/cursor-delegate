use crate::capability::map_capability;
use crate::git::capture_head as capture_head_real;
use crate::isolation::map_isolation;
use crate::job_registry::{AnswerLookup, JobRegistry, WaitOpts};
use crate::models::resolve_model;
use crate::prompt::compose_prompt;
use crate::safety::{CliConfig, verify_deny_list};
use crate::types::{
    Capability, Config, DispatchResult, Isolation, JobSpec, ResumeContext, RunInput,
};
use std::sync::Arc;

pub type ResolveBinFn = Arc<dyn Fn(Option<&str>) -> String + Send + Sync>;
pub type CaptureHeadFn = Arc<dyn Fn(&str, Option<&str>) -> Option<String> + Send + Sync>;

pub struct RunnerDeps {
    pub config: Config,
    pub registry: Arc<JobRegistry>,
    pub cli_config: Option<CliConfig>,
    pub server_cwd: String,
    pub resolve_bin: Option<ResolveBinFn>,
    pub capture_head: Option<CaptureHeadFn>,
}

pub fn build_argv(
    model: &str,
    cap_flags: &[String],
    iso_flags: &[String],
    session: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    let mut argv = vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--trust".into(),
        "--approve-mcps".into(),
        "--model".into(),
        model.to_string(),
    ];
    argv.extend(cap_flags.iter().cloned());
    argv.extend(iso_flags.iter().cloned());
    if let Some(s) = session {
        argv.push("--resume".into());
        argv.push(s.to_string());
    }
    argv.push("--".into());
    argv.push(prompt.to_string());
    argv
}

pub fn run_delegation(
    input: RunInput,
    deps: &RunnerDeps,
    opts: WaitOpts<'_>,
) -> Result<DispatchResult, Box<dyn std::error::Error + Send + Sync>> {
    let config = &deps.config;
    let resolved = resolve_model(
        input.model.as_deref(),
        input.require_non_claude.unwrap_or(false),
        config,
    )?;

    let capability = input.capability.unwrap_or(Capability::Ask);
    let allow_unsandboxed = input.allow_unsandboxed.unwrap_or(false);
    let isolation = input.isolation.clone().unwrap_or(Isolation::None);
    let allow_partial_commit = input.allow_partial_commit.unwrap_or(false);

    let cap = map_capability(capability, allow_unsandboxed);

    if cap.forced {
        verify_deny_list(
            config.profile.required_deny.as_deref().unwrap_or(&[]),
            deps.cli_config.as_ref(),
        )?;
    }

    let iso = map_isolation(&isolation, &deps.server_cwd);

    let prompt = compose_prompt(
        config.profile.prompt_preamble.as_deref(),
        input
            .verify_commands
            .as_deref()
            .or(config.profile.verify_commands.as_deref()),
        &input.prompt,
    );

    let gate = input
        .gate
        .clone()
        .or_else(|| config.profile.gate.clone())
        .unwrap_or_default();

    let capture = |cwd: &str, r#ref: Option<&str>| {
        if let Some(f) = &deps.capture_head {
            f(cwd, r#ref)
        } else {
            capture_head_real(cwd, r#ref)
        }
    };

    let (head_before, worktree_name) = match &isolation {
        Isolation::BackendProvided { name, base } => {
            (capture(&deps.server_cwd, base.as_deref()), name.clone())
        }
        _ => (capture(&iso.cwd, None), None),
    };

    let bin = if let Some(f) = &deps.resolve_bin {
        f(None)
    } else {
        crate::cursor_bin::resolve_cursor_bin(None)
    };
    let argv = build_argv(
        &resolved.model,
        &cap.flags,
        &iso.flags,
        input.session.as_deref(),
        &prompt,
    );

    let mut resume_context = ResumeContext {
        model: resolved.model.clone(),
        require_non_claude: None,
        capability,
        allow_unsandboxed,
        isolation,
        verify_commands: None,
        gate: gate.clone(),
        allow_partial_commit,
    };
    if input.require_non_claude.is_some() {
        resume_context.require_non_claude = input.require_non_claude;
    }
    if input.verify_commands.is_some() {
        resume_context.verify_commands = input.verify_commands.clone();
    }

    let spec = JobSpec {
        bin,
        argv,
        cwd: iso.cwd,
        model: resolved.model,
        backend: "cursor".into(),
        is_write: cap.is_write,
        path: iso.path,
        head_before,
        gate,
        allow_partial_commit,
        wait_ms: input.wait_ms,
        idle_ms: input.idle_ms,
        tool_idle_ms: input.tool_idle_ms,
        background: input.background,
        price_map: config.price_map.clone(),
        downgraded: cap.downgraded,
        worktree_name,
        resume_context,
    };

    Ok(deps.registry.dispatch(spec, opts))
}

#[derive(Debug)]
pub enum AnswerError {
    NotFound,
    NotAwaiting,
}

impl std::fmt::Display for AnswerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("NOT_FOUND"),
            Self::NotAwaiting => f.write_str("job is not awaiting an answer"),
        }
    }
}
impl std::error::Error for AnswerError {}

pub fn answer_delegation(
    job_id: &str,
    answer: &str,
    deps: &RunnerDeps,
    opts: WaitOpts<'_>,
) -> Result<DispatchResult, Box<dyn std::error::Error + Send + Sync>> {
    match deps.registry.lookup_answer(job_id) {
        AnswerLookup::NotFound => Err(Box::new(AnswerError::NotFound)),
        AnswerLookup::NotAwaiting { .. } => Err(Box::new(AnswerError::NotAwaiting)),
        AnswerLookup::Ok {
            session_id,
            resume_context,
        } => {
            let result = run_delegation(
                RunInput {
                    prompt: answer.to_string(),
                    session: Some(session_id),
                    model: Some(resume_context.model),
                    require_non_claude: resume_context.require_non_claude,
                    capability: Some(resume_context.capability),
                    allow_unsandboxed: Some(resume_context.allow_unsandboxed),
                    isolation: Some(resume_context.isolation),
                    verify_commands: resume_context.verify_commands,
                    gate: Some(resume_context.gate),
                    allow_partial_commit: Some(resume_context.allow_partial_commit),
                    ..RunInput::default()
                },
                deps,
                opts,
            )?;
            // The old record stays NEEDS_CONTEXT forever — point watchers at the run that
            // actually continues the work. Only when the resume detached to a new jobId.
            if let DispatchResult::Detached { job_id: new_id, .. } = &result {
                deps.registry.mark_superseded(job_id, new_id);
            }
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests;
