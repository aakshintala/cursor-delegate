import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import type { CliConfig } from "./safety.js";
import {
  loadConfig,
  loadCliConfig,
  defaultCliConfigPath,
} from "./config.js";
import { makeJobRegistry, type JobRegistry } from "./job-registry.js";
import { makeCursorAdapter } from "./backends/cursor.js";
import { runDelegation, answerDelegation } from "./runner.js";
import { validateRunInput } from "./validate.js";
import {
  progressSinkFrom,
  type McpExtra,
  type ProgressSink,
} from "./progress.js";
import { buildTools } from "./tool-schemas.js";
import { runDoctor } from "./doctor.js";

export interface ServerDeps {
  config: Awaited<ReturnType<typeof loadConfig>>;
  registry: JobRegistry;
  cliConfig: CliConfig | null;
  serverCwd: string;
}

interface CallExtra extends McpExtra {
  signal?: AbortSignal;
}

function jsonContent(value: unknown) {
  return {
    content: [
      { type: "text" as const, text: JSON.stringify(value, null, 2) },
    ],
  };
}

function requireString(
  args: Record<string, unknown> | undefined,
  key: string,
): string {
  const v = args?.[key];
  if (typeof v !== "string" || v.length === 0) {
    throw new Error(`missing required field "${key}"`);
  }
  return v;
}

function asStringArray(v: unknown): string[] {
  if (!Array.isArray(v)) return [];
  return v.filter((x): x is string => typeof x === "string");
}

/** Route a tool call to the right handler. Exported for the index smoke test. */
export async function handleCall(
  name: string,
  args: Record<string, unknown> | undefined,
  deps: ServerDeps,
  extra: CallExtra = {},
): Promise<unknown> {
  const sink: ProgressSink | undefined = progressSinkFrom(extra);
  const signal = extra.signal;

  switch (name) {
    case "cursor_run": {
      return runDelegation(validateRunInput(args ?? {}), deps, { sink, signal });
    }
    case "cursor_poll":
      return deps.registry.poll(requireString(args, "jobId"));
    case "cursor_cancel":
      return deps.registry.cancel(requireString(args, "jobId"));
    case "cursor_wait":
      return deps.registry.wait(
        requireString(args, "jobId"),
        typeof args?.timeoutMs === "number" ? args.timeoutMs : undefined,
        { sink, signal },
      );
    case "cursor_wait_any":
      return deps.registry.waitAny(
        asStringArray(args?.jobIds),
        typeof args?.timeoutMs === "number" ? args.timeoutMs : undefined,
        { sink, signal },
      );
    case "cursor_wait_all":
      return deps.registry.waitAll(
        asStringArray(args?.jobIds),
        typeof args?.timeoutMs === "number" ? args.timeoutMs : undefined,
        { sink, signal },
      );
    case "cursor_answer":
      return answerDelegation(
        requireString(args, "jobId"),
        requireString(args, "answer"),
        deps,
        { sink, signal },
      );
    case "doctor": {
      if (args?.deep !== undefined && typeof args.deep !== "boolean") {
        throw new Error(`invalid field "deep" (expected boolean)`);
      }
      return runDoctor({
        config: deps.config,
        deep: args?.deep === true,
      });
    }
    default:
      throw new Error(`unknown tool "${name}"`);
  }
}

/** Wire an MCP Server around the deps (no transport connected). */
export function createServer(deps: ServerDeps): Server {
  const server = new Server(
    { name: "cursor-delegate", version: "0.3.0" },
    { capabilities: { tools: {} } },
  );

  const tools = buildTools(deps.config);

  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: tools.map((t) => ({
      name: t.name,
      description: t.description,
      inputSchema: t.inputSchema,
    })),
  }));

  server.setRequestHandler(CallToolRequestSchema, async (req, extra) => {
    const value = await handleCall(
      req.params.name,
      req.params.arguments as Record<string, unknown> | undefined,
      deps,
      extra as unknown as CallExtra,
    );
    return jsonContent(value);
  });

  return server;
}

export async function buildDeps(): Promise<ServerDeps> {
  // Locate bundled config relative to this module (dist/.. or src/..).
  const here = dirname(fileURLToPath(import.meta.url));
  const configDir = join(here, "..", "config");

  const config = await loadConfig({
    modelsPath: join(configDir, "models.json"),
  });
  const cliConfig = await loadCliConfig(defaultCliConfigPath());

  const registry = makeJobRegistry({
    backend: makeCursorAdapter(),
    deadlineMs: config.profile.deadlineMs ?? 60000,
    idleMs:
      config.profile.idleMs === undefined ? 300000 : config.profile.idleMs,
    toolIdleMs:
      config.profile.toolIdleMs === undefined
        ? 1800000
        : config.profile.toolIdleMs,
  });

  return { config, registry, cliConfig, serverCwd: process.cwd() };
}

export async function main(): Promise<void> {
  const deps = await buildDeps();
  const server = createServer(deps);

  const shutdown = () => {
    deps.registry.killAll();
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);
  process.on("exit", shutdown);

  const transport = new StdioServerTransport();
  await server.connect(transport);
}

// Guard so importing this module in tests does not start the server.
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}
