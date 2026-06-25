// JSON Schemas + descriptions for the six MCP tools (model-facing).

export const RUN_INPUT_SCHEMA = {
  type: "object",
  properties: {
    prompt: {
      type: "string",
      description: "The task for the delegated Cursor agent. Required.",
    },
    tier: {
      type: "string",
      enum: ["cheap-bulk", "standard", "coding-specialist", "diversity"],
      description:
        "Symbolic model selector. 'diversity' is contractually non-Claude (uncorrelated second opinion). Ignored if `model` is set.",
    },
    model: {
      type: "string",
      description: "Raw model id; bypasses tier resolution.",
    },
    capability: {
      type: "string",
      enum: ["ask", "plan", "write", "write-unsandboxed"],
      description:
        "ask/plan are read-only. write runs sandboxed+non-interactive. write-unsandboxed also needs allowUnsandboxed:true (else it is downgraded to write). Default 'ask'.",
    },
    allowUnsandboxed: {
      type: "boolean",
      description: "Required second signal to actually run write-unsandboxed.",
    },
    session: {
      type: "string",
      description: "Resume a prior sessionId for continuity.",
    },
    isolation: {
      description:
        "Where the agent works. {type:'None'} = server cwd; {type:'CallerProvided',path} = a named workspace (participates in the write lock); {type:'BackendProvided',name?,base?} = a cursor worktree.",
      oneOf: [
        {
          type: "object",
          properties: { type: { const: "None" } },
          required: ["type"],
        },
        {
          type: "object",
          properties: {
            type: { const: "CallerProvided" },
            path: { type: "string" },
          },
          required: ["type", "path"],
        },
        {
          type: "object",
          properties: {
            type: { const: "BackendProvided" },
            name: { type: "string" },
            base: { type: "string" },
          },
          required: ["type"],
        },
      ],
    },
    verifyCommands: {
      type: "array",
      items: { type: "string" },
      description:
        "The ONLY verification commands the agent may run (injected into the prompt). Overrides the profile default.",
    },
    gate: {
      type: "string",
      description:
        "A postcondition command the TOOL runs after the agent. A failing gate downgrades DONE to DONE_WITH_CONCERNS.",
    },
    allowPartialCommit: {
      type: "boolean",
      description:
        "Suppress the incomplete-commit concern (commits landed but tree still dirty).",
    },
    waitMs: {
      type: "number",
      description:
        "How long to block before auto-detaching to a jobId. Clamped to [1000, 600000]. Default ~60s.",
    },
    background: {
      type: "boolean",
      description: "Return {RUNNING, jobId} immediately without blocking.",
    },
  },
  required: ["prompt"],
} as const;

const JOB_ID_SCHEMA = {
  type: "object",
  properties: { jobId: { type: "string" } },
  required: ["jobId"],
} as const;

const JOB_IDS_TIMEOUT_SCHEMA = {
  type: "object",
  properties: {
    jobIds: { type: "array", items: { type: "string" } },
    timeoutMs: { type: "number" },
  },
  required: ["jobIds"],
} as const;

const WAIT_SCHEMA = {
  type: "object",
  properties: {
    jobId: { type: "string" },
    timeoutMs: { type: "number" },
  },
  required: ["jobId"],
} as const;

export const TOOLS = [
  {
    name: "cursor_run",
    description:
      "Delegate a coding/research task to a Cursor model via the local cursor-agent CLI. " +
      "Fast tasks return a full RunOutput (status, text, usage, cost, git change-set, gate result). " +
      "Slow tasks detach and return {status:'RUNNING', jobId} to poll/wait on. " +
      "A write to a locked CallerProvided path returns {status:'BUSY', jobId, busyPath}.",
    inputSchema: RUN_INPUT_SCHEMA,
  },
  {
    name: "cursor_poll",
    description:
      "Non-blocking status check for a jobId. Returns {RUNNING, progress} or {<terminal>, result} or {NOT_FOUND}.",
    inputSchema: JOB_ID_SCHEMA,
  },
  {
    name: "cursor_cancel",
    description:
      "SIGTERM the job's child, mark it CANCELLED, and return the resulting poll result.",
    inputSchema: JOB_ID_SCHEMA,
  },
  {
    name: "cursor_wait",
    description:
      "Long-poll: block until the job is terminal or timeoutMs elapses (default 120000, clamp [1000,600000]). On timeout returns the RUNNING snapshot. Works on a BUSY jobId (waits for the lock holder).",
    inputSchema: WAIT_SCHEMA,
  },
  {
    name: "cursor_wait_any",
    description:
      "Block until the FIRST listed job is terminal (or timeout). Returns {jobs, firstDone?}. Pairs with background:true for parallel dispatch.",
    inputSchema: JOB_IDS_TIMEOUT_SCHEMA,
  },
  {
    name: "cursor_wait_all",
    description:
      "Block until ALL listed (known) jobs are terminal (or timeout). Returns {jobs, allDone}. Empty input -> {jobs:{}, allDone:true}.",
    inputSchema: JOB_IDS_TIMEOUT_SCHEMA,
  },
] as const;
