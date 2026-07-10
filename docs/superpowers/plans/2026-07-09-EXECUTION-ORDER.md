# Execution order — cursor-delegate redesign plans

These four plans implement one design spec
(`docs/superpowers/specs/2026-07-09-cursor-delegate-model-layer-design.md`).
They share the `buildTools(config)` surface and must be executed **in this order**,
because each later plan assumes the earlier ones are merged.

1. **`2026-07-09-model-layer.md`** — foundational. Ends with `buildTools` returning
   **6** tools (`cursor_run` … `cursor_wait_all`). Everything else builds on this.
2. **`2026-07-09-needs-input.md`** — adds the `cursor_answer` tool → `buildTools`
   returns **7**. Assumes model-layer is merged.
3. **`2026-07-09-doctor.md`** — adds the `doctor` tool → `buildTools` returns **8**.
   Assumes model-layer **and** needs-input are merged (so `cursor_answer` is the 7th
   tool and `doctor` is the 8th). Its `buildTools` tests assert length 8 and the
   name list `[…6…, "cursor_answer", "doctor"]`.
4. **`2026-07-09-delegate-skill.md`** — documentation only (skill + catalog rewrite);
   does not touch `buildTools`. Can run any time after model-layer, but last is
   simplest since its examples reference `cursor_answer` and the final tool surface.

**Why the order matters:** plans 2 and 3 both edit `tests/tool-schemas.test.ts` and
`tests/index.test.ts` tool-count/name assertions. Run out of order and those
assertions collide. Plan 3 (doctor) is written to assume the 7-tool baseline plan 2
produces.

Each plan was drafted by `grok-4.5-xhigh` and reviewed against the live codebase.
Known fixes already applied to the drafts: doctor's tool-count assertions (7→8,
incl. `cursor_answer`) and its `parseAbout` parser (real whitespace-column
`agent about` format, not `Label: value`).
