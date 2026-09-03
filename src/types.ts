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
  /** Per-call idle watchdog override. `null` disables it for this job. Falls back to the server default when omitted. */
  idleMs?: number | null;
  /** Per-call override of the in-tool idle window. `null` disables it for this job. Falls back to the server default when omitted. */
  toolIdleMs?: number | null;
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
  /** Set when the gate was killed by its timeout or an abort signal. */
  error?: string;
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
  /** Idle window applied while a tool call is in flight (no event since it started). Wider than idleMs; a hung build/test still eventually dies. */
  toolIdleMs?: number | null;
}

export interface Config {
  default: string;
  models: Record<string, ModelEntry>;
  /** Derived from `models` at load time for `computeCost` / JobSpec. */
  priceMap: PriceMap;
  profile: HostProfile;
}

export interface DoctorPluginInfo {
  version: string;
}

export interface DoctorAgentInfo {
  found: boolean;
  path: string | null;
  version: string | null;
  error?: string;
}

export interface DoctorAccountInfo {
  loggedIn: boolean;
  email: string | null;
  subscription: string | null;
  currentModel: string | null;
  error?: string;
}

export interface DoctorModelMenuInfo {
  configuredIds: string[];
  accountIds: string[] | null;
  missingFromAccount: string[];
  /** CLI exposes no pricing; always false. */
  pricesCheckable: false;
  note: string;
  error?: string;
}

export interface PluginRegistrationCheck {
  /** settings.json enabledPlugins[pluginId] === true */
  enabled: boolean;
  /** `claude mcp get <serverName>` exited 0 — some server is live under this name, plugin- or
   * raw-sourced. */
  reachable: boolean;
  /** Only meaningful when `reachable`: the live server under `serverName` is plugin-launched
   * — Claude Code injected `CLAUDE_PLUGIN_ROOT` into its Environment block — not a raw
   * hand-added registration. */
  resolvesToPluginInstall: boolean;
  /** `claude mcp get` for the bare legacy name exited non-zero — no raw `cursor-delegate`
   * registration is live (desired post-migration state). */
  legacyAbsent: boolean;
  ok: boolean;
  /** Human-readable detail lines, one per failing sub-check. */
  detail: string[];
}

export interface DoctorReport {
  ok: boolean;
  plugin: DoctorPluginInfo;
  agent: DoctorAgentInfo;
  account: DoctorAccountInfo;
  modelMenu: DoctorModelMenuInfo;
  pluginRegistration: PluginRegistrationCheck;
  warnings: string[];
  failures: string[];
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
  /** Per-call idle watchdog override. `null` disables it for this job. Falls back to the server default when omitted. */
  idleMs?: number | null;
  /** Per-call override of the in-tool idle window. `null` disables it for this job. Falls back to the server default when omitted. */
  toolIdleMs?: number | null;
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
  | {
      status: "RUNNING";
      progress: ProgressSnapshot;
      /** Server clock at the last record refresh; consumers compare against their own clock to detect a dead server. */
      lastHeartbeatAt?: number;
      supersededBy?: string;
    }
  | {
      status: Exclude<JobStatus, "RUNNING">;
      result: RunOutput;
      /** Set when a resume of this job (e.g. after NEEDS_CONTEXT) continues under a new jobId. */
      supersededBy?: string;
    }
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
  runGate?: (
    command: string,
    cwd: string,
    opts?: { signal?: AbortSignal },
  ) => Promise<GateResult>;
  /** Abort signal for bounded finalize: cancel/shutdown escalates into a hung gate. */
  signal?: AbortSignal;
  gitDelta?: (cwd: string, headBefore: string | null) => Promise<ChangeSet | null>;
  /** When set, change-set and gate run in the resolved worktree under `cwd`. */
  worktreeName?: string;
  resolveWorktreePath?: (
    repoCwd: string,
    name?: string,
  ) => Promise<string | null>;
}
