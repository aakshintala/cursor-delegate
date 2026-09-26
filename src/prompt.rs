const SEP: &str = "\n\n---\n\n";

pub fn verify_block(verify_commands: Option<&[String]>) -> Option<String> {
    let cmds = verify_commands.filter(|c| !c.is_empty())?;
    if cmds.is_empty() {
        return None;
    }
    let list = cmds
        .iter()
        .map(|c| format!("`{c}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "These are the ONLY verification commands you may run: {list}. \
Do not run workspace-wide builds (e.g. `cargo check --workspace`, full test suites) \
or any other build/test command."
    ))
}

pub fn status_block() -> String {
    "End your final message with a single trailing line that is exactly one of: \
STATUS: DONE, STATUS: DONE_WITH_CONCERNS, STATUS: BLOCKED, STATUS: NEEDS_CONTEXT, or STATUS: ERROR. \
When you need an answer from the orchestrator before you can proceed, put your question in the \
message body and end with STATUS: NEEDS_CONTEXT."
        .to_string()
}

pub fn compose_prompt(
    preamble: Option<&str>,
    verify_commands: Option<&[String]>,
    prompt: &str,
) -> String {
    let vb = verify_block(verify_commands);
    let mut parts: Vec<&str> = Vec::new();
    if let Some(p) = preamble.filter(|s| !s.is_empty()) {
        parts.push(p);
    }
    if let Some(v) = vb.as_deref() {
        parts.push(v);
    }
    let sb = status_block();
    parts.push(&sb);
    if !prompt.is_empty() {
        parts.push(prompt);
    }
    parts.join(SEP).replace('\0', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_block_null_with_no_commands() {
        assert!(verify_block(None).is_none());
        assert!(verify_block(Some(&[])).is_none());
    }

    #[test]
    fn verify_block_lists_commands() {
        let b = verify_block(Some(&["cargo test".into(), "cargo check".into()])).unwrap();
        assert!(b.contains("ONLY verification commands"));
        assert!(b.contains("`cargo test`, `cargo check`"));
    }

    #[test]
    fn status_block_instructs_needs_context() {
        let b = status_block();
        assert!(b.contains("STATUS: NEEDS_CONTEXT"));
        assert!(b.contains("STATUS: DONE"));
        assert!(b.to_lowercase().contains("question"));
    }

    #[test]
    fn compose_always_includes_status() {
        let out = compose_prompt(None, None, "do the thing");
        let parts: Vec<&str> = out.split("\n\n---\n\n").collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("STATUS: NEEDS_CONTEXT"));
        assert_eq!(parts[1], "do the thing");
    }

    #[test]
    fn compose_joins_preamble_verify_status_prompt() {
        let out = compose_prompt(Some("STANDING"), Some(&["x test".into()]), "do the thing");
        let parts: Vec<&str> = out.split("\n\n---\n\n").collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "STANDING");
        assert!(parts[1].contains("ONLY verification"));
        assert!(parts[2].contains("STATUS: NEEDS_CONTEXT"));
        assert_eq!(parts[3], "do the thing");
    }

    #[test]
    fn compose_omits_empty_preamble() {
        let out = compose_prompt(None, None, "hi");
        let parts: Vec<&str> = out.split("\n\n---\n\n").collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("STATUS:"));
        assert_eq!(parts[1], "hi");
    }

    #[test]
    fn compose_strips_nul() {
        let out = compose_prompt(None, None, "a\0b\0c");
        let parts: Vec<&str> = out.split("\n\n---\n\n").collect();
        assert_eq!(parts[parts.len() - 1], "abc");
        assert!(!out.contains('\0'));
    }
}
