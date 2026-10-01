# delegate

A small CLI that runs coding and research tasks on Cursor, pi, or Claude Code models by driving the local `cursor-agent` binary in headless mode. Jobs are detached supervisors; you poll status from JSON files on disk.

## Install

From this repo:

```bash
./bin/setup.sh
```

This builds `delegate`, copies it to `~/.local/bin/delegate`, optionally migrates an old host profile, and installs the Claude Code plugin (skill only).

You need Rust (`cargo`), and `cursor-agent` on PATH with `cursor-agent login` before read-write jobs.

## Commands

| Command | Purpose |
| --- | --- |
| `delegate run` | Start a job (prompt on stdin or `--prompt-file`); prints the job id. |
| `delegate resume <jobId>` | Continue a finished job that has a session id; optional model/capability/gate overrides. |
| `delegate cancel <jobId>` | Stop a running job and print its terminal record. |
| `delegate watch <jobId>...` | Block until each job is terminal (optional `--timeout` seconds). |
| `delegate models` | List configured model ids, labels, backends, and prices. |
| `delegate doctor` | Check the binary, `cursor-agent`, login, and model menu drift. |

Capabilities: `read-only` (ask mode) or `read-write` (sandbox disabled with force). Read-write jobs take an exclusive lock on the working directory.

## Backends

| Backend | Status |
| --- | --- |
| `cursor` | Implemented (`cursor-agent`). |
| `pi` | Planned; lands in a later release. |
| `claude` | Planned; lands in a later release. |

Model ids and prices come from bundled `config/models.json`, merged with your host profile.

## Status records

Each job writes `$TMPDIR/delegate-jobs/<jobId>.json` (use `$TMPDIR` when set, otherwise the OS temp directory). Prompt text is stored alongside as `<jobId>.prompt` until the supervisor starts.

## Host profile

Optional JSON at `~/.config/delegate/host-profile.json` (or `$XDG_CONFIG_HOME/delegate/host-profile.json`). Override the path with `DELEGATE_HOST_PROFILE`. Keys can set `default`, `models`, `gate`, `idleMs`, and `toolIdleMs`. Missing file is fine; defaults are built in.

## Plugin

The Claude Code plugin ships only the skill under `skills/delegate/`. Run setup to register marketplace `delegate` and install `delegate@delegate`.
