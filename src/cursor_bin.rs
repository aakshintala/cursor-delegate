use std::process::Command;

pub fn resolve_cursor_bin(r#override: Option<&str>) -> String {
    if let Some(o) = r#override.filter(|s| !s.is_empty()) {
        return o.to_string();
    }
    if let Ok(env) = std::env::var("CURSOR_AGENT_BIN")
        && !env.is_empty()
    {
        return env;
    }
    if let Ok(out) = Command::new("which").arg("cursor-agent").output()
        && out.status.success()
    {
        let found = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !found.is_empty() {
            return found;
        }
    }
    crate::util::homedir()
        .join(".local/bin/cursor-agent")
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_override_wins() {
        assert_eq!(
            resolve_cursor_bin(Some("/custom/cursor-agent")),
            "/custom/cursor-agent"
        );
    }

    #[test]
    fn env_used_when_no_override() {
        let prev = std::env::var("CURSOR_AGENT_BIN").ok();
        unsafe { std::env::set_var("CURSOR_AGENT_BIN", "/env/cursor-agent") };
        assert_eq!(resolve_cursor_bin(None), "/env/cursor-agent");
        match prev {
            Some(v) => unsafe { std::env::set_var("CURSOR_AGENT_BIN", v) },
            None => unsafe { std::env::remove_var("CURSOR_AGENT_BIN") },
        }
    }
}
