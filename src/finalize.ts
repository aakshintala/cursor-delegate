import type { ChangeSet, FinalizeCtx, PriceMap, RunOutput } from "./types.js";
import type { BackendResult } from "./backends/types.js";
import { toRunOutput } from "./output.js";
import { computeCost } from "./pricing.js";
import { runGate as defaultRunGate } from "./gate.js";
import {
  gitDelta as defaultGitDelta,
  resolveWorktreePath as defaultResolveWorktreePath,
} from "./git.js";
import { tail } from "./util.js";

async function resolveOpsCwd(ctx: FinalizeCtx): Promise<string | null> {
  if (ctx.worktreeName === undefined) return ctx.cwd;
  const resolve = ctx.resolveWorktreePath ?? defaultResolveWorktreePath;
  return resolve(ctx.cwd, ctx.worktreeName);
}

/** #6 tool-computed git change-set (ground truth) — shared by the full finalize path and finalizeStall. */
async function computeChangeSet(
  ctx: FinalizeCtx,
): Promise<{ opsCwd: string | null; changeSet: ChangeSet | undefined }> {
  const opsCwd = await resolveOpsCwd(ctx);
  if (opsCwd === null) return { opsCwd, changeSet: undefined };
  const gitDelta = ctx.gitDelta ?? defaultGitDelta;
  const changeSet = await gitDelta(opsCwd, ctx.headBefore);
  return { opsCwd, changeSet: changeSet ?? undefined };
}

/**
 * Base RunOutput (status/text/usage/cost) shared by the full finalize path and the
 * cancel/stall short-circuit in the registry — keeps cost/jobId/downgraded logic in one place.
 */
export function baseOutput(
  res: BackendResult,
  opts: {
    model: string;
    backend: string;
    priceMap: PriceMap;
    jobId?: string;
    downgraded?: boolean;
  },
): RunOutput {
  const usage = res.raw.usage ?? null;
  const out = toRunOutput(res, {
    model: opts.model,
    backend: opts.backend,
    usage,
    costUsd: computeCost(usage, opts.priceMap, opts.model),
  });
  if (opts.jobId) out.jobId = opts.jobId;
  if (opts.downgraded) out.downgraded = true;
  return out;
}

/**
 * Assemble the terminal RunOutput (#3 stderrTail, #7 gate, #6 changeSet, #1 incomplete-commit).
 */
export async function finalizeRun(
  res: BackendResult,
  ctx: FinalizeCtx,
): Promise<RunOutput> {
  const out = baseOutput(res, {
    model: ctx.model,
    backend: ctx.backend,
    priceMap: ctx.priceMap,
    jobId: ctx.jobId,
    downgraded: ctx.downgraded,
  });

  // #3 stderrTail — only on a non-clean exit / ERROR, and only when non-empty.
  if ((!res.cleanExit || out.status === "ERROR") && res.stderr) {
    out.stderrTail = tail(res.stderr, 2048);
  }

  const concerns: string[] = [];

  const opsCwd = await resolveOpsCwd(ctx);

  // #7 gate (tool-enforced postcondition).
  if (ctx.gate) {
    const runGate = ctx.runGate ?? defaultRunGate;
    const gateResult = await runGate(ctx.gate, opsCwd ?? ctx.cwd);
    out.gateResult = gateResult;
    if (!gateResult.passed && out.status === "DONE") {
      out.status = "DONE_WITH_CONCERNS";
    }
  }

  // #6 tool-computed git change-set (ground truth).
  if (opsCwd !== null) {
    const gitDelta = ctx.gitDelta ?? defaultGitDelta;
    const changeSet = await gitDelta(opsCwd, ctx.headBefore);
    if (changeSet) {
      out.changeSet = changeSet;
      // #1 incomplete-commit concern: commits landed but the tree is still dirty.
      if (
        ctx.isWrite &&
        changeSet.newCommits.length > 0 &&
        changeSet.uncommittedFiles.length > 0 &&
        !ctx.allowPartialCommit
      ) {
        concerns.push(
          "Commits landed but the working tree is still dirty: HEAD may not reflect a " +
            "complete, buildable change. Review the uncommitted files, or pass " +
            "allowPartialCommit to suppress this.",
        );
        if (out.status === "DONE") out.status = "DONE_WITH_CONCERNS";
      }
    }
  }

  if (concerns.length > 0) out.concerns = concerns;
  return out;
}

/**
 * Terminal RunOutput for a CANCELLED/STALLED job (#9). Deliberately narrower than
 * `finalizeRun`: no gate (meaningless against a run that was killed mid-flight, and
 * potentially expensive) and no incomplete-commit concern (that check assumes a run that
 * finished on its own terms). It still computes the #6 change-set — whatever the agent
 * already wrote to disk before the kill is exactly what a caller needs to decide whether to
 * keep, discard, or redispatch, and skipping it left every terminated job with `text: ""`
 * and no idea what (if anything) happened.
 */
export async function finalizeStall(
  res: BackendResult,
  ctx: FinalizeCtx,
): Promise<RunOutput> {
  const out = baseOutput(res, {
    model: ctx.model,
    backend: ctx.backend,
    priceMap: ctx.priceMap,
    jobId: ctx.jobId,
    downgraded: ctx.downgraded,
  });

  if (res.stderr) out.stderrTail = tail(res.stderr, 2048);

  const { changeSet } = await computeChangeSet(ctx);
  if (changeSet) out.changeSet = changeSet;

  return out;
}
