# Spec: a non-blocking wait UX for cursor-delegate jobs

## Goals

- Give a `cursor_run` job a wait path that doesn't force the orchestrating agent to block a
  whole turn inside `cursor_wait`, and doesn't require it to manually loop `cursor_poll`
  itself. The registry persists each job's status to disk on its own transitions; the
  orchestrator watches that file in the background and is notified once, when the job
  reaches a terminal state, with the full result already in hand — no follow-up poll call.
- Make cursor-delegate discoverable as a Claude Code plugin — listed, description-matched,
  invocable on demand — on both hosts where it runs today (the Mac and arca), so the
  orchestration guidance already written for it actually reaches an agent instead of sitting
  unreachable behind a raw MCP-server registration.
- Document the new wait pattern, including the multi-job batch variant, in the existing
  skill guidance for calling `cursor_run`, so an agent knows when to reach for it versus the
  existing blocking wait tools.
- Add a self-check that verifies the plugin migration held: the plugin stays enabled, the
  legacy raw registration stays removed, and the server an agent actually talks to is the
  one the plugin provides.

## Non-goals

- Removing or deprecating the existing blocking wait tools. They remain the right choice for
  jobs expected to finish well under a minute; the new path is additive, for jobs where
  blocking a full turn is wasteful.
- Live progress narration — surfacing a job's in-flight tool calls, token counts, or
  intermediate assistant text as a stream of notifications while it runs. Investigated and
  rejected: the underlying event stream is far too fine-grained for a one-notification-per-
  line model to stay useful: it would cost more attention than it saves. This spec is
  terminal-status-only.
- A general framework for how MCP servers should support backgrounding. This is scoped to
  cursor-delegate specifically.

## Functional requirements

- Every dispatched job gets a status record persisted to a location derived deterministically
  from its job ID, independent of any other job's record.
- The record is written at exactly two points in a job's life: when it starts running, and
  when it reaches a terminal state. No write happens on intermediate progress events.
- The terminal-state record carries the complete result an equivalent `cursor_poll` call
  would return for that job — not merely a status label — so that reading the record once it
  is terminal fully answers "what happened," with no further tool call required.
- An orchestrating agent can wait on a single job's terminal state without blocking a whole
  turn, and is notified exactly once, at the moment that job reaches a terminal state.
- An orchestrating agent can wait on a batch of jobs' terminal states and be notified once
  when the whole batch is done, without having to individually track each job's completion
  notification itself.
- cursor-delegate's skill guidance (its "how to call `cursor_run`" documentation) is reachable
  by an agent through Claude Code's normal skill discovery — listed among available skills,
  matched by description, invocable on demand — on every host cursor-delegate runs on.
  Reaching it does not depend on an agent already knowing to look for a raw MCP server entry.
- Only one live registration of the cursor-delegate MCP server exists per host once this work
  lands; the legacy raw registration is gone, not left in place alongside the plugin's own.
- The skill guidance documents, with a runnable example, both the single-job wait pattern and
  the multi-job batch variant, alongside the existing blocking wait tools' documentation, so an
  agent can choose correctly between them.
- A diagnostic check confirms, on demand: the plugin is enabled, the legacy raw registration is
  actually absent (not silently reintroduced), and the MCP server currently reachable under the
  cursor-delegate name is the plugin-sourced one.

## Non-functional requirements

- The status-record mechanism must not increase per-job write volume beyond the two
  transition points above; it must not piggyback on the existing per-event progress callbacks
  (tool-call, stderr, raw-activity events), which fire far more often than status changes.
- Record placement must not assume or require any particular caller working directory,
  since a job's own working directory is caller-supplied per call and unrelated to where
  cursor-delegate itself happens to be installed.
- Record cleanup must not race a still-active reader: nothing removes a record's storage out
  from under a wait that has not yet observed it reach a terminal state. Reclamation may be
  deferred to the host OS's own temporary-storage lifecycle rather than performed eagerly by
  cursor-delegate.
- The plugin registration must be reproducible identically on every host cursor-delegate runs
  on, using the same manifest layout and marketplace convention already proven to work for at
  least one other locally-installed plugin on this machine.

## Premises

- Claude Code's MCP client does not surface generic server-initiated notifications as chat
  messages — it handles only a fixed set of protocol notification types, none of which are a
  generic "here is an update" channel. A design that needs the orchestrator to learn about a
  job's progress therefore has to have the orchestrator actively watch something (a file), not
  wait to be pushed to.
- A wait for one job's terminal state is a single-notification case by nature — the state
  either changes once or the wait times out — and the tool built for open-ended or repeating
  event streams is not the right instrument for it; a background shell wait built for exactly
  one completion notification is.
- The job registry currently tracks all job state purely in memory, with no persisted record
  of any kind. Persisting a status record is new capability, not exposing something that
  already exists.
- A job's tracked status changes at exactly two points in its life — start and terminal
  transition — never on the intermediate events that update its in-flight progress fields.
  This is what makes "write at transition points" and "carry the full result" simultaneously
  cheap: neither adds write volume beyond what a bare-status design would already cost.
- cursor-delegate is registered as a raw MCP-server entry, not as a plugin, identically on
  every host it runs on today. Its skill guidance already exists in the repository in a form
  meant to be shipped as a plugin skill, but that packaging is not yet wired up to be
  discoverable by an agent on any host.
- The plugin manifest layout already present in the repository does not match the layout
  Claude Code's plugin system expects, as verified against the one plugin confirmed working
  locally today. Wiring up discoverability is therefore a manifest restructure plus
  registration, not merely flipping an enablement switch on something already correctly
  shaped.
- The existing skill guidance for cursor-delegate was checked against the current codebase —
  its model list, its documented tool parameters, and its claim about prompt-injected status
  conventions — and found to still accurately describe current behavior. The documentation
  work here is a pure addition, not a fixup of drift.

## Approaches considered

- **Watch mechanism: a background shell wait with a one-shot completion notification**, chosen
  over a tool built for streaming or repeating event notifications. The latter is designed to
  stay armed across many occurrences and explicitly documents itself as the wrong choice for a
  single "tell me when this is done" wait — using it here would fight its own intended use
  rather than fit it.
- **One status record per job**, chosen over a single shared record covering the whole
  registry. A shared record would need write-serialization and atomic-replace handling to stay
  safe under concurrently running jobs and would require a reader to parse out "this job's"
  entry from a growing structure; per-job records need neither, at the cost of one more small
  file per job.
- **The terminal record carries the full result**, chosen over carrying only a status label.
  Carrying the full result costs nothing extra in write volume, since it only happens at the
  same two transition points a bare-status design would use anyway — and it means the
  orchestrator's wait resolves with a complete answer already in hand, matching what the
  existing blocking wait already returns today, rather than resolving with a signal that a
  second call is still needed.
- **Reclamation is deferred to the host OS**, chosen over eagerly deleting each job's record
  once consumed. Eager deletion introduces a real race against a wait that has not yet
  performed its final read; deferring reclamation avoids that race entirely at the cost of
  records persisting slightly longer than strictly necessary, which nothing in this design
  needs to avoid.
- **The legacy raw registration is removed, not kept alongside the plugin.** Keeping both risks
  two live registrations under the same server name with no established precedent for how that
  resolves, and keeping the raw entry as a fallback would leave in place exactly the
  registration fragmentation this work exists to remove.
- **The multi-job batch wait is documentation, not new mechanism.** Once every job already has
  its own status record, collapsing several jobs' completions into one notification is a
  matter of how the orchestrator constructs its wait, not something the registry needs to
  support specially — so it is scoped as an example alongside the single-job pattern rather
  than as a separate capability.

## Out of scope

- Removing or deprecating the existing blocking wait tools (`cursor_wait` and its
  any/all variants).
- Live progress narration of a running job's intermediate events.
- A generalized framework for MCP-server backgrounding beyond cursor-delegate.

## Status-record mechanism

Each job's record is a self-contained snapshot of exactly what a poll call would return for
that job at the moment it was written: a progress snapshot while running, or the complete
result once terminal. It is written twice per job — once when the job starts, once when it
reaches whichever terminal state it ends in (including the "parked, awaiting an answer" state,
which is terminal from a waiter's point of view even though the job itself isn't finished) —
and never in between. Its storage location is derived only from the job's own identifier, is
independent of any caller-supplied working directory, and lives somewhere the host OS already
reclaims on its own schedule, so no explicit deletion step is required or attempted.

An orchestrator waiting on one job arms a single background wait that repeatedly reads the
record and exits the moment it stops reporting the running state, printing the record's final
content as it exits — so the same notification that says "this finished" also says what
happened. Waiting on several jobs at once follows the identical pattern extended across their
records, collapsing what would otherwise be one notification per job into a single "the whole
batch is done" notification.

## Plugin registration

cursor-delegate's plugin manifest is restructured into the layout Claude Code's plugin system
expects, mirroring the one other locally-installed plugin already proven to work that way, and
declared as its own local directory-sourced marketplace. Registration — adding that
marketplace and enabling the plugin — is performed identically on every host cursor-delegate
runs on. The legacy raw MCP-server registration is removed on every host once the plugin's own
server registration is confirmed to take over cleanly, so exactly one registration of the
server exists per host at any time.

## Self-review

- No placeholders or TBDs remain.
- Sections agree with each other: the functional requirements, the status-record mechanism
  section, and the approaches-considered entries describe the same two-write, per-job,
  full-payload design without contradiction; the plugin-registration requirements and section
  describe the same restructure-then-register-then-remove-legacy sequence.
- Scope is one coherent piece of work: a wait mechanism, the plugin registration that makes its
  documentation reachable, the documentation itself, and one diagnostic check on the migration
  — not a multi-workstream project.
- No requirement reads two ways: "exactly two write points," "carries the complete result,"
  "notified exactly once," and "one registration per host" are each single, checkable
  conditions.
