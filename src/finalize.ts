import type { FinalizeCtx, RunOutput } from "./types.js";
import type { BackendResult } from "./backends/types.js";
import { toRunOutput } from "./output.js";
import { computeCost } from "./pricing.js";
import { runGate as defaultRunGate } from "./gate.js";
import { gitDelta as defaultGitDelta } from "./git.js";
import { tail } from "./util.js";

/**
 * Assemble the terminal RunOutput (#3 stderrTail, #7 gate, #6 changeSet, #1 incomplete-commit).
 */
export async function finalizeRun(
  res: BackendResult,
  ctx: FinalizeCtx,
): Promise<RunOutput> {
  const usage = res.raw.usage ?? null;
  const costUsd = computeCost(usage, ctx.priceMap, ctx.model);

  const out = toRunOutput(res, {
    model: ctx.model,
    backend: ctx.backend,
    usage,
    costUsd,
  });
  if (ctx.jobId) out.jobId = ctx.jobId;
  if (ctx.downgraded) out.downgraded = true;

  // #3 stderrTail — only on a non-clean exit / ERROR, and only when non-empty.
  if ((!res.cleanExit || out.status === "ERROR") && res.stderr) {
    out.stderrTail = tail(res.stderr, 2048);
  }

  const concerns: string[] = [];

  // #7 gate (tool-enforced postcondition).
  if (ctx.gate) {
    const runGate = ctx.runGate ?? defaultRunGate;
    const gateResult = await runGate(ctx.gate, ctx.cwd);
    out.gateResult = gateResult;
    if (!gateResult.passed && out.status === "DONE") {
      out.status = "DONE_WITH_CONCERNS";
    }
  }

  // #6 tool-computed git change-set (ground truth).
  const gitDelta = ctx.gitDelta ?? defaultGitDelta;
  const changeSet = await gitDelta(ctx.cwd, ctx.headBefore);
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

  if (concerns.length > 0) out.concerns = concerns;
  return out;
}
