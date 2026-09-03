import type { RawCursorJson } from "./types.js";

export interface StreamState {
  lastTool: string | null;
  tokensSoFar: number;
  lastAssistant: string | null;
  filesTouched: string[];
  phase: string | null;
}

export function initStreamState(): StreamState {
  return {
    lastTool: null,
    tokensSoFar: 0,
    lastAssistant: null,
    filesTouched: [],
    phase: null,
  };
}

export interface ParsedLine {
  /** Set only for the terminal `result` event. */
  result?: RawCursorJson;
  /** Whether live state changed (a progress emit is worthwhile). */
  changed: boolean;
}

interface ToolCallShape {
  [key: string]: unknown;
  mcpToolCall?: { toolName?: string };
}

const PATH_ARG_KEYS = ["path", "filePath", "file_path", "target", "destination"];

function pathFromArgs(args: Record<string, unknown> | undefined): string | null {
  if (!args) return null;
  for (const key of PATH_ARG_KEYS) {
    const v = args[key];
    if (typeof v === "string" && v.length > 0) return v;
  }
  return null;
}

function extractTool(tc: ToolCallShape): {
  tool: string | null;
  path: string | null;
} {
  if (tc.mcpToolCall) {
    return { tool: tc.mcpToolCall.toolName ?? "mcp", path: null };
  }
  for (const key of Object.keys(tc)) {
    if (key.endsWith("ToolCall")) {
      const name = key.slice(0, -"ToolCall".length); // "shell", "edit", ...
      const args = (tc[key] as { args?: Record<string, unknown> } | undefined)
        ?.args;
      return { tool: name, path: pathFromArgs(args) };
    }
  }
  return { tool: null, path: null };
}

function extractAssistantText(message: unknown): string | null {
  const content = (message as { content?: unknown })?.content;
  if (!Array.isArray(content)) return null;
  const parts = content
    .filter(
      (b): b is { type: string; text: string } =>
        !!b && (b as { type?: string }).type === "text" &&
        typeof (b as { text?: unknown }).text === "string",
    )
    .map((b) => b.text);
  if (parts.length === 0) return null;
  return parts.join("");
}

/**
 * Parse one stream-json (NDJSON) line, mutating `state`.
 * Ignores blank / non-JSON lines and events with no `type`.
 */
export function parseLine(line: string, state: StreamState): ParsedLine {
  const trimmed = line.trim();
  if (!trimmed) return { changed: false };

  let ev: Record<string, unknown>;
  try {
    ev = JSON.parse(trimmed) as Record<string, unknown>;
  } catch {
    return { changed: false };
  }
  if (!ev || typeof ev.type !== "string") return { changed: false };

  let changed = false;

  // Any event carrying a numeric usage.outputTokens updates the live token count.
  const usage = ev.usage as { outputTokens?: unknown } | undefined;
  if (usage && typeof usage.outputTokens === "number") {
    state.tokensSoFar = Math.max(state.tokensSoFar, usage.outputTokens);
    changed = true;
  }

  switch (ev.type) {
    case "tool_call": {
      if (ev.subtype === "started" && ev.tool_call) {
        // A tool is now in flight — the caller's idle watchdog uses this to switch to the
        // wider toolIdleMs window, since a running build/test can go silent for a long time
        // without that being a hang.
        state.phase = "running_tool";
        changed = true;
        const { tool, path } = extractTool(ev.tool_call as ToolCallShape);
        if (tool) {
          state.lastTool = tool;
        }
        if (path && !state.filesTouched.includes(path)) {
          state.filesTouched.push(path);
        }
      }
      break;
    }
    case "assistant": {
      const text = extractAssistantText(ev.message);
      if (text) {
        state.lastAssistant = text.slice(0, 200);
        state.phase = "responding";
        changed = true;
      }
      break;
    }
    case "thinking": {
      if (ev.subtype === "delta" && typeof ev.text === "string") {
        state.phase = "thinking";
        changed = true;
      }
      break;
    }
    case "result":
      return { result: ev as RawCursorJson, changed: true };
  }

  return { changed };
}
