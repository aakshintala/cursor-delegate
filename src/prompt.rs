pub fn status_block() -> String {
    "End your final message with a single trailing line that is exactly one of: \
STATUS: DONE, STATUS: DONE_WITH_CONCERNS, STATUS: BLOCKED, STATUS: NEEDS_CONTEXT, or STATUS: ERROR. \
When you need an answer from the orchestrator before you can proceed, put your question in the \
message body and end with STATUS: NEEDS_CONTEXT."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_block_instructs_needs_context() {
        let b = status_block();
        assert!(b.contains("STATUS: NEEDS_CONTEXT"));
        assert!(b.contains("STATUS: DONE"));
        assert!(b.to_lowercase().contains("question"));
    }
}
