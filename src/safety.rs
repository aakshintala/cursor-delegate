use serde::Deserialize;

#[derive(Debug)]
pub struct DenyListError {
    pub message: String,
}

impl std::fmt::Display for DenyListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for DenyListError {}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CliConfig {
    pub permissions: Option<CliPermissions>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CliPermissions {
    pub deny: Option<Vec<String>>,
}

pub fn verify_deny_list(
    required_deny: &[String],
    cli_config: Option<&CliConfig>,
) -> Result<(), DenyListError> {
    if required_deny.is_empty() {
        return Ok(());
    }
    let empty: Vec<String> = Vec::new();
    let deny = cli_config
        .and_then(|c| c.permissions.as_ref())
        .and_then(|p| p.deny.as_ref())
        .unwrap_or(&empty);
    let missing: Vec<&String> = required_deny.iter().filter(|p| !deny.contains(p)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    let joined = missing
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(DenyListError {
        message: format!(
            "cursor-agent deny-list is missing required patterns: {joined}. \
Add them to ~/.cursor/cli-config.json permissions.deny before running write tools."
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_required_deny_always_passes() {
        verify_deny_list(&[], None).unwrap();
        verify_deny_list(
            &[],
            Some(&CliConfig {
                permissions: Some(CliPermissions { deny: Some(vec![]) }),
            }),
        )
        .unwrap();
    }

    #[test]
    fn all_required_patterns_present_passes() {
        verify_deny_list(
            &["rm -rf /".into(), "shutdown".into()],
            Some(&CliConfig {
                permissions: Some(CliPermissions {
                    deny: Some(vec!["rm -rf /".into(), "shutdown".into(), "reboot".into()]),
                }),
            }),
        )
        .unwrap();
    }

    #[test]
    fn missing_pattern_throws() {
        let e = verify_deny_list(
            &["rm -rf /".into(), "shutdown".into()],
            Some(&CliConfig {
                permissions: Some(CliPermissions {
                    deny: Some(vec!["rm -rf /".into()]),
                }),
            }),
        )
        .unwrap_err();
        let _ = e;
    }

    #[test]
    fn null_cli_config_with_requirement_throws() {
        assert!(verify_deny_list(&["rm -rf /".into()], None).is_err());
    }
}
