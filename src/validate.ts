import type { Capability, Isolation, RunInput } from "./types.js";

const CAPABILITIES: Capability[] = [
  "ask",
  "plan",
  "write",
  "write-unsandboxed",
];

function invalid(field: string, detail?: string): never {
  throw new Error(
    detail ? `invalid ${field}: ${detail}` : `invalid ${field}`,
  );
}

function parseIsolation(v: unknown): Isolation {
  if (!v || typeof v !== "object") invalid("isolation");
  const o = v as Record<string, unknown>;
  switch (o.type) {
    case "None":
      return { type: "None" };
    case "CallerProvided": {
      if (typeof o.path !== "string" || o.path.length === 0) {
        invalid("isolation.path", "required non-empty string");
      }
      return { type: "CallerProvided", path: o.path };
    }
    case "BackendProvided": {
      const iso: Extract<Isolation, { type: "BackendProvided" }> = {
        type: "BackendProvided",
      };
      if (o.name !== undefined) {
        if (typeof o.name !== "string") invalid("isolation.name");
        iso.name = o.name;
      }
      if (o.base !== undefined) {
        if (typeof o.base !== "string") invalid("isolation.base");
        iso.base = o.base;
      }
      return iso;
    }
    default:
      invalid("isolation.type");
  }
}

/** Validate MCP cursor_run args into a typed RunInput. */
export function validateRunInput(args: Record<string, unknown>): RunInput {
  const prompt = args.prompt;
  if (typeof prompt !== "string" || prompt.length === 0) {
    throw new Error('missing required field "prompt"');
  }

  const input: RunInput = { prompt };

  if (args.model !== undefined) {
    if (typeof args.model !== "string" || args.model.length === 0) {
      invalid("model");
    }
    input.model = args.model;
  }

  if (args.requireNonClaude !== undefined) {
    if (typeof args.requireNonClaude !== "boolean") {
      invalid("requireNonClaude", "must be a boolean");
    }
    input.requireNonClaude = args.requireNonClaude;
  }

  if (args.capability !== undefined) {
    if (
      typeof args.capability !== "string" ||
      !CAPABILITIES.includes(args.capability as Capability)
    ) {
      invalid("capability");
    }
    input.capability = args.capability as Capability;
  }

  if (args.allowUnsandboxed !== undefined) {
    if (typeof args.allowUnsandboxed !== "boolean") {
      invalid("allowUnsandboxed", "must be a boolean");
    }
    input.allowUnsandboxed = args.allowUnsandboxed;
  }

  if (args.session !== undefined) {
    if (typeof args.session !== "string" || args.session.length === 0) {
      invalid("session");
    }
    input.session = args.session;
  }

  if (args.isolation !== undefined) {
    input.isolation = parseIsolation(args.isolation);
  }

  if (args.verifyCommands !== undefined) {
    if (!Array.isArray(args.verifyCommands)) invalid("verifyCommands");
    const cmds = args.verifyCommands.filter(
      (x): x is string => typeof x === "string",
    );
    if (cmds.length !== args.verifyCommands.length) {
      invalid("verifyCommands", "must be an array of strings");
    }
    input.verifyCommands = cmds;
  }

  if (args.gate !== undefined) {
    if (typeof args.gate !== "string") invalid("gate");
    input.gate = args.gate;
  }

  if (args.allowPartialCommit !== undefined) {
    if (typeof args.allowPartialCommit !== "boolean") {
      invalid("allowPartialCommit", "must be a boolean");
    }
    input.allowPartialCommit = args.allowPartialCommit;
  }

  if (args.waitMs !== undefined) {
    if (typeof args.waitMs !== "number" || !Number.isFinite(args.waitMs)) {
      invalid("waitMs", "must be a finite number");
    }
    input.waitMs = args.waitMs;
  }

  if (args.idleMs !== undefined) {
    if (
      args.idleMs !== null &&
      (typeof args.idleMs !== "number" || !Number.isFinite(args.idleMs))
    ) {
      invalid("idleMs", "must be a finite number or null");
    }
    input.idleMs = args.idleMs as number | null;
  }

  if (args.toolIdleMs !== undefined) {
    if (
      args.toolIdleMs !== null &&
      (typeof args.toolIdleMs !== "number" || !Number.isFinite(args.toolIdleMs))
    ) {
      invalid("toolIdleMs", "must be a finite number or null");
    }
    input.toolIdleMs = args.toolIdleMs as number | null;
  }

  if (args.background !== undefined) {
    if (typeof args.background !== "boolean") {
      invalid("background", "must be a boolean");
    }
    input.background = args.background;
  }

  return input;
}
