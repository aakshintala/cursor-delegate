# Agent instructions

## Agent skills

- **Issues and specs**: GitHub Issues on this repo, via `gh`. Before publishing or fetching a ticket, or running `/wayfinder`, read `docs/agents/issue-tracker.md`.
- **Triage labels**: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`; meanings in `docs/agents/triage-labels.md`.
- **Domain docs**: before exploring an area or naming a domain concept, read `docs/agents/domain.md`.

## Parser fixtures

- `tests/fixtures/contract/<backend>/`: curated streams that parser tests assert on. Record one with `FIXTURE_KIND=contract scripts/record.sh BACKEND MODEL NAME < prompt`.
- `tests/fixtures/recorded/<backend>/`: the raw archive of real runs, which tests only parse. Never edit these by hand. `scripts/record.sh` writes here by default.
- When a ticket needs a new contract fixture, the orchestrator records it before the lane starts, because recording runs a real model.
