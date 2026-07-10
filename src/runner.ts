import type {
  Config,
  DispatchResult,
  JobSpec,
  ResumeContext,
  RunInput,
} from "./types.js";
import type { CliConfig } from "./safety.js";
import type { JobRegistry, DispatchOpts } from "./job-registry.js";
import { resolveModel } from "./models.js";
import { mapCapability } from "./capability.js";
import { mapIsolation } from "./isolation.js";
import { verifyDenyList } from "./safety.js";
import { composePrompt } from "./prompt.js";
import { captureHead } from "./git.js";
import { resolveCursorBin } from "./cursor-bin.js";

export interface RunnerDeps {
  config: Config;
  registry: JobRegistry;
  cliConfig: CliConfig | null;
  serverCwd: string;
  resolveBin?: (override?: string) => string;
  captureHead?: (cwd: string, ref?: string) => Promise<string | null>;
}

/** Build the cursor-agent argv (pure). The prompt is always the last positional arg. */
export function buildArgv(o: {
  model: string;
  capFlags: string[];
  isoFlags: string[];
  session?: string;
  prompt: string;
}): string[] {
  const argv = [
    "--print",
    "--output-format",
    "stream-json",
    "--trust",
    "--approve-mcps",
    "--model",
    o.model,
    ...o.capFlags,
    ...o.isoFlags,
  ];
  if (o.session) argv.push("--resume", o.session);
  argv.push("--", o.prompt);
  return argv;
}

/**
 * Pure pre-flight: resolve model, map capability, verify deny-list, map isolation,
 * compose the prompt, capture HEAD, build a JobSpec, then hand to the registry.
 */
export async function runDelegation(
  input: RunInput,
  deps: RunnerDeps,
  opts: DispatchOpts = {},
): Promise<DispatchResult> {
  const { config } = deps;

  const resolved = resolveModel(
    { model: input.model, requireNonClaude: input.requireNonClaude },
    { default: config.default, models: config.models },
  );

  const capability = input.capability ?? "ask";
  const allowUnsandboxed = input.allowUnsandboxed ?? false;
  const isolation = input.isolation ?? { type: "None" as const };
  const allowPartialCommit = input.allowPartialCommit ?? false;

  const cap = mapCapability(capability, allowUnsandboxed);

  if (cap.isWrite) {
    verifyDenyList(config.profile.requiredDeny ?? [], deps.cliConfig);
  }

  const iso = mapIsolation(isolation, deps.serverCwd);

  const prompt = composePrompt({
    preamble: config.profile.promptPreamble,
    verifyCommands: input.verifyCommands ?? config.profile.verifyCommands,
    prompt: input.prompt,
  });

  const gate = input.gate ?? config.profile.gate ?? "";

  const captureHeadFn = deps.captureHead ?? captureHead;
  let headBefore: string | null;
  let worktreeName: string | undefined;
  if (isolation.type === "BackendProvided") {
    worktreeName = isolation.name;
    // A worktree forks from `base` (or server HEAD) at creation, so its base commit is
    // resolvable in the server repo now — even though the worktree itself may not exist
    // yet. finalize resolves the worktree dir post-run and diffs it against this base.
    headBefore = await captureHeadFn(deps.serverCwd, isolation.base);
  } else {
    headBefore = await captureHeadFn(iso.cwd);
  }

  const bin = (deps.resolveBin ?? resolveCursorBin)();
  const argv = buildArgv({
    model: resolved.model,
    capFlags: cap.flags,
    isoFlags: iso.flags,
    session: input.session,
    prompt,
  });

  const resumeContext: ResumeContext = {
    model: resolved.model,
    capability,
    allowUnsandboxed,
    isolation,
    gate,
    allowPartialCommit,
  };
  if (input.requireNonClaude !== undefined) {
    resumeContext.requireNonClaude = input.requireNonClaude;
  }
  if (input.verifyCommands !== undefined) {
    resumeContext.verifyCommands = input.verifyCommands;
  }

  const spec: JobSpec = {
    bin,
    argv,
    cwd: iso.cwd,
    model: resolved.model,
    backend: "cursor",
    isWrite: cap.isWrite,
    path: iso.path,
    headBefore,
    gate,
    allowPartialCommit,
    waitMs: input.waitMs,
    background: input.background,
    priceMap: config.priceMap,
    downgraded: cap.downgraded,
    worktreeName,
    resumeContext,
  };

  return deps.registry.dispatch(spec, opts);
}
