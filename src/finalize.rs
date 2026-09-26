use crate::backends::types::BackendResult;
use crate::output::to_run_output;
use crate::pricing::compute_cost;
use crate::types::{ChangeSet, FinalizeCtx, GateResult, PriceMap, RunOutput, RunStatus};
use crate::util::Abort;
use crate::util::tail;

fn resolve_ops_cwd(ctx: &FinalizeCtx) -> Option<String> {
    if ctx.worktree_name.is_none() {
        return Some(ctx.cwd.clone());
    }
    if let Some(r) = &ctx.resolve_worktree_path {
        return r(&ctx.cwd, ctx.worktree_name.as_deref());
    }
    crate::git::resolve_worktree_path(&ctx.cwd, ctx.worktree_name.as_deref())
}

fn compute_change_set(ctx: &FinalizeCtx) -> (Option<String>, Option<ChangeSet>) {
    let ops_cwd = resolve_ops_cwd(ctx);
    let Some(cwd) = ops_cwd.clone() else {
        return (ops_cwd, None);
    };
    let cs = if let Some(g) = &ctx.git_delta {
        g(&cwd, ctx.head_before.as_deref())
    } else {
        crate::git::git_delta(&cwd, ctx.head_before.as_deref())
    };
    (ops_cwd, cs)
}

pub fn base_output(
    res: &BackendResult,
    model: &str,
    backend: &str,
    price_map: &PriceMap,
    job_id: Option<&str>,
    downgraded: bool,
) -> RunOutput {
    let usage = res.raw.usage.clone();
    let mut out = to_run_output(
        res,
        model,
        backend,
        usage.clone(),
        compute_cost(usage.as_ref(), price_map, model),
    );
    if let Some(id) = job_id {
        out.job_id = Some(id.to_string());
    }
    if downgraded {
        out.downgraded = Some(true);
    }
    out
}

fn run_gate_ctx(ctx: &FinalizeCtx, cwd: &str) -> GateResult {
    if let Some(rg) = &ctx.run_gate {
        rg(&ctx.gate, cwd, ctx.signal.as_ref())
    } else {
        crate::gate::run_gate(
            &ctx.gate,
            cwd,
            crate::gate::GateOpts {
                timeout_ms: None,
                signal: ctx.signal.as_ref(),
            },
        )
    }
}

pub fn finalize_run(res: &BackendResult, ctx: &FinalizeCtx) -> RunOutput {
    let mut out = base_output(
        res,
        &ctx.model,
        &ctx.backend,
        &ctx.price_map,
        ctx.job_id.as_deref(),
        ctx.downgraded,
    );
    if (!res.clean_exit || out.status == RunStatus::Error) && !res.stderr.is_empty() {
        out.stderr_tail = Some(tail(&res.stderr, 2048));
    }
    let mut concerns: Vec<String> = Vec::new();
    let ops_cwd = resolve_ops_cwd(ctx);
    if !ctx.gate.is_empty() {
        let cwd = ops_cwd.clone().unwrap_or_else(|| ctx.cwd.clone());
        let gate_result = run_gate_ctx(ctx, &cwd);
        if !gate_result.passed && out.status == RunStatus::Done {
            out.status = RunStatus::DoneWithConcerns;
        }
        out.gate_result = Some(gate_result);
    }
    if let Some(cwd) = ops_cwd {
        let change_set = if let Some(g) = &ctx.git_delta {
            g(&cwd, ctx.head_before.as_deref())
        } else {
            crate::git::git_delta(&cwd, ctx.head_before.as_deref())
        };
        if let Some(cs) = change_set {
            if ctx.is_write
                && !cs.new_commits.is_empty()
                && !cs.uncommitted_files.is_empty()
                && !ctx.allow_partial_commit
            {
                concerns.push(
                    "Commits landed but the working tree is still dirty: HEAD may not reflect a \
complete, buildable change. Review the uncommitted files, or pass \
allowPartialCommit to suppress this."
                        .into(),
                );
                if out.status == RunStatus::Done {
                    out.status = RunStatus::DoneWithConcerns;
                }
            }
            out.change_set = Some(cs);
        }
    }
    if !concerns.is_empty() {
        out.concerns = Some(concerns);
    }
    out
}

pub fn finalize_stall(res: &BackendResult, ctx: &FinalizeCtx) -> RunOutput {
    let mut out = base_output(
        res,
        &ctx.model,
        &ctx.backend,
        &ctx.price_map,
        ctx.job_id.as_deref(),
        ctx.downgraded,
    );
    if !res.stderr.is_empty() {
        out.stderr_tail = Some(tail(&res.stderr, 2048));
    }
    let (_, change_set) = compute_change_set(ctx);
    if let Some(cs) = change_set {
        out.change_set = Some(cs);
    }
    out
}

pub fn default_finalize_ctx(cwd: &str, model: &str, backend: &str) -> FinalizeCtx {
    FinalizeCtx {
        cwd: cwd.into(),
        head_before: None,
        is_write: false,
        gate: String::new(),
        allow_partial_commit: false,
        model: model.into(),
        backend: backend.into(),
        price_map: Default::default(),
        job_id: None,
        downgraded: false,
        run_gate: None,
        signal: None::<Abort>,
        git_delta: None,
        worktree_name: None,
        resolve_worktree_path: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChangeSet, GateResult, RawCursorJson, RunStatus};

    fn base_ctx() -> FinalizeCtx {
        let mut ctx = default_finalize_ctx("/repo", "composer-2.5", "cursor");
        ctx.git_delta = Some(Box::new(|_, _| None));
        ctx
    }

    fn ok_result() -> BackendResult {
        BackendResult {
            raw: RawCursorJson {
                result: Some("done\nSTATUS: DONE".into()),
                is_error: Some(false),
                ..RawCursorJson::default()
            },
            clean_exit: true,
            stderr: String::new(),
        }
    }

    fn dirty() -> ChangeSet {
        ChangeSet {
            head_before: Some("aaa".into()),
            head_after: Some("bbb".into()),
            new_commits: vec!["bbb".into()],
            files_changed: vec!["x".into()],
            diffstat: "x | 1 +".into(),
            uncommitted_files: vec!["y".into()],
            dirty_after: true,
        }
    }

    #[test]
    fn stderr_absent_on_clean_done() {
        let mut res = ok_result();
        res.stderr = "some noise".into();
        let out = finalize_run(&res, &base_ctx());
        assert_eq!(out.status, RunStatus::Done);
        assert!(out.stderr_tail.is_none());
    }

    #[test]
    fn stderr_present_on_non_clean() {
        let res = BackendResult {
            raw: RawCursorJson::default(),
            clean_exit: false,
            stderr: "boom".into(),
        };
        let out = finalize_run(&res, &base_ctx());
        assert_eq!(out.status, RunStatus::Error);
        assert_eq!(out.stderr_tail.as_deref(), Some("boom"));
    }

    #[test]
    fn failing_gate_downgrades() {
        let mut ctx = base_ctx();
        ctx.gate = "make test".into();
        ctx.run_gate = Some(Box::new(|_, _, _| GateResult {
            command: "make test".into(),
            exit_code: 1,
            passed: false,
            output_tail: "FAIL".into(),
            error: None,
        }));
        let out = finalize_run(&ok_result(), &ctx);
        assert_eq!(out.status, RunStatus::DoneWithConcerns);
        assert_eq!(out.gate_result.as_ref().unwrap().exit_code, 1);
    }

    #[test]
    fn passing_gate_leaves_done() {
        let mut ctx = base_ctx();
        ctx.gate = "make test".into();
        ctx.run_gate = Some(Box::new(|_, _, _| GateResult {
            command: "make test".into(),
            exit_code: 0,
            passed: true,
            output_tail: String::new(),
            error: None,
        }));
        let out = finalize_run(&ok_result(), &ctx);
        assert_eq!(out.status, RunStatus::Done);
    }

    #[test]
    fn incomplete_commit_downgrades_write() {
        let d = dirty();
        let mut ctx = base_ctx();
        ctx.is_write = true;
        ctx.head_before = Some("aaa".into());
        ctx.git_delta = Some(Box::new(move |_, _| Some(d.clone())));
        let out = finalize_run(&ok_result(), &ctx);
        assert_eq!(out.status, RunStatus::DoneWithConcerns);
        assert_eq!(out.concerns.as_ref().map(|c| c.len()), Some(1));
    }

    #[test]
    fn allow_partial_commit_suppresses() {
        let d = dirty();
        let mut ctx = base_ctx();
        ctx.is_write = true;
        ctx.allow_partial_commit = true;
        ctx.head_before = Some("aaa".into());
        ctx.git_delta = Some(Box::new(move |_, _| Some(d.clone())));
        let out = finalize_run(&ok_result(), &ctx);
        assert_eq!(out.status, RunStatus::Done);
        assert!(out.concerns.is_none());
    }

    #[test]
    fn gate_failure_on_error_stays_error() {
        let mut ctx = base_ctx();
        ctx.gate = "make test".into();
        ctx.run_gate = Some(Box::new(|_, _, _| GateResult {
            command: "make test".into(),
            exit_code: 1,
            passed: false,
            output_tail: "FAIL".into(),
            error: None,
        }));
        let res = BackendResult {
            raw: RawCursorJson::default(),
            clean_exit: false,
            stderr: "boom".into(),
        };
        let out = finalize_run(&res, &ctx);
        assert_eq!(out.status, RunStatus::Error);
    }

    #[test]
    fn gate_failure_leaves_needs_context() {
        let mut ctx = base_ctx();
        ctx.gate = "make test".into();
        ctx.run_gate = Some(Box::new(|_, _, _| GateResult {
            command: "make test".into(),
            exit_code: 1,
            passed: false,
            output_tail: "FAIL".into(),
            error: None,
        }));
        let res = BackendResult {
            raw: RawCursorJson {
                result: Some("q?\nSTATUS: NEEDS_CONTEXT".into()),
                is_error: Some(false),
                ..RawCursorJson::default()
            },
            clean_exit: true,
            stderr: String::new(),
        };
        let out = finalize_run(&res, &ctx);
        assert_eq!(out.status, RunStatus::NeedsContext);
    }

    #[test]
    fn read_only_never_concern() {
        let d = dirty();
        let mut ctx = base_ctx();
        ctx.is_write = false;
        ctx.head_before = Some("aaa".into());
        ctx.git_delta = Some(Box::new(move |_, _| Some(d.clone())));
        let out = finalize_run(&ok_result(), &ctx);
        assert_eq!(out.status, RunStatus::Done);
        assert!(out.concerns.is_none());
        assert!(out.change_set.is_some());
    }

    #[test]
    fn write_clean_tree_no_concern() {
        let mut clean = dirty();
        clean.uncommitted_files.clear();
        clean.dirty_after = false;
        let mut ctx = base_ctx();
        ctx.is_write = true;
        ctx.head_before = Some("aaa".into());
        ctx.git_delta = Some(Box::new(move |_, _| Some(clean.clone())));
        let out = finalize_run(&ok_result(), &ctx);
        assert_eq!(out.status, RunStatus::Done);
        assert!(out.concerns.is_none());
    }

    #[test]
    fn job_id_and_downgraded_propagate() {
        let mut ctx = base_ctx();
        ctx.job_id = Some("job-1".into());
        ctx.downgraded = true;
        let out = finalize_run(&ok_result(), &ctx);
        assert_eq!(out.job_id.as_deref(), Some("job-1"));
        assert_eq!(out.downgraded, Some(true));
    }

    fn stalled() -> BackendResult {
        BackendResult {
            raw: RawCursorJson::default(),
            clean_exit: false,
            stderr: String::new(),
        }
    }

    #[test]
    fn stall_computes_changeset() {
        let d = dirty();
        let mut ctx = base_ctx();
        ctx.is_write = true;
        ctx.head_before = Some("aaa".into());
        ctx.git_delta = Some(Box::new(move |_, _| Some(d.clone())));
        let out = finalize_stall(&stalled(), &ctx);
        assert_eq!(out.change_set.as_ref().unwrap().new_commits, ["bbb"]);
    }

    #[test]
    fn stall_never_runs_gate() {
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ran2 = std::sync::Arc::clone(&ran);
        let mut ctx = base_ctx();
        ctx.gate = "make test".into();
        ctx.run_gate = Some(Box::new(move |_, _, _| {
            ran2.store(true, std::sync::atomic::Ordering::SeqCst);
            GateResult {
                command: "make test".into(),
                exit_code: 0,
                passed: true,
                output_tail: String::new(),
                error: None,
            }
        }));
        let out = finalize_stall(&stalled(), &ctx);
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        assert!(out.gate_result.is_none());
    }

    #[test]
    fn stall_no_incomplete_commit() {
        let d = dirty();
        let mut ctx = base_ctx();
        ctx.is_write = true;
        ctx.head_before = Some("aaa".into());
        ctx.git_delta = Some(Box::new(move |_, _| Some(d.clone())));
        let out = finalize_stall(&stalled(), &ctx);
        assert!(out.concerns.is_none());
    }

    #[test]
    fn stall_stderr() {
        let mut res = stalled();
        res.stderr = "boom".into();
        let out = finalize_stall(&res, &base_ctx());
        assert_eq!(out.stderr_tail.as_deref(), Some("boom"));
    }

    #[test]
    fn stall_jobid_downgraded() {
        let mut ctx = base_ctx();
        ctx.job_id = Some("job-9".into());
        ctx.downgraded = true;
        let out = finalize_stall(&stalled(), &ctx);
        assert_eq!(out.job_id.as_deref(), Some("job-9"));
        assert_eq!(out.downgraded, Some(true));
    }

    #[test]
    fn stall_resolves_worktree() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let seen2 = std::sync::Arc::clone(&seen);
        let d = dirty();
        let mut ctx = base_ctx();
        ctx.cwd = "/repo".into();
        ctx.worktree_name = Some("wt-side".into());
        ctx.resolve_worktree_path = Some(Box::new(|_, _| Some("/repo/wt-side".into())));
        ctx.git_delta = Some(Box::new(move |cwd, _| {
            *seen2.lock().unwrap() = cwd.to_string();
            Some(d.clone())
        }));
        let out = finalize_stall(&stalled(), &ctx);
        assert_eq!(seen.lock().unwrap().as_str(), "/repo/wt-side");
        assert!(out.change_set.is_some());
    }
}
