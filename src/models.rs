use crate::types::{Config, ResolvedModel};

#[derive(Debug)]
pub struct ModelNotAllowedError {
    pub message: String,
}

impl std::fmt::Display for ModelNotAllowedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ModelNotAllowedError {}

#[derive(Debug)]
pub struct NonClaudeViolationError {
    pub message: String,
}

impl std::fmt::Display for NonClaudeViolationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for NonClaudeViolationError {}

pub fn resolve_model(
    model: Option<&str>,
    require_non_claude: bool,
    config: &impl ConfigLike,
) -> Result<ResolvedModel, Box<dyn std::error::Error + Send + Sync>> {
    let model = model
        .map(|s| s.to_string())
        .unwrap_or_else(|| config.default_model().to_string());
    let entry = config.models().get(&model).ok_or_else(|| {
        Box::new(ModelNotAllowedError {
            message: format!("model \"{model}\" is not in the allow-list"),
        }) as Box<dyn std::error::Error + Send + Sync>
    })?;
    if require_non_claude && entry.family == "claude" {
        return Err(Box::new(NonClaudeViolationError {
            message: format!("requireNonClaude is set but model \"{model}\" has family \"claude\""),
        }));
    }
    Ok(ResolvedModel {
        model,
        family: entry.family.clone(),
        price: entry.price,
    })
}

pub trait ConfigLike {
    fn default_model(&self) -> &str;
    fn models(&self) -> &std::collections::HashMap<String, crate::types::ModelEntry>;
}

impl ConfigLike for Config {
    fn default_model(&self) -> &str {
        &self.default
    }
    fn models(&self) -> &std::collections::HashMap<String, crate::types::ModelEntry> {
        &self.models
    }
}

impl ConfigLike
    for (
        &str,
        &std::collections::HashMap<String, crate::types::ModelEntry>,
    )
{
    fn default_model(&self) -> &str {
        self.0
    }
    fn models(&self) -> &std::collections::HashMap<String, crate::types::ModelEntry> {
        self.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelEntry, Price};
    use std::collections::HashMap;

    fn price(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Price {
        Price {
            input,
            output,
            cache_read,
            cache_write,
        }
    }

    fn base() -> (String, HashMap<String, ModelEntry>) {
        let mut models = HashMap::new();
        models.insert(
            "composer-2.5".into(),
            ModelEntry {
                label: "Composer 2.5".into(),
                family: "composer".into(),
                price: price(0.5, 2.5, 0.2, 0.0),
            },
        );
        models.insert(
            "grok-4.5-xhigh".into(),
            ModelEntry {
                label: "Grok 4.5".into(),
                family: "grok".into(),
                price: price(2.0, 6.0, 0.5, 0.0),
            },
        );
        models.insert(
            "claude-sonnet-4".into(),
            ModelEntry {
                label: "Claude Sonnet 4".into(),
                family: "claude".into(),
                price: price(3.0, 15.0, 0.3, 0.0),
            },
        );
        ("composer-2.5".into(), models)
    }

    #[test]
    fn omitted_model_resolves_to_default() {
        let (d, m) = base();
        let r = resolve_model(None, false, &(d.as_str(), &m)).unwrap();
        assert_eq!(r.model, "composer-2.5");
        assert_eq!(r.family, "composer");
        assert_eq!(r.price, m["composer-2.5"].price);
    }

    #[test]
    fn allowed_id_resolves_with_family_and_price() {
        let (d, m) = base();
        let r = resolve_model(Some("grok-4.5-xhigh"), false, &(d.as_str(), &m)).unwrap();
        assert_eq!(r.model, "grok-4.5-xhigh");
        assert_eq!(r.family, "grok");
        assert_eq!(r.price, m["grok-4.5-xhigh"].price);
    }

    #[test]
    fn unknown_id_throws_model_not_allowed() {
        let (d, m) = base();
        let e = resolve_model(Some("not-listed"), false, &(d.as_str(), &m)).unwrap_err();
        assert!(e.downcast_ref::<ModelNotAllowedError>().is_some());
    }

    #[test]
    fn require_non_claude_rejects_explicit_claude() {
        let (d, m) = base();
        let e = resolve_model(Some("claude-sonnet-4"), true, &(d.as_str(), &m)).unwrap_err();
        assert!(e.downcast_ref::<NonClaudeViolationError>().is_some());
    }

    #[test]
    fn require_non_claude_rejects_claude_default() {
        let (_, m) = base();
        let e = resolve_model(None, true, &("claude-sonnet-4", &m)).unwrap_err();
        assert!(e.downcast_ref::<NonClaudeViolationError>().is_some());
    }

    #[test]
    fn require_non_claude_passes_non_claude() {
        let (d, m) = base();
        let r = resolve_model(Some("grok-4.5-xhigh"), true, &(d.as_str(), &m)).unwrap();
        assert_eq!(r.model, "grok-4.5-xhigh");
        assert_eq!(r.family, "grok");
    }

    #[test]
    fn require_non_claude_false_allows_claude() {
        let (d, m) = base();
        let r = resolve_model(Some("claude-sonnet-4"), false, &(d.as_str(), &m)).unwrap();
        assert_eq!(r.model, "claude-sonnet-4");
        assert_eq!(r.family, "claude");
    }
}
