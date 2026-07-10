// All domain types for cursor-delegate.

export type Capability = "ask" | "plan" | "write" | "write-unsandboxed";

export type Isolation =
  | { type: "None" }
  | { type: "CallerProvided"; path: string }
  | { type: "BackendProvided"; name?: string; base?: string };

export interface RunInput {
  prompt: string;
  model?: string;
  requireNonClaude?: boolean;
  capability?: Capability;
  allowUnsandboxed?: boolean;
  session?: string;
  isolation?: Isolation;
  verifyCommands?: string[];
  gate?: string;
  allowPartialCommit?: boolean;
  waitMs?: number;
  background?: boolean;
}

export interface Usage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
}

export type RunStatus =
  | "DONE"
  | "DONE_WITH_CONCERNS"
  | "BLOCKED"
  | "NEEDS_CONTEXT"
  | "ERROR";

export const RUN_STATUSES: RunStatus[] = [
  "DONE",
  "DONE_WITH_CONCERNS",
  "BLOCKED",
  "NEEDS_CONTEXT",
  "ERROR",
];

export interface ChangeSet {
  headBefore: string | null;
  headAfter: string | null;
  newCommits: string[];
  filesChanged: string[];
  diffstat: string;
  uncommittedFiles: string[];
  dirtyAfter: boolean;
}

export interface GateResult {
  command: string;
  exitCode: number;
  passed: boolean;
  outputTail: string;
}

export interface RunOutput {
  status: RunStatus;
  text: string;
  sessionId: string | null;
  backend: string;
  model: string;
  usage: Usage | null;
  costUsd: number | null;
  costEstimated: boolean;
  durationMs: number | null;
  stderrTail?: string;
  changeSet?: ChangeSet;
  gateResult?: GateResult;
  jobId?: string;
  concerns?: string[];
  downgraded?: boolean;
}

// Raw shape emitted by the cursor-agent `result` event.
export interface RawCursorJson {
  type?: string;
  subtype?: string;
  is_error?: boolean;
  duration_ms?: number;
  duration_api_ms?: number;
  result?: string;
  session_id?: string;
  request_id?: string;
  usage?: Usage;
}

export interface Price {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
}
export type PriceMap = Record<string, Price>;

export interface ModelEntry {
  label: string;
  family: string;
  price: Price;
}

export interface ResolvedModel {
  model: string;
  family: string;
  price: Price;
}

export interface HostProfile {
  default?: string;
  models?: Record<string, ModelEntry>;
  requiredDeny?: string[];
  promptPreamble?: string;
  verifyCommands?: string[];
  gate?: string;
  deadlineMs?: number;
  idleMs?: number | null;
}

export interface Config {
  default: string;
  models: Record<string, ModelEntry>;
  /** Derived from `models` at load time for `computeCost` / JobSpec. */
  priceMap: PriceMap;
  profile: HostProfile;
}

/** Fields needed to resume a parked NEEDS_CONTEXT job via cursor_answer. */
export interface ResumeContext {
  model: string;
  requireNonClaude?: boolean;
  capability: Capability;
  allowUnsandboxed: boolean;
  isolation: Isolation;
  verifyCommands?: string[];
  gate: string;
  allowPartialCommit: boolean;
}

// The fully-resolved unit of work handed from runner to registry/backend.
export interface JobSpec {
  bin: string;
  argv: string[];
  cwd: string;
  model: string;
  backend: string;
  isWrite: boolean;
  path: string | null; // CallerProvided path for the write lock, else null
  headBefore: string | null;
  gate: string; // "" = no gate
  allowPartialCommit: boolean;
  waitMs?: number;
  background?: boolean;
  priceMap: PriceMap;
  downgraded: boolean;
  /** Set for BackendProvided isolation; used to resolve the worktree for change-set tracking. */
  worktreeName?: string;
  /** Original run knobs for cursor_answer resume. */
  resumeContext: ResumeContext;
}

export type JobStatus =
  | "RUNNING"
  | "DONE"
  | "DONE_WITH_CONCERNS"
  | "BLOCKED"
  | "NEEDS_CONTEXT"
  | "ERROR"
  | "CANCELLED"
  | "STALLED";

export type PollStatus = JobStatus | "NOT_FOUND";

export interface ProgressSnapshot {
  lastTool: string | null;
  tokensSoFar: number;
  elapsedMs: number;
  lastAssistant: string | null;
  filesTouchedSoFar: string[];
  phase: string | null;
}

export type PollResult =
  | { status: "RUNNING"; progress: ProgressSnapshot }
  | { status: Exclude<JobStatus, "RUNNING">; result: RunOutput }
  | { status: "NOT_FOUND" };

export type DispatchResult =
  | RunOutput
  | { status: "RUNNING"; jobId: string; busyPath?: string }
  | { status: "BUSY"; jobId: string; busyPath: string };

// Context passed to finalize. Impure deps are injectable (structural) for tests.
export interface FinalizeCtx {
  cwd: string;
  headBefore: string | null;
  isWrite: boolean;
  gate: string;
  allowPartialCommit: boolean;
  model: string;
  backend: string;
  priceMap: PriceMap;
  jobId?: string;
  downgraded?: boolean;
  runGate?: (command: string, cwd: string) => Promise<GateResult>;
  gitDelta?: (cwd: string, headBefore: string | null) => Promise<ChangeSet | null>;
  /** When set, change-set and gate run in the resolved worktree under `cwd`. */
  worktreeName?: string;
  resolveWorktreePath?: (
    repoCwd: string,
    name?: string,
  ) => Promise<string | null>;
}
