use crate::backends::Backend;
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

pub fn resolve_model(
    model: Option<&str>,
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
    if Backend::from_name(&entry.backend).is_none() {
        return Err(format!(
            "model \"{model}\" uses backend \"{}\", which is not implemented yet",
            entry.backend
        )
        .into());
    }
    Ok(ResolvedModel {
        model,
        backend: entry.backend.clone(),
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
                backend: "cursor".into(),
                price: price(0.5, 2.5, 0.2, 0.0),
            },
        );
        models.insert(
            "grok-4.7-high".into(),
            ModelEntry {
                label: "Grok 4.7 High".into(),
                backend: "cursor".into(),
                price: price(2.0, 6.0, 0.5, 0.0),
            },
        );
        models.insert(
            "openai-codex/gpt-6-luna".into(),
            ModelEntry {
                label: "GPT-6 Luna".into(),
                backend: "pi".into(),
                price: price(0.1, 0.5, 0.01, 0.125),
            },
        );
        models.insert(
            "claude-sonnet-5-5".into(),
            ModelEntry {
                label: "Claude Sonnet 5.5".into(),
                backend: "claude".into(),
                price: price(2.0, 10.0, 0.2, 2.5),
            },
        );
        ("composer-2.5".into(), models)
    }

    #[test]
    fn omitted_model_resolves_to_default() {
        let (d, m) = base();
        let r = resolve_model(None, &(d.as_str(), &m)).unwrap();
        assert_eq!(r.model, "composer-2.5");
        assert_eq!(r.backend, "cursor");
        assert_eq!(r.price, m["composer-2.5"].price);
    }

    #[test]
    fn allowed_id_resolves_with_backend_and_price() {
        let (d, m) = base();
        let r = resolve_model(Some("grok-4.7-high"), &(d.as_str(), &m)).unwrap();
        assert_eq!(r.model, "grok-4.7-high");
        assert_eq!(r.backend, "cursor");
        assert_eq!(r.price, m["grok-4.7-high"].price);
    }

    #[test]
    fn unknown_id_throws_model_not_allowed() {
        let (d, m) = base();
        let e = resolve_model(Some("not-listed"), &(d.as_str(), &m)).unwrap_err();
        assert!(e.downcast_ref::<ModelNotAllowedError>().is_some());
    }

    #[test]
    fn claude_backend_resolves() {
        let (d, m) = base();
        let r = resolve_model(Some("claude-sonnet-5-5"), &(d.as_str(), &m)).unwrap();
        assert_eq!(r.model, "claude-sonnet-5-5");
        assert_eq!(r.backend, "claude");
        assert_eq!(r.price, m["claude-sonnet-5-5"].price);
    }

    #[test]
    fn pi_backend_resolves() {
        let (d, m) = base();
        let r = resolve_model(Some("openai-codex/gpt-6-luna"), &(d.as_str(), &m)).unwrap();
        assert_eq!(r.model, "openai-codex/gpt-6-luna");
        assert_eq!(r.backend, "pi");
        assert_eq!(r.price, m["openai-codex/gpt-6-luna"].price);
    }
}
