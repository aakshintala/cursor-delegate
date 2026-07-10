# Model Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the tier/price-map model layer with a curated `config/models.json` allow-list, a hard-reject `requireNonClaude` resolver, and a startup-generated tool schema enum.

**Architecture:** A single `models.json` is the source of truth for ids, labels, families, and `$/MTok` prices. `loadConfig` merges host-profile `default`/`models` overrides into that map and derives a `priceMap` for existing cost computation. `resolveModel` in `src/models.ts` (replacing `src/tiers.ts`) applies default → allow-list lookup → optional non-Claude hard reject. `tool-schemas.ts` becomes a builder so the MCP `model` enum and recommended-models blurb always match the loaded config.

**Tech Stack:** TypeScript (NodeNext ESM), Node.js built-in test runner (`node --import tsx --test`), `@modelcontextprotocol/sdk`, JSON config under `config/`.

## Global Constraints

- Hard cut: no `tier` param, no `tier-map.json`, no `tierOverrides`, no back-compat shim (the plugin ships as a whole).
- Curated allow-list only: a model is callable iff it is in the map — no raw-passthrough escape hatch.
- Prices are `$/MTok`, maintained by hand (the CLI exposes no pricing).
- Default model when `model` is omitted is `composer-2.5`.
- `requireNonClaude` hard-rejects on `family === "claude"` (no silent swap).
- Seed `models.json` has no Claude entry; tests that need Claude inject a fixture entry.
- Pricing computation stays `computeCost(usage, priceMap, model)`; the map is now derived from `models.json`.
- Out of scope for this plan: needs-input / `NEEDS_CONTEXT` / `cursor_answer`, `doctor`, `delegate` skill & catalog rewrite.

## File structure (target)

| Path | Role |
|------|------|
| `config/models.json` | Single source of truth (replaces `tier-map.json` + `price-map.json`) |
| `src/types.ts` | `ModelEntry`, new `Config`/`HostProfile`/`RunInput`/`ResolvedModel`; delete `Tier`/`TierMap` |
| `src/config.ts` | Load + merge `models.json` + host profile; derive `priceMap` |
| `src/models.ts` | `resolveModel`, `ModelNotAllowedError`, `NonClaudeViolationError` (replaces `src/tiers.ts`) |
| `src/validate.ts` | Validate `model` + `requireNonClaude`; drop `TIERS` / `tier` |
| `src/tool-schemas.ts` | `buildRunInputSchema` / `buildTools` / `buildRecommendedModelsBlurb` |
| `src/pricing.ts` | Keep `computeCost`; drop bare `gpt-5.5` alias |
| `src/runner.ts` | Call new `resolveModel`; hardcode `backend: "cursor"` |
| `src/index.ts` | Load `models.json`; register tools from `buildTools(config)` |
| `tests/models.test.ts` | Resolver tests (replaces `tests/tiers.test.ts`) |
| `tests/validate.test.ts` | New focused validate tests |
| `tests/tool-schemas.test.ts` | Schema enum + blurb tests |
| Deleted | `config/tier-map.json`, `config/price-map.json`, `src/tiers.ts`, `tests/tiers.test.ts` |

---

### Task 1: Seed `config/models.json` and rewrite domain types

**Files:**
- Create: `config/models.json`
- Modify: `src/types.ts` (full file — replace tier types; reshape `Config` / `HostProfile` / `RunInput` / `ResolvedModel`)
- Test: none yet (types + static JSON; exercised starting in Task 2)

**Interfaces:**
- Consumes: nothing new
- Produces:
  - `ModelEntry`: `{ label: string; family: string; price: Price }`
  - `ResolvedModel`: `{ model: string; family: string; price: Price }`
  - `Config`: `{ default: string; models: Record<string, ModelEntry>; priceMap: PriceMap; profile: HostProfile }`
  - `HostProfile`: drops `tierOverrides` / `priceOverrides`; adds optional `default?: string` and `models?: Record<string, ModelEntry>`
  - `RunInput`: drops `tier?: Tier`; adds `requireNonClaude?: boolean`
  - Deletes: `Tier`, `TierMap`

- [ ] **Step 1: Create `config/models.json` with the approved seed**

```json
{
  "default": "composer-2.5",
  "models": {
    "composer-2.5":     { "label": "Composer 2.5",     "family": "composer", "price": { "input": 0.5, "output": 2.5, "cacheRead": 0.2,  "cacheWrite": 0 } },
    "grok-4.5-xhigh":   { "label": "Grok 4.5",          "family": "grok",     "price": { "input": 2,   "output": 6,  "cacheRead": 0.5,  "cacheWrite": 0 } },
    "gemini-3.5-flash": { "label": "Gemini 3.5 Flash",  "family": "gemini",   "price": { "input": 1.5, "output": 9,  "cacheRead": 0.15, "cacheWrite": 0 } },
    "gpt-5.5-high":     { "label": "GPT-5.5 1M High",   "family": "gpt",      "price": { "input": 5,   "output": 30, "cacheRead": 0.5,  "cacheWrite": 0 } }
  }
}
```

- [ ] **Step 2: Rewrite the model-related types in `src/types.ts`**

Replace the tier / config / resolution types. Keep `Price`, `PriceMap`, `JobSpec.priceMap`, and everything else unchanged.

Delete:

```typescript
export type Tier = "cheap-bulk" | "standard" | "coding-specialist" | "diversity";
```

Change `RunInput` to:

```typescript
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
```

Replace `ResolvedModel` / `TierMap` / `HostProfile` / `Config` with:

```typescript
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
```

Keep the existing `Price` / `PriceMap` definitions above these (unchanged):

```typescript
export interface Price {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
}
export type PriceMap = Record<string, Price>;
```

- [ ] **Step 3: Commit**

```bash
git add config/models.json src/types.ts
git commit -m "$(cat <<'EOF'
feat(models): add models.json seed and replace tier domain types

EOF
)"
```

---

### Task 2: Load and merge `models.json` in `config.ts`

**Files:**
- Modify: `src/config.ts` (full rewrite of `LoadConfigOpts` + `loadConfig`)
- Modify: `tests/config.test.ts` (full rewrite)
- Test: `tests/config.test.ts`

**Interfaces:**
- Consumes: `ModelEntry`, `Config`, `HostProfile`, `PriceMap` from Task 1
- Produces:
  - `LoadConfigOpts`: `{ modelsPath: string; hostProfilePath?: string; readFile?: ReadFileFn }`
  - `loadConfig(opts: LoadConfigOpts): Promise<Config>`
  - Internal merge: `default = profile.default ?? file.default`; `models = { ...file.models, ...profile.models }`; `priceMap` derived from merged `models`
  - Throws if the merged `default` is missing from `models`

- [ ] **Step 1: Write the failing config tests**

Replace `tests/config.test.ts` entirely with:

```typescript
import { test } from "node:test";
import assert from "node:assert/strict";
import { loadConfig } from "../src/config.js";

const modelsDefault = JSON.stringify({
  default: "composer-2.5",
  models: {
    "composer-2.5": {
      label: "Composer 2.5",
      family: "composer",
      price: { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    },
    "grok-4.5-xhigh": {
      label: "Grok 4.5",
      family: "grok",
      price: { input: 2, output: 6, cacheRead: 0.5, cacheWrite: 0 },
    },
  },
});

function reader(files: Record<string, string>) {
  return async (path: string): Promise<string> => {
    if (path in files) return files[path];
    const err = new Error("ENOENT") as NodeJS.ErrnoException;
    err.code = "ENOENT";
    throw err;
  };
}

test("loads models.json default and derives priceMap", async () => {
  const cfg = await loadConfig({
    modelsPath: "/models.json",
    hostProfilePath: "/does-not-exist.json",
    readFile: reader({ "/models.json": modelsDefault }),
  });
  assert.equal(cfg.default, "composer-2.5");
  assert.equal(cfg.models["composer-2.5"].family, "composer");
  assert.deepEqual(cfg.priceMap["composer-2.5"], {
    input: 0.5,
    output: 2.5,
    cacheRead: 0.2,
    cacheWrite: 0,
  });
  assert.deepEqual(cfg.profile, {});
});

test("merges host-profile default and models over the bundled map", async () => {
  const cfg = await loadConfig({
    modelsPath: "/models.json",
    hostProfilePath: "/profile.json",
    readFile: reader({
      "/models.json": modelsDefault,
      "/profile.json": JSON.stringify({
        default: "grok-4.5-xhigh",
        models: {
          "gpt-5.5-high": {
            label: "GPT-5.5 1M High",
            family: "gpt",
            price: { input: 5, output: 30, cacheRead: 0.5, cacheWrite: 0 },
          },
        },
        requiredDeny: ["rm -rf /"],
        deadlineMs: 5000,
      }),
    }),
  });
  assert.equal(cfg.default, "grok-4.5-xhigh");
  assert.ok("composer-2.5" in cfg.models);
  assert.ok("gpt-5.5-high" in cfg.models);
  assert.equal(cfg.models["gpt-5.5-high"].family, "gpt");
  assert.deepEqual(cfg.priceMap["gpt-5.5-high"], {
    input: 5,
    output: 30,
    cacheRead: 0.5,
    cacheWrite: 0,
  });
  assert.deepEqual(cfg.profile.requiredDeny, ["rm -rf /"]);
  assert.equal(cfg.profile.deadlineMs, 5000);
});

test("a missing host profile (ENOENT) yields an empty profile, not an error", async () => {
  const cfg = await loadConfig({
    modelsPath: "/models.json",
    hostProfilePath: "/does-not-exist.json",
    readFile: reader({ "/models.json": modelsDefault }),
  });
  assert.deepEqual(cfg.profile, {});
  assert.equal(cfg.default, "composer-2.5");
  assert.ok("composer-2.5" in cfg.priceMap);
});

test("throws when merged default is absent from models", async () => {
  await assert.rejects(
    () =>
      loadConfig({
        modelsPath: "/models.json",
        hostProfilePath: "/profile.json",
        readFile: reader({
          "/models.json": modelsDefault,
          "/profile.json": JSON.stringify({ default: "not-a-real-model" }),
        }),
      }),
    /default/,
  );
});
```

- [ ] **Step 2: Run the config tests — expect FAIL**

Run: `node --import tsx --test tests/config.test.ts`

Expected: FAIL (e.g. `LoadConfigOpts` still requires `tierMapPath` / `priceMapPath`, or `cfg.default` / `cfg.models` are undefined).

- [ ] **Step 3: Rewrite `src/config.ts`**

Replace the file with:

```typescript
import { readFile as fsReadFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import type {
  Config,
  HostProfile,
  ModelEntry,
  PriceMap,
} from "./types.js";
import type { CliConfig } from "./safety.js";

export type ReadFileFn = (path: string) => Promise<string>;

function defaultReadFile(path: string): Promise<string> {
  return fsReadFile(path, "utf8");
}

async function readJson<T>(
  readFile: ReadFileFn,
  path: string,
): Promise<T | null> {
  try {
    return JSON.parse(await readFile(path)) as T;
  } catch (e: unknown) {
    if ((e as NodeJS.ErrnoException)?.code === "ENOENT") return null;
    throw e;
  }
}

export function defaultHostProfilePath(): string {
  return join(homedir(), ".config", "cursor-delegate", "host-profile.json");
}

export function defaultCliConfigPath(): string {
  return join(homedir(), ".cursor", "cli-config.json");
}

export interface LoadConfigOpts {
  modelsPath: string;
  hostProfilePath?: string;
  readFile?: ReadFileFn;
}

interface ModelsFile {
  default: string;
  models: Record<string, ModelEntry>;
}

function toPriceMap(models: Record<string, ModelEntry>): PriceMap {
  const out: PriceMap = {};
  for (const [id, entry] of Object.entries(models)) {
    out[id] = entry.price;
  }
  return out;
}

/**
 * Read bundled models.json + host profile and merge:
 *   default = profile.default ?? file.default
 *   models  = { ...file.models, ...profile.models }
 * A missing host profile (ENOENT) is treated as empty, not an error.
 */
export async function loadConfig(opts: LoadConfigOpts): Promise<Config> {
  const readFile = opts.readFile ?? defaultReadFile;
  const profilePath =
    opts.hostProfilePath ??
    process.env.CURSOR_DELEGATE_HOST_PROFILE ??
    defaultHostProfilePath();

  const [fileRaw, profileRaw] = await Promise.all([
    readJson<ModelsFile>(readFile, opts.modelsPath),
    readJson<HostProfile>(readFile, profilePath),
  ]);

  if (!fileRaw || !fileRaw.models || typeof fileRaw.default !== "string") {
    throw new Error(`invalid or missing models file: ${opts.modelsPath}`);
  }

  const profile: HostProfile = profileRaw ?? {};
  const models: Record<string, ModelEntry> = {
    ...fileRaw.models,
    ...(profile.models ?? {}),
  };
  const defaultModel = profile.default ?? fileRaw.default;

  if (!(defaultModel in models)) {
    throw new Error(
      `default model "${defaultModel}" is not present in the models map`,
    );
  }

  return {
    default: defaultModel,
    models,
    priceMap: toPriceMap(models),
    profile,
  };
}

/** Read the cursor-agent cli-config (for the deny-list). Missing file -> null. */
export async function loadCliConfig(
  path: string,
  readFile: ReadFileFn = defaultReadFile,
): Promise<CliConfig | null> {
  return readJson<CliConfig>(readFile, path);
}
```

- [ ] **Step 4: Run the config tests — expect PASS**

Run: `node --import tsx --test tests/config.test.ts`

Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/config.ts tests/config.test.ts
git commit -m "$(cat <<'EOF'
feat(config): load and merge models.json as the model allow-list

EOF
)"
```

---

### Task 3: Resolver in `src/models.ts` (replaces `src/tiers.ts`)

**Files:**
- Create: `src/models.ts`
- Create: `tests/models.test.ts`
- Delete (at end of this task, after new tests pass): `src/tiers.ts`, `tests/tiers.test.ts`
- Test: `tests/models.test.ts`

**Interfaces:**
- Consumes: `Config`, `ResolvedModel`, `ModelEntry` from Task 1
- Produces:
  - `class ModelNotAllowedError extends Error` (`name = "ModelNotAllowedError"`)
  - `class NonClaudeViolationError extends Error` (`name = "NonClaudeViolationError"`)
  - `resolveModel(input: { model?: string; requireNonClaude?: boolean }, config: Pick<Config, "default" | "models">): ResolvedModel`
  - Algorithm: `model = input.model ?? config.default` → lookup → if missing throw `ModelNotAllowedError` → if `requireNonClaude && family === "claude"` throw `NonClaudeViolationError` → return `{ model, family, price }`

- [ ] **Step 1: Write the failing resolver tests**

Create `tests/models.test.ts`:

```typescript
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  resolveModel,
  ModelNotAllowedError,
  NonClaudeViolationError,
} from "../src/models.js";
import type { Config } from "../src/types.js";

const base: Pick<Config, "default" | "models"> = {
  default: "composer-2.5",
  models: {
    "composer-2.5": {
      label: "Composer 2.5",
      family: "composer",
      price: { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    },
    "grok-4.5-xhigh": {
      label: "Grok 4.5",
      family: "grok",
      price: { input: 2, output: 6, cacheRead: 0.5, cacheWrite: 0 },
    },
    "claude-sonnet-4": {
      label: "Claude Sonnet 4",
      family: "claude",
      price: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
    },
  },
};

test("omitted model resolves to default", () => {
  const r = resolveModel({}, base);
  assert.equal(r.model, "composer-2.5");
  assert.equal(r.family, "composer");
  assert.deepEqual(r.price, base.models["composer-2.5"].price);
});

test("allowed id resolves with family and price", () => {
  const r = resolveModel({ model: "grok-4.5-xhigh" }, base);
  assert.equal(r.model, "grok-4.5-xhigh");
  assert.equal(r.family, "grok");
  assert.deepEqual(r.price, base.models["grok-4.5-xhigh"].price);
});

test("unknown id throws ModelNotAllowedError", () => {
  assert.throws(
    () => resolveModel({ model: "not-listed" }, base),
    ModelNotAllowedError,
  );
});

test("requireNonClaude rejects an explicit Claude model", () => {
  assert.throws(
    () =>
      resolveModel(
        { model: "claude-sonnet-4", requireNonClaude: true },
        base,
      ),
    NonClaudeViolationError,
  );
});

test("requireNonClaude rejects a Claude default", () => {
  const claudeDefault: Pick<Config, "default" | "models"> = {
    default: "claude-sonnet-4",
    models: base.models,
  };
  assert.throws(
    () => resolveModel({ requireNonClaude: true }, claudeDefault),
    NonClaudeViolationError,
  );
});

test("requireNonClaude passes a non-Claude model", () => {
  const r = resolveModel(
    { model: "grok-4.5-xhigh", requireNonClaude: true },
    base,
  );
  assert.equal(r.model, "grok-4.5-xhigh");
  assert.equal(r.family, "grok");
});

test("requireNonClaude false allows a Claude model", () => {
  const r = resolveModel(
    { model: "claude-sonnet-4", requireNonClaude: false },
    base,
  );
  assert.equal(r.model, "claude-sonnet-4");
  assert.equal(r.family, "claude");
});
```

- [ ] **Step 2: Run the models tests — expect FAIL**

Run: `node --import tsx --test tests/models.test.ts`

Expected: FAIL with module not found (`Cannot find module '../src/models.js'` or equivalent).

- [ ] **Step 3: Implement `src/models.ts`**

```typescript
import type { Config, ResolvedModel } from "./types.js";

export class ModelNotAllowedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ModelNotAllowedError";
  }
}

export class NonClaudeViolationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NonClaudeViolationError";
  }
}

/**
 * Resolve a callable model from the curated allow-list.
 * Order: explicit model -> config.default; then allow-list lookup;
 * then optional requireNonClaude hard reject when family === "claude".
 */
export function resolveModel(
  input: { model?: string; requireNonClaude?: boolean },
  config: Pick<Config, "default" | "models">,
): ResolvedModel {
  const model = input.model ?? config.default;
  const entry = config.models[model];
  if (!entry) {
    throw new ModelNotAllowedError(
      `model "${model}" is not in the allow-list`,
    );
  }
  if (input.requireNonClaude && entry.family === "claude") {
    throw new NonClaudeViolationError(
      `requireNonClaude is set but model "${model}" has family "claude"`,
    );
  }
  return { model, family: entry.family, price: entry.price };
}
```

- [ ] **Step 4: Run the models tests — expect PASS**

Run: `node --import tsx --test tests/models.test.ts`

Expected: PASS (7 tests).

- [ ] **Step 5: Delete the old tier resolver and its tests**

```bash
rm src/tiers.ts tests/tiers.test.ts
```

- [ ] **Step 6: Commit**

```bash
git add src/models.ts tests/models.test.ts
git rm -f src/tiers.ts tests/tiers.test.ts
git commit -m "$(cat <<'EOF'
feat(models): replace tier resolver with allow-list resolveModel

EOF
)"
```

---

### Task 4: Pricing against the models map (drop bare-id alias)

**Files:**
- Modify: `src/pricing.ts:1-27` (remove `gpt-5.5` → `gpt-5.5-medium` alias)
- Modify: `tests/pricing.test.ts` (full rewrite of fixtures + drop alias test; add seed-map coverage)
- Test: `tests/pricing.test.ts`

**Interfaces:**
- Consumes: `PriceMap`, `Usage` (unchanged)
- Produces: `computeCost(usage, priceMap, model): number | null` — exact lookup only; no alias fallback

- [ ] **Step 1: Write the failing/updated pricing tests**

Replace `tests/pricing.test.ts` with:

```typescript
import { test } from "node:test";
import assert from "node:assert/strict";
import { computeCost } from "../src/pricing.js";
import type { PriceMap, Usage } from "../src/types.js";

/** Mirrors the seed prices from config/models.json. */
const priceMap: PriceMap = {
  "composer-2.5": { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
  "grok-4.5-xhigh": { input: 2, output: 6, cacheRead: 0.5, cacheWrite: 0 },
  "gemini-3.5-flash": { input: 1.5, output: 9, cacheRead: 0.15, cacheWrite: 0 },
  "gpt-5.5-high": { input: 5, output: 30, cacheRead: 0.5, cacheWrite: 0 },
};

const usage: Usage = {
  inputTokens: 1_000_000,
  outputTokens: 1_000_000,
  cacheReadTokens: 0,
  cacheWriteTokens: 0,
};

test("null usage -> null", () => {
  assert.equal(computeCost(null, priceMap, "composer-2.5"), null);
});

test("missing price entry -> null", () => {
  assert.equal(computeCost(usage, priceMap, "unknown-model"), null);
});

test("sums tokens x price / 1e6 for composer-2.5", () => {
  // 1M input * 0.5 + 1M output * 2.5 = 3.0
  assert.equal(computeCost(usage, priceMap, "composer-2.5"), 3.0);
});

test("sums tokens x price / 1e6 for grok-4.5-xhigh", () => {
  // 1M * 2 + 1M * 6 = 8
  assert.equal(computeCost(usage, priceMap, "grok-4.5-xhigh"), 8);
});

test("sums tokens x price / 1e6 for gpt-5.5-high", () => {
  // 1M * 5 + 1M * 30 = 35
  assert.equal(computeCost(usage, priceMap, "gpt-5.5-high"), 35);
});

test("bare gpt-5.5 is not aliased (allow-list only)", () => {
  assert.equal(computeCost(usage, priceMap, "gpt-5.5"), null);
});

test("missing usage fields are treated as 0", () => {
  const partial = { outputTokens: 1_000_000 } as unknown as Usage;
  assert.equal(computeCost(partial, priceMap, "composer-2.5"), 2.5);
});
```

- [ ] **Step 2: Run pricing tests — expect FAIL on the alias test**

Run: `node --import tsx --test tests/pricing.test.ts`

Expected: FAIL on `bare gpt-5.5 is not aliased` (old code still aliases to a missing `gpt-5.5-medium` or returns a number if that key existed; with the new map it may already return `null` via missing `gpt-5.5-medium` — if that test already PASSes, the FAIL you need is any remaining reference to `gpt-5.5-medium` in `src/pricing.ts`. Proceed to Step 3 to delete the alias either way).

- [ ] **Step 3: Simplify `src/pricing.ts`**

Replace the file with:

```typescript
import type { PriceMap, Usage } from "./types.js";

const n = (x: number | undefined): number => (typeof x === "number" ? x : 0);

/**
 * Best-effort USD cost. Always estimated (the CLI emits no cost field).
 * Returns null when usage or a price entry is missing.
 * Prices come from the curated models map (no bare-id aliases).
 */
export function computeCost(
  usage: Usage | null | undefined,
  priceMap: PriceMap,
  model: string,
): number | null {
  if (!usage) return null;
  const price = priceMap[model];
  if (!price) return null;
  return (
    (n(usage.inputTokens) * price.input +
      n(usage.outputTokens) * price.output +
      n(usage.cacheReadTokens) * price.cacheRead +
      n(usage.cacheWriteTokens) * price.cacheWrite) /
    1e6
  );
}
```

- [ ] **Step 4: Run pricing tests — expect PASS**

Run: `node --import tsx --test tests/pricing.test.ts`

Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add src/pricing.ts tests/pricing.test.ts
git commit -m "$(cat <<'EOF'
feat(pricing): source costs from models map without bare-id aliases

EOF
)"
```

---

### Task 5: Validate `model` + `requireNonClaude`; drop `tier`

**Files:**
- Modify: `src/validate.ts:1-143` (drop `TIERS` / `tier` block; add `requireNonClaude`)
- Create: `tests/validate.test.ts`
- Test: `tests/validate.test.ts`

**Interfaces:**
- Consumes: `RunInput` from Task 1 (no `tier`; optional `requireNonClaude?: boolean`)
- Produces: `validateRunInput(args: Record<string, unknown>): RunInput`
  - `model`: optional non-empty string
  - `requireNonClaude`: optional boolean
  - Unknown fields (including `tier`) are ignored — same as today's validator

- [ ] **Step 1: Write the failing validate tests**

Create `tests/validate.test.ts`:

```typescript
import { test } from "node:test";
import assert from "node:assert/strict";
import { validateRunInput } from "../src/validate.js";

test("accepts model and requireNonClaude", () => {
  const input = validateRunInput({
    prompt: "do it",
    model: "grok-4.5-xhigh",
    requireNonClaude: true,
  });
  assert.equal(input.prompt, "do it");
  assert.equal(input.model, "grok-4.5-xhigh");
  assert.equal(input.requireNonClaude, true);
});

test("rejects empty model", () => {
  assert.throws(
    () => validateRunInput({ prompt: "x", model: "" }),
    /model/,
  );
});

test("rejects non-boolean requireNonClaude", () => {
  assert.throws(
    () => validateRunInput({ prompt: "x", requireNonClaude: "yes" }),
    /requireNonClaude/,
  );
});

test("tier is no longer recognized (silently ignored)", () => {
  const input = validateRunInput({
    prompt: "x",
    tier: "cheap-bulk",
  });
  assert.equal(input.prompt, "x");
  assert.equal("tier" in input, false);
  assert.equal(input.model, undefined);
});

test("omitted requireNonClaude stays undefined (default false at resolve)", () => {
  const input = validateRunInput({ prompt: "x" });
  assert.equal(input.requireNonClaude, undefined);
});
```

- [ ] **Step 2: Run validate tests — expect FAIL**

Run: `node --import tsx --test tests/validate.test.ts`

Expected: FAIL on `requireNonClaude` acceptance and/or `tier is no longer recognized` (old code still copies `tier` onto the input when it matches the old allow-list).

- [ ] **Step 3: Update `src/validate.ts`**

1. Change the import to drop `Tier`:

```typescript
import type { Capability, Isolation, RunInput } from "./types.js";
```

2. Delete the `TIERS` constant entirely.

3. Delete the entire `args.tier` block (the `if (args.tier !== undefined) { ... }` section).

4. After the existing `args.model` block, add:

```typescript
  if (args.requireNonClaude !== undefined) {
    if (typeof args.requireNonClaude !== "boolean") {
      invalid("requireNonClaude", "must be a boolean");
    }
    input.requireNonClaude = args.requireNonClaude;
  }
```

Leave all other field validation unchanged.

- [ ] **Step 4: Run validate tests — expect PASS**

Run: `node --import tsx --test tests/validate.test.ts`

Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/validate.ts tests/validate.test.ts
git commit -m "$(cat <<'EOF'
feat(validate): accept requireNonClaude and drop tier validation

EOF
)"
```

---

### Task 6: Dynamic tool-schema builder (enum + blurb)

**Files:**
- Modify: `src/tool-schemas.ts` (replace static `RUN_INPUT_SCHEMA` / `TOOLS` with builders)
- Create: `tests/tool-schemas.test.ts`
- Test: `tests/tool-schemas.test.ts`

**Interfaces:**
- Consumes: `Config` (`default`, `models`) from Task 1
- Produces:
  - `buildRecommendedModelsBlurb(models: Record<string, ModelEntry>): string` — lines of `id — label — $in/$out` joined by `"; "`
  - `buildRunInputSchema(config: Pick<Config, "default" | "models">): object` — `model.enum` = `Object.keys(config.models)` (stable sort); no `tier`; includes `requireNonClaude` boolean
  - `buildTools(config: Pick<Config, "default" | "models">): Array<{ name; description; inputSchema }>` — `cursor_run` description includes the blurb and default id

- [ ] **Step 1: Write the failing tool-schema tests**

Create `tests/tool-schemas.test.ts`:

```typescript
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  buildRecommendedModelsBlurb,
  buildRunInputSchema,
  buildTools,
} from "../src/tool-schemas.js";
import type { ModelEntry } from "../src/types.js";

const models: Record<string, ModelEntry> = {
  "composer-2.5": {
    label: "Composer 2.5",
    family: "composer",
    price: { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
  },
  "grok-4.5-xhigh": {
    label: "Grok 4.5",
    family: "grok",
    price: { input: 2, output: 6, cacheRead: 0.5, cacheWrite: 0 },
  },
};

const config = { default: "composer-2.5", models };

test("blurb lists id, label, and in/out prices", () => {
  const blurb = buildRecommendedModelsBlurb(models);
  assert.match(blurb, /composer-2\.5 — Composer 2\.5 — \$0\.5\/\$2\.5/);
  assert.match(blurb, /grok-4\.5-xhigh — Grok 4\.5 — \$2\/\$6/);
});

test("run input schema enum is the allow-list ids", () => {
  const schema = buildRunInputSchema(config) as {
    properties: {
      model: { enum: string[] };
      tier?: unknown;
      requireNonClaude: { type: string };
    };
  };
  assert.deepEqual(
    [...schema.properties.model.enum].sort(),
    ["composer-2.5", "grok-4.5-xhigh"],
  );
  assert.equal(schema.properties.tier, undefined);
  assert.equal(schema.properties.requireNonClaude.type, "boolean");
});

test("buildTools wires cursor_run description with blurb and default", () => {
  const tools = buildTools(config);
  assert.equal(tools.length, 6);
  const run = tools.find((t) => t.name === "cursor_run");
  assert.ok(run);
  assert.match(run!.description, /composer-2\.5 — Composer 2\.5/);
  assert.match(run!.description, /Default model: composer-2\.5/);
  assert.equal(
    (run!.inputSchema as { properties: { model: { enum: string[] } } })
      .properties.model.enum.includes("grok-4.5-xhigh"),
    true,
  );
  assert.deepEqual(
    tools.map((t) => t.name),
    [
      "cursor_run",
      "cursor_poll",
      "cursor_cancel",
      "cursor_wait",
      "cursor_wait_any",
      "cursor_wait_all",
    ],
  );
});
```

- [ ] **Step 2: Run tool-schema tests — expect FAIL**

Run: `node --import tsx --test tests/tool-schemas.test.ts`

Expected: FAIL with module export missing (`buildTools` / `buildRunInputSchema` not found).

- [ ] **Step 3: Rewrite `src/tool-schemas.ts` as a builder**

Replace the file with:

```typescript
// JSON Schemas + descriptions for the six MCP tools (model-facing).

import type { Config, ModelEntry } from "./types.js";

export function buildRecommendedModelsBlurb(
  models: Record<string, ModelEntry>,
): string {
  return Object.entries(models)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(
      ([id, m]) =>
        `${id} — ${m.label} — $${m.price.input}/$${m.price.output}`,
    )
    .join("; ");
}

export function buildRunInputSchema(
  config: Pick<Config, "default" | "models">,
) {
  const modelIds = Object.keys(config.models).sort();
  return {
    type: "object",
    properties: {
      prompt: {
        type: "string",
        description: "The task for the delegated Cursor agent. Required.",
      },
      model: {
        type: "string",
        enum: modelIds,
        description:
          `Curated model id from the allow-list. Default '${config.default}' when omitted.`,
      },
      requireNonClaude: {
        type: "boolean",
        description:
          "When true, hard-reject if the resolved model family is 'claude'. Default false.",
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
        description: "Return {status:'RUNNING', jobId} immediately without blocking.",
      },
    },
    required: ["prompt"],
  };
}

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

export function buildTools(config: Pick<Config, "default" | "models">) {
  const blurb = buildRecommendedModelsBlurb(config.models);
  const runSchema = buildRunInputSchema(config);
  return [
    {
      name: "cursor_run",
      description:
        "Delegate a coding/research task to a Cursor model via the local cursor-agent CLI. " +
        "Fast tasks return a full RunOutput (status, text, usage, cost, git change-set, gate result). " +
        "Slow tasks detach and return {status:'RUNNING', jobId} to poll/wait on. " +
        "A write to a locked CallerProvided path returns {status:'BUSY', jobId, busyPath}. " +
        `Recommended models: ${blurb}. Default model: ${config.default}.`,
      inputSchema: runSchema,
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
  ];
}
```

- [ ] **Step 4: Run tool-schema tests — expect PASS**

Run: `node --import tsx --test tests/tool-schemas.test.ts`

Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/tool-schemas.ts tests/tool-schemas.test.ts
git commit -m "$(cat <<'EOF'
feat(schemas): build model enum and recommended-models blurb from config

EOF
)"
```

---

### Task 7: Wire runner + index; hard-cut remaining callers; delete old config files

**Files:**
- Modify: `src/runner.ts:1-119` (`resolveModel` import/call; `backend: "cursor"`; pass `requireNonClaude`)
- Modify: `src/index.ts:24-25,109-115,130-138` (`buildTools`; `models.json` path)
- Modify: `tests/runner.test.ts` (new `Config` fixture; replace diversity test)
- Modify: `tests/index.test.ts` (new `Config` fixture; use `buildTools`)
- Modify: `tests/integration.live.test.ts:19-33` (`modelsPath`; drop `tier`)
- Delete: `config/tier-map.json`, `config/price-map.json`
- Test: `tests/runner.test.ts`, `tests/index.test.ts`, then full `npm test`

**Interfaces:**
- Consumes:
  - `resolveModel({ model, requireNonClaude }, { default: config.default, models: config.models })` from Task 3
  - `buildTools(config)` from Task 6
  - `loadConfig({ modelsPath })` from Task 2
- Produces: end-to-end wired server — `cursor_run` resolves via allow-list; tools listed from config; old tier/price files gone

- [ ] **Step 1: Update runner tests for the new Config + NonClaudeViolationError**

In `tests/runner.test.ts`:

1. Replace the import of `DiversityClaudeError` with:

```typescript
import { NonClaudeViolationError } from "../src/models.js";
```

2. Replace the `config` fixture with:

```typescript
const config: Config = {
  default: "composer-2.5",
  models: {
    "composer-2.5": {
      label: "Composer 2.5",
      family: "composer",
      price: { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    },
    "grok-4.5-xhigh": {
      label: "Grok 4.5",
      family: "grok",
      price: { input: 2, output: 6, cacheRead: 0.5, cacheWrite: 0 },
    },
    "claude-sonnet-4": {
      label: "Claude Sonnet 4",
      family: "claude",
      price: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
    },
  },
  priceMap: {
    "composer-2.5": { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    "grok-4.5-xhigh": { input: 2, output: 6, cacheRead: 0.5, cacheWrite: 0 },
    "claude-sonnet-4": { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
  },
  profile: {
    promptPreamble: "PREAMBLE",
    requiredDeny: ["rm -rf /"],
    gate: "make ci",
  },
};
```

3. Replace the test `"diversity + a Claude model throws"` with:

```typescript
test("requireNonClaude + a Claude model throws", async () => {
  const { registry } = fakeRegistry();
  await assert.rejects(
    () =>
      runDelegation(
        {
          prompt: "x",
          model: "claude-sonnet-4",
          requireNonClaude: true,
        },
        depsWith(registry, []),
      ),
    NonClaudeViolationError,
  );
});
```

Leave the other runner tests unchanged (they already expect default `composer-2.5`).

- [ ] **Step 2: Run runner tests — expect FAIL**

Run: `node --import tsx --test tests/runner.test.ts`

Expected: FAIL (`Cannot find module './tiers.js'` from `src/runner.ts`, and/or `config.tierMap` access).

- [ ] **Step 3: Update `src/runner.ts`**

1. Change the import:

```typescript
import { resolveModel } from "./models.js";
```

2. Replace the resolve + JobSpec model/backend wiring inside `runDelegation`:

```typescript
  const resolved = resolveModel(
    { model: input.model, requireNonClaude: input.requireNonClaude },
    { default: config.default, models: config.models },
  );
```

3. In `buildArgv` / `JobSpec` construction, use:

```typescript
  const argv = buildArgv({
    model: resolved.model,
    capFlags: cap.flags,
    isoFlags: iso.flags,
    session: input.session,
    prompt,
  });

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
    allowPartialCommit: input.allowPartialCommit ?? false,
    waitMs: input.waitMs,
    background: input.background,
    priceMap: config.priceMap,
    downgraded: cap.downgraded,
    worktreeName,
  };
```

(Only `resolveModel` call site and `backend: "cursor"` change relative to today; keep the rest of the function body.)

- [ ] **Step 4: Run runner tests — expect PASS**

Run: `node --import tsx --test tests/runner.test.ts`

Expected: PASS.

- [ ] **Step 5: Update index tests for `buildTools` + new Config**

Replace the top of `tests/index.test.ts` (imports + config + tools assertion) as follows:

```typescript
import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer, handleCall, type ServerDeps } from "../src/index.js";
import { buildTools } from "../src/tool-schemas.js";
import type { JobRegistry } from "../src/job-registry.js";
import type { Config } from "../src/types.js";

const config: Config = {
  default: "composer-2.5",
  models: {
    "composer-2.5": {
      label: "Composer 2.5",
      family: "composer",
      price: { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    },
  },
  priceMap: {
    "composer-2.5": { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
  },
  profile: {},
};

// ... keep fakeRegistry() and deps() unchanged ...

test("exposes six tools from buildTools", () => {
  const tools = buildTools(config);
  assert.equal(tools.length, 6);
  const names = tools.map((t) => t.name);
  assert.deepEqual(names, [
    "cursor_run",
    "cursor_poll",
    "cursor_cancel",
    "cursor_wait",
    "cursor_wait_any",
    "cursor_wait_all",
  ]);
});
```

Keep the remaining `handleCall` / `createServer` tests as they are (they use `deps()` which now carries the new `config` shape).

- [ ] **Step 6: Update `src/index.ts` to load `models.json` and register dynamic tools**

1. Change the tool-schemas import:

```typescript
import { buildTools } from "./tool-schemas.js";
```

2. Inside `createServer`, replace the static `TOOLS.map` handler with:

```typescript
  const tools = buildTools(deps.config);

  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: tools.map((t) => ({
      name: t.name,
      description: t.description,
      inputSchema: t.inputSchema,
    })),
  }));
```

3. In `buildDeps`, replace the `loadConfig` call with:

```typescript
  const config = await loadConfig({
    modelsPath: join(configDir, "models.json"),
  });
```

- [ ] **Step 7: Update the live integration test paths**

In `tests/integration.live.test.ts`, change the `loadConfig` call and run input to:

```typescript
  const config = await loadConfig({
    modelsPath: join(configDir, "models.json"),
  });
```

and:

```typescript
  const res = (await runDelegation(
    {
      prompt:
        "Reply with exactly the word OK and then a final line 'STATUS: DONE'. Do not run any commands.",
      model: "composer-2.5",
      capability: "ask",
      waitMs: 120000,
    },
    { config, registry, cliConfig: null, serverCwd: process.cwd() },
  )) as RunOutput;
```

- [ ] **Step 8: Delete replaced config files**

```bash
rm config/tier-map.json config/price-map.json
```

- [ ] **Step 9: Run index + runner + models + config + validate + tool-schemas + pricing — expect PASS**

Run:

```bash
node --import tsx --test \
  tests/config.test.ts \
  tests/models.test.ts \
  tests/pricing.test.ts \
  tests/validate.test.ts \
  tests/tool-schemas.test.ts \
  tests/runner.test.ts \
  tests/index.test.ts
```

Expected: PASS (all listed files).

- [ ] **Step 10: Run the full unit suite — expect PASS**

Run: `npm test`

Expected: PASS. Note: `npm test` globs `tests/**/*.test.ts`, which **does** match `tests/integration.live.test.ts` — but that file self-gates on the `CURSOR_DELEGATE_LIVE` env var (unset under `npm test`), so it registers no live assertions and never spawns the real CLI. It still must compile, which is why Step 7 updates its `loadConfig` call.

If any remaining test file still constructs `{ tierMap, priceMap }` Config objects, update those fixtures to the Task 1 `Config` shape before declaring PASS. Known fixtures after the edits above: `tests/runner.test.ts`, `tests/index.test.ts`. Grep to confirm none remain:

```bash
rg "tierMap|tierOverrides|priceOverrides|from \"../src/tiers|tier-map|price-map" tests src
```

Expected: no matches.

- [ ] **Step 11: Commit**

```bash
git add src/runner.ts src/index.ts \
  tests/runner.test.ts tests/index.test.ts tests/integration.live.test.ts
git rm -f config/tier-map.json config/price-map.json
git commit -m "$(cat <<'EOF'
feat(model-layer): hard-cut tiers; wire allow-list resolver and dynamic schemas

EOF
)"
```

---

## Self-Review

### 1. Spec coverage (scoped sections only)

| Spec item | Task |
|-----------|------|
| §3.1 Drop tiers entirely (hard cut, no shim) | Tasks 1, 5, 7 (types, validate ignore, delete files) |
| §3.2 Curated allow-list replaces tier-map + price-map | Tasks 1–3, 7 |
| §3.3 `nonClaudeDiversity` kept via family tag, hard reject | Tasks 3, 5, 7 (`requireNonClaude`) |
| §4.1 `config/models.json` seed + schema | Task 1 |
| §4.2 Extension/override via host-profile `models`/`default`; no raw passthrough | Tasks 2–3 |
| §4.3 Schema enum + blurb at startup; builder | Task 6, wired in Task 7 |
| §4.4 `resolveModel` + errors; delete tier machinery | Task 3 |
| §5 `requireNonClaude` param + hard reject | Tasks 3, 5, 7 |
| §10 Pricing sourced from `models.json` | Tasks 2, 4, 7 (`priceMap` derived) |
| §11 tests: load/merge, schema/blurb, resolver, validate, pricing | Tasks 2–6 |
| §12 Migration hard cut (delete tier-map, tiers.ts, tiers.test, Tier, tierOverrides, tier schema, price-map) | Tasks 1, 3, 5, 6, 7 |
| §6 / §7 / §8 | Explicitly excluded — no tasks |

### 2. Placeholder scan

No TBD/TODO steps. Every code step includes full source. Every command includes the exact invocation. Types/functions referenced later (`resolveModel`, `buildTools`, `loadConfig`, `ModelEntry`, `NonClaudeViolationError`, etc.) are defined in an earlier task.

### 3. Type consistency

- `ResolvedModel` is `{ model, family, price }` everywhere (Task 1 definition; Task 3 return; Task 7 runner uses `resolved.model` and hardcodes `backend: "cursor"`).
- `Config` is `{ default, models, priceMap, profile }` from Task 1 through Tasks 2–7.
- `HostProfile` override keys are `default` + `models` (not `tierOverrides` / `priceOverrides`).
- `RunInput` has `model?` + `requireNonClaude?` and no `tier`.
- `computeCost` still takes `PriceMap`; `Config.priceMap` is derived at load time so `JobSpec` / finalize stay unchanged.
- Error names: `ModelNotAllowedError`, `NonClaudeViolationError` (not the deleted `DiversityClaudeError` / `TierResolutionError`).
