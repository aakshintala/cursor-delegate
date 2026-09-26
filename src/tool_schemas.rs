use crate::types::{Config, ModelEntry};
use serde_json::{Value, json};
use std::collections::HashMap;

pub fn build_recommended_models_blurb(models: &HashMap<String, ModelEntry>) -> String {
    let mut entries: Vec<_> = models.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
        .into_iter()
        .map(|(id, m)| {
            format!(
                "{id} — {} — ${}/${}",
                m.label, m.price.input, m.price.output
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn build_run_input_schema(config: &Config) -> Value {
    let mut model_ids: Vec<_> = config.models.keys().cloned().collect();
    model_ids.sort();
    json!({
        "type": "object",
        "properties": {
            "prompt": {
                "type": "string",
                "description": "The task for the delegated Cursor agent. Required.",
            },
            "model": {
                "type": "string",
                "enum": model_ids,
                "description": format!(
                    "Curated model id from the allow-list. Default '{}' when omitted.",
                    config.default
                ),
            },
            "requireNonClaude": {
                "type": "boolean",
                "description": "When true, hard-reject if the resolved model family is 'claude'. Default false.",
            },
            "capability": {
                "type": "string",
                "enum": ["ask", "plan", "write", "write-unsandboxed"],
                "description": "ask/plan are read-only. write runs sandboxed+non-interactive. write-unsandboxed also needs allowUnsandboxed:true (else it is downgraded to write). Default 'ask'.",
            },
            "allowUnsandboxed": {
                "type": "boolean",
                "description": "Required second signal to actually run write-unsandboxed.",
            },
            "session": {
                "type": "string",
                "description": "Resume a prior sessionId for continuity.",
            },
            "isolation": {
                "description": "Where the agent works. {type:'None'} = server cwd; {type:'CallerProvided',path} = a named workspace (participates in the write lock); {type:'BackendProvided',name?,base?} = a cursor worktree.",
                "oneOf": [
                    {
                        "type": "object",
                        "properties": { "type": { "const": "None" } },
                        "required": ["type"],
                    },
                    {
                        "type": "object",
                        "properties": {
                            "type": { "const": "CallerProvided" },
                            "path": { "type": "string" },
                        },
                        "required": ["type", "path"],
                    },
                    {
                        "type": "object",
                        "properties": {
                            "type": { "const": "BackendProvided" },
                            "name": { "type": "string" },
                            "base": { "type": "string" },
                        },
                        "required": ["type"],
                    },
                ],
            },
            "verifyCommands": {
                "type": "array",
                "items": { "type": "string" },
                "description": "The ONLY verification commands the agent may run (injected into the prompt). Overrides the profile default.",
            },
            "gate": {
                "type": "string",
                "description": "A postcondition command the TOOL runs after the agent. A failing gate downgrades DONE to DONE_WITH_CONCERNS.",
            },
            "allowPartialCommit": {
                "type": "boolean",
                "description": "Suppress the incomplete-commit concern (commits landed but tree still dirty).",
            },
            "waitMs": {
                "type": "number",
                "description": "How long to block before auto-detaching to a jobId. Clamped to [1000, 600000]. Default ~60s.",
            },
            "idleMs": {
                "type": ["number", "null"],
                "description": "Override the idle watchdog window applied while NO tool call is in flight (i.e. waiting on the model itself) — a silence here is a real hang signal. null disables it for this job. Defaults to the server profile's idleMs (currently 300000ms / 5min). You usually don't need this: a running tool call uses the separate, wider toolIdleMs window automatically.",
            },
            "toolIdleMs": {
                "type": ["number", "null"],
                "description": "Override the idle watchdog window applied while a tool call IS in flight (e.g. a shell command, build, or test suite) — this can legitimately go silent for a long time, so it defaults much wider than idleMs. null disables the watchdog for in-flight tool calls on this job. Defaults to the server profile's toolIdleMs (currently 1800000ms / 30min).",
            },
            "background": {
                "type": "boolean",
                "description": "Return {status:'RUNNING', jobId} immediately without blocking.",
            },
        },
        "required": ["prompt"],
    })
}

#[derive(Clone, Debug)]
pub struct ToolDesc {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub fn build_tools(config: &Config) -> Vec<ToolDesc> {
    let blurb = build_recommended_models_blurb(&config.models);
    let run_schema = build_run_input_schema(config);
    let job_id_schema = json!({
        "type": "object",
        "properties": { "jobId": { "type": "string" } },
        "required": ["jobId"],
    });
    let job_ids_timeout = json!({
        "type": "object",
        "properties": {
            "jobIds": { "type": "array", "items": { "type": "string" } },
            "timeoutMs": { "type": "number" },
        },
        "required": ["jobIds"],
    });
    let wait_schema = json!({
        "type": "object",
        "properties": {
            "jobId": { "type": "string" },
            "timeoutMs": { "type": "number" },
        },
        "required": ["jobId"],
    });
    let answer_schema = json!({
        "type": "object",
        "properties": {
            "jobId": {
                "type": "string",
                "description": "jobId of a parked run that ended with status NEEDS_CONTEXT.",
            },
            "answer": {
                "type": "string",
                "description": "Orchestrator answer to the delegated agent's question (becomes the resume prompt).",
            },
        },
        "required": ["jobId", "answer"],
    });
    vec![
        ToolDesc {
            name: "cursor_run".into(),
            description: format!(
                "Delegate a coding/research task to a Cursor model via the local cursor-agent CLI. \
Fast tasks return a full RunOutput (status, text, usage, cost, git change-set, gate result). \
Slow tasks detach and return {{status:'RUNNING', jobId}} to poll/wait on. \
A write to a locked CallerProvided path returns {{status:'BUSY', jobId, busyPath}}. \
Recommended models: {blurb}. Default model: {}.",
                config.default
            ),
            input_schema: run_schema,
        },
        ToolDesc {
            name: "cursor_poll".into(),
            description: "Non-blocking status check for a jobId. Returns {RUNNING, progress, lastHeartbeatAt, supersededBy?} or {<terminal>, result, supersededBy?} or {NOT_FOUND}. lastHeartbeatAt (server ms) lets you detect a dead server; supersededBy points at the new jobId when a NEEDS_CONTEXT job was resumed.".into(),
            input_schema: job_id_schema.clone(),
        },
        ToolDesc {
            name: "cursor_cancel".into(),
            description: "SIGTERM the job's child, mark it CANCELLED, and return the resulting poll result.".into(),
            input_schema: job_id_schema,
        },
        ToolDesc {
            name: "cursor_wait".into(),
            description: "Long-poll: block until the job is terminal or timeoutMs elapses (default 120000, clamp [1000,600000]). On timeout returns the RUNNING snapshot. Works on a BUSY jobId (waits for the lock holder).".into(),
            input_schema: wait_schema,
        },
        ToolDesc {
            name: "cursor_wait_any".into(),
            description: "Block until the FIRST listed job is terminal (or timeout). Returns {jobs, firstDone?}. Pairs with background:true for parallel dispatch.".into(),
            input_schema: job_ids_timeout.clone(),
        },
        ToolDesc {
            name: "cursor_wait_all".into(),
            description: "Block until ALL listed (known) jobs are terminal (or timeout). Returns {jobs, allDone}. Empty input -> {jobs:{}, allDone:true}.".into(),
            input_schema: job_ids_timeout,
        },
        ToolDesc {
            name: "cursor_answer".into(),
            description: "Resume a parked NEEDS_CONTEXT job: look up its sessionId and original run context \
(isolation, capability, verifyCommands, gate, model), then continue via \
--resume <sessionId> with `answer` as the prompt. Returns the same shape as \
cursor_run (terminal, NEEDS_CONTEXT again, or RUNNING/jobId). \
Unknown/expired jobId → {status:'NOT_FOUND'}; a job not awaiting input is rejected.".into(),
            input_schema: answer_schema,
        },
        ToolDesc {
            name: "doctor".into(),
            description: "Diagnose cursor-delegate setup: plugin version, cursor-agent binary + --version, \
account login via `cursor-agent about` (email, subscription, current model), and \
model-menu drift (config/models.json ids vs `cursor-agent models` / `--list-models`). \
Configured ids missing from the account list are WARNINGS, not failures. \
Prices are not checkable via the CLI.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{HostProfile, Price};

    fn models() -> HashMap<String, ModelEntry> {
        let mut m = HashMap::new();
        m.insert(
            "composer-2.5".into(),
            ModelEntry {
                label: "Composer 2.5".into(),
                family: "composer".into(),
                price: Price {
                    input: 0.5,
                    output: 2.5,
                    cache_read: 0.2,
                    cache_write: 0.0,
                },
            },
        );
        m.insert(
            "grok-4.5-xhigh".into(),
            ModelEntry {
                label: "Grok 4.5".into(),
                family: "grok".into(),
                price: Price {
                    input: 2.0,
                    output: 6.0,
                    cache_read: 0.5,
                    cache_write: 0.0,
                },
            },
        );
        m
    }

    fn config() -> Config {
        let models = models();
        Config {
            default: "composer-2.5".into(),
            price_map: models.iter().map(|(k, v)| (k.clone(), v.price)).collect(),
            models,
            profile: HostProfile::default(),
        }
    }

    #[test]
    fn blurb_lists_id_label_prices() {
        let blurb = build_recommended_models_blurb(&models());
        assert!(blurb.contains("composer-2.5 — Composer 2.5 — $0.5/$2.5"));
        assert!(blurb.contains("grok-4.5-xhigh — Grok 4.5 — $2/$6"));
    }

    #[test]
    fn run_input_schema_enum() {
        let schema = build_run_input_schema(&config());
        let mut ids: Vec<String> = schema["properties"]["model"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        ids.sort();
        assert_eq!(ids, ["composer-2.5", "grok-4.5-xhigh"]);
        assert!(schema["properties"].get("tier").is_none());
        assert_eq!(schema["properties"]["requireNonClaude"]["type"], "boolean");
    }

    #[test]
    fn build_tools_wires_description() {
        let tools = build_tools(&config());
        assert_eq!(tools.len(), 8);
        let run = tools.iter().find(|t| t.name == "cursor_run").unwrap();
        assert!(run.description.contains("composer-2.5 — Composer 2.5"));
        assert!(run.description.contains("Default model: composer-2.5"));
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "cursor_run",
                "cursor_poll",
                "cursor_cancel",
                "cursor_wait",
                "cursor_wait_any",
                "cursor_wait_all",
                "cursor_answer",
                "doctor",
            ]
        );
    }

    #[test]
    fn doctor_schema_no_input() {
        let tools = build_tools(&config());
        let doctor = tools.iter().find(|t| t.name == "doctor").unwrap();
        assert!(doctor.description.to_lowercase().contains("plugin"));
        assert!(
            doctor.description.contains("model menu") || doctor.description.contains("models.json")
        );
        assert!(doctor.description.to_lowercase().contains("warning"));
        assert_eq!(doctor.input_schema["properties"], json!({}));
        assert!(
            doctor.input_schema.get("required").is_none()
                || doctor.input_schema["required"]
                    .as_array()
                    .map(|a| a.is_empty())
                    .unwrap_or(true)
        );
    }

    #[test]
    fn cursor_answer_schema() {
        let tools = build_tools(&config());
        let answer = tools.iter().find(|t| t.name == "cursor_answer").unwrap();
        assert_eq!(answer.input_schema["required"], json!(["jobId", "answer"]));
        assert_eq!(answer.input_schema["properties"]["jobId"]["type"], "string");
        assert_eq!(
            answer.input_schema["properties"]["answer"]["type"],
            "string"
        );
        assert!(answer.description.contains("NEEDS_CONTEXT"));
    }
}
