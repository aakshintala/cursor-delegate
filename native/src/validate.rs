use crate::types::RunInput;
use serde_json::Value;

fn invalid(field: &str, detail: Option<&str>) -> String {
    match detail {
        Some(d) => format!("invalid {field}: {d}"),
        None => format!("invalid {field}"),
    }
}

fn parse_isolation(v: &Value) -> Result<crate::types::Isolation, String> {
    let o = v.as_object().ok_or_else(|| invalid("isolation", None))?;
    let ty = o.get("type").and_then(|x| x.as_str()).unwrap_or("");
    match ty {
        "None" => Ok(crate::types::Isolation::None),
        "CallerProvided" => {
            let path = o.get("path").and_then(|x| x.as_str()).unwrap_or("");
            if path.is_empty() {
                return Err(invalid("isolation.path", Some("required non-empty string")));
            }
            Ok(crate::types::Isolation::CallerProvided {
                path: path.to_string(),
            })
        }
        "BackendProvided" => {
            let mut name = None;
            let mut base = None;
            if let Some(n) = o.get("name") {
                let s = n.as_str().ok_or_else(|| invalid("isolation.name", None))?;
                name = Some(s.to_string());
            }
            if let Some(b) = o.get("base") {
                let s = b.as_str().ok_or_else(|| invalid("isolation.base", None))?;
                base = Some(s.to_string());
            }
            Ok(crate::types::Isolation::BackendProvided { name, base })
        }
        _ => Err(invalid("isolation.type", None)),
    }
}

pub fn validate_run_input(args: &Value) -> Result<RunInput, String> {
    let obj = args.as_object();
    let prompt = obj.and_then(|o| o.get("prompt")).and_then(|p| p.as_str());
    let prompt = match prompt {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => return Err("missing required field \"prompt\"".into()),
    };
    let mut input = RunInput {
        prompt,
        ..RunInput::default()
    };
    let Some(obj) = obj else {
        return Ok(input);
    };

    if let Some(v) = obj.get("model") {
        let s = v
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("model", None))?;
        input.model = Some(s.to_string());
    }
    if let Some(v) = obj.get("requireNonClaude") {
        let b = v
            .as_bool()
            .ok_or_else(|| invalid("requireNonClaude", Some("must be a boolean")))?;
        input.require_non_claude = Some(b);
    }
    if let Some(v) = obj.get("capability") {
        let s = v.as_str().ok_or_else(|| invalid("capability", None))?;
        let cap = crate::types::Capability::parse(s).ok_or_else(|| invalid("capability", None))?;
        input.capability = Some(cap);
    }
    if let Some(v) = obj.get("allowUnsandboxed") {
        let b = v
            .as_bool()
            .ok_or_else(|| invalid("allowUnsandboxed", Some("must be a boolean")))?;
        input.allow_unsandboxed = Some(b);
    }
    if let Some(v) = obj.get("session") {
        let s = v
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("session", None))?;
        input.session = Some(s.to_string());
    }
    if let Some(v) = obj.get("isolation") {
        input.isolation = Some(parse_isolation(v)?);
    }
    if let Some(v) = obj.get("verifyCommands") {
        let arr = v
            .as_array()
            .ok_or_else(|| invalid("verifyCommands", None))?;
        let cmds: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect();
        if cmds.len() != arr.len() {
            return Err(invalid(
                "verifyCommands",
                Some("must be an array of strings"),
            ));
        }
        input.verify_commands = Some(cmds);
    }
    if let Some(v) = obj.get("gate") {
        let s = v.as_str().ok_or_else(|| invalid("gate", None))?;
        input.gate = Some(s.to_string());
    }
    if let Some(v) = obj.get("allowPartialCommit") {
        let b = v
            .as_bool()
            .ok_or_else(|| invalid("allowPartialCommit", Some("must be a boolean")))?;
        input.allow_partial_commit = Some(b);
    }
    if let Some(v) = obj.get("waitMs") {
        let n = v
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| invalid("waitMs", Some("must be a finite number")))?;
        input.wait_ms = Some(n);
    }
    if let Some(v) = obj.get("idleMs") {
        if v.is_null() {
            input.idle_ms = Some(None);
        } else {
            let n = v
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| invalid("idleMs", Some("must be a finite number or null")))?;
            input.idle_ms = Some(Some(n));
        }
    }
    if let Some(v) = obj.get("toolIdleMs") {
        if v.is_null() {
            input.tool_idle_ms = Some(None);
        } else {
            let n = v
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| invalid("toolIdleMs", Some("must be a finite number or null")))?;
            input.tool_idle_ms = Some(Some(n));
        }
    }
    if let Some(v) = obj.get("background") {
        let b = v
            .as_bool()
            .ok_or_else(|| invalid("background", Some("must be a boolean")))?;
        input.background = Some(b);
    }
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_model_and_require_non_claude() {
        let input = validate_run_input(&json!({
            "prompt": "do it",
            "model": "grok-4.5-xhigh",
            "requireNonClaude": true,
        }))
        .unwrap();
        assert_eq!(input.prompt, "do it");
        assert_eq!(input.model.as_deref(), Some("grok-4.5-xhigh"));
        assert_eq!(input.require_non_claude, Some(true));
    }

    #[test]
    fn rejects_empty_model() {
        let e = validate_run_input(&json!({"prompt": "x", "model": ""})).unwrap_err();
        assert!(e.contains("model"));
    }

    #[test]
    fn rejects_non_boolean_require_non_claude() {
        let e = validate_run_input(&json!({"prompt": "x", "requireNonClaude": "yes"})).unwrap_err();
        assert!(e.contains("requireNonClaude"));
    }

    #[test]
    fn tier_is_silently_ignored() {
        let input = validate_run_input(&json!({"prompt": "x", "tier": "cheap-bulk"})).unwrap();
        assert_eq!(input.prompt, "x");
        assert!(input.model.is_none());
    }

    #[test]
    fn omitted_require_non_claude_stays_none() {
        let input = validate_run_input(&json!({"prompt": "x"})).unwrap();
        assert!(input.require_non_claude.is_none());
    }
}
