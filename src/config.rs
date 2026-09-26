use crate::safety::CliConfig;
use crate::types::{Config, HostProfile, ModelEntry, Price, PriceMap};
use serde_json::Value;
use std::collections::HashMap;

pub const BUNDLED_MODELS_JSON: &str = include_str!("../config/models.json");

pub type ReadFileFn = Box<dyn Fn(&str) -> Result<String, IoErr> + Send + Sync>;

#[derive(Debug)]
pub struct IoErr {
    pub code: Option<String>,
    pub message: String,
}

impl std::fmt::Display for IoErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for IoErr {}

fn default_read_file(path: &str) -> Result<String, IoErr> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(IoErr {
            code: Some("ENOENT".into()),
            message: e.to_string(),
        }),
        Err(e) => Err(IoErr {
            code: None,
            message: e.to_string(),
        }),
    }
}

fn read_json(
    read_file: &dyn Fn(&str) -> Result<String, IoErr>,
    path: &str,
) -> Result<Option<Value>, Box<dyn std::error::Error + Send + Sync>> {
    match read_file(path) {
        Ok(s) => Ok(Some(serde_json::from_str(&s)?)),
        Err(e) if e.code.as_deref() == Some("ENOENT") => Ok(None),
        Err(e) => Err(Box::new(e)),
    }
}

fn is_record(v: &Value) -> bool {
    v.is_object()
}

fn str_field(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    where_: &str,
) -> Result<String, String> {
    obj.get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("invalid config: {where_}.{field} must be a string"))
}

fn opt_str(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    where_: &str,
) -> Result<Option<String>, String> {
    match obj.get(field) {
        None => Ok(None),
        Some(v) => v
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| format!("invalid config: {where_}.{field} must be a string")),
    }
}

fn opt_str_array(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    where_: &str,
) -> Result<Option<Vec<String>>, String> {
    match obj.get(field) {
        None => Ok(None),
        Some(v) => {
            let arr = v.as_array().ok_or_else(|| {
                format!("invalid config: {where_}.{field} must be an array of strings")
            })?;
            if arr.iter().any(|x| !x.is_string()) {
                return Err(format!(
                    "invalid config: {where_}.{field} must be an array of strings"
                ));
            }
            Ok(Some(
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect(),
            ))
        }
    }
}

fn opt_finite(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    where_: &str,
) -> Result<Option<f64>, String> {
    match obj.get(field) {
        None => Ok(None),
        Some(v) => {
            let n = v
                .as_f64()
                .filter(|n| n.is_finite() && *n > 0.0)
                .ok_or_else(|| {
                    format!("invalid config: {where_}.{field} must be a positive finite number")
                })?;
            Ok(Some(n))
        }
    }
}

fn opt_finite_or_null(
    obj: &serde_json::Map<String, Value>,
    field: &str,
    where_: &str,
) -> Result<Option<Option<f64>>, String> {
    match obj.get(field) {
        None => Ok(None),
        Some(v) if v.is_null() => Ok(Some(None)),
        Some(_) => Ok(Some(opt_finite(obj, field, where_)?)),
    }
}

fn decode_price(raw: &Value, where_: &str) -> Result<Price, String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| format!("invalid config: {where_}.price must be an object"))?;
    let mut out = Price {
        input: 0.0,
        output: 0.0,
        cache_read: 0.0,
        cache_write: 0.0,
    };
    for (k, slot) in [
        ("input", &mut out.input),
        ("output", &mut out.output),
        ("cacheRead", &mut out.cache_read),
        ("cacheWrite", &mut out.cache_write),
    ] {
        let v = obj.get(k).and_then(|x| x.as_f64());
        match v {
            Some(n) if n.is_finite() && n >= 0.0 => *slot = n,
            _ => {
                return Err(format!(
                    "invalid config: {where_}.price.{k} must be a non-negative finite number"
                ));
            }
        }
    }
    Ok(out)
}

fn decode_model_entry(raw: &Value, id: &str, where_: &str) -> Result<ModelEntry, String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| format!("invalid config: {where_}[\"{id}\"] must be an object"))?;
    let loc = format!("{where_}[\"{id}\"]");
    Ok(ModelEntry {
        label: str_field(obj, "label", &loc)?,
        family: str_field(obj, "family", &loc)?,
        price: decode_price(obj.get("price").unwrap_or(&Value::Null), &loc)?,
    })
}

fn decode_models(raw: &Value, where_: &str) -> Result<HashMap<String, ModelEntry>, String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| format!("invalid config: {where_} must be an object"))?;
    let mut out = HashMap::new();
    for (id, entry) in obj {
        out.insert(id.clone(), decode_model_entry(entry, id, where_)?);
    }
    Ok(out)
}

fn decode_host_profile(raw: Option<&Value>) -> Result<HostProfile, String> {
    let Some(raw) = raw else {
        return Ok(HostProfile::default());
    };
    if raw.is_null() {
        return Ok(HostProfile::default());
    }
    let obj = raw
        .as_object()
        .ok_or_else(|| "invalid config: host profile must be an object".to_string())?;
    let mut profile = HostProfile::default();
    if let Some(d) = opt_str(obj, "default", "host-profile")? {
        profile.default = Some(d);
    }
    if let Some(models) = obj.get("models") {
        profile.models = Some(decode_models(models, "host-profile.models")?);
    }
    if let Some(deny) = opt_str_array(obj, "requiredDeny", "host-profile")? {
        profile.required_deny = Some(deny);
    }
    if let Some(p) = opt_str(obj, "promptPreamble", "host-profile")? {
        profile.prompt_preamble = Some(p);
    }
    if let Some(v) = opt_str_array(obj, "verifyCommands", "host-profile")? {
        profile.verify_commands = Some(v);
    }
    if let Some(g) = opt_str(obj, "gate", "host-profile")? {
        profile.gate = Some(g);
    }
    if let Some(d) = opt_finite(obj, "deadlineMs", "host-profile")? {
        profile.deadline_ms = Some(d);
    }
    if let Some(i) = opt_finite_or_null(obj, "idleMs", "host-profile")? {
        profile.idle_ms = Some(i);
    }
    if let Some(i) = opt_finite_or_null(obj, "toolIdleMs", "host-profile")? {
        profile.tool_idle_ms = Some(i);
    }
    Ok(profile)
}

pub fn default_host_profile_path() -> String {
    crate::util::homedir()
        .join(".config/cursor-delegate/host-profile.json")
        .to_string_lossy()
        .into_owned()
}

pub fn default_cli_config_path() -> String {
    crate::util::homedir()
        .join(".cursor/cli-config.json")
        .to_string_lossy()
        .into_owned()
}

pub struct LoadConfigOpts {
    pub models_path: String,
    pub host_profile_path: Option<String>,
    pub read_file: Option<ReadFileFn>,
}

fn to_price_map(models: &HashMap<String, ModelEntry>) -> PriceMap {
    models.iter().map(|(id, e)| (id.clone(), e.price)).collect()
}

pub fn load_config(
    opts: LoadConfigOpts,
) -> Result<Config, Box<dyn std::error::Error + Send + Sync>> {
    let read_owned = opts.read_file;
    let read_fn = |path: &str| -> Result<String, IoErr> {
        if let Some(rf) = &read_owned {
            rf(path)
        } else {
            default_read_file(path)
        }
    };
    let profile_path = opts.host_profile_path.clone().unwrap_or_else(|| {
        std::env::var("CURSOR_DELEGATE_HOST_PROFILE")
            .unwrap_or_else(|_| default_host_profile_path())
    });

    let file_raw = read_json(&read_fn, &opts.models_path)?;
    let profile_raw = read_json(&read_fn, &profile_path)?;

    let file_raw =
        file_raw.ok_or_else(|| format!("invalid or missing models file: {}", opts.models_path))?;
    if !is_record(&file_raw) || !file_raw.get("default").and_then(|d| d.as_str()).is_some() {
        return Err(format!("invalid or missing models file: {}", opts.models_path).into());
    }
    let file_models = decode_models(
        file_raw.get("models").unwrap_or(&Value::Null),
        "models.json.models",
    )?;
    let profile = decode_host_profile(profile_raw.as_ref())?;

    let mut models = file_models;
    if let Some(pm) = &profile.models {
        for (k, v) in pm {
            models.insert(k.clone(), v.clone());
        }
    }
    let default_model = profile
        .default
        .clone()
        .unwrap_or_else(|| file_raw["default"].as_str().unwrap().to_string());
    if !models.contains_key(&default_model) {
        return Err(
            format!("default model \"{default_model}\" is not present in the models map").into(),
        );
    }
    Ok(Config {
        price_map: to_price_map(&models),
        default: default_model,
        models,
        profile,
    })
}

pub fn load_cli_config(
    path: &str,
    read_file: Option<&dyn Fn(&str) -> Result<String, IoErr>>,
) -> Result<Option<CliConfig>, Box<dyn std::error::Error + Send + Sync>> {
    let fallback = |p: &str| default_read_file(p);
    let rf: &dyn Fn(&str) -> Result<String, IoErr> = match read_file {
        Some(f) => f,
        None => &fallback,
    };
    let raw = match read_json(rf, path)? {
        None => return Ok(None),
        Some(v) => v,
    };
    if !is_record(&raw) {
        return Err(format!("invalid config: {path} must be an object").into());
    }
    if let Some(perms) = raw.get("permissions") {
        if !perms.is_null() && !is_record(perms) {
            return Err(format!("invalid config: {path}.permissions must be an object").into());
        }
        if let Some(deny) = perms.get("deny")
            && !deny.is_null()
        {
            let arr = deny.as_array();
            if arr.is_none() || arr.unwrap().iter().any(|x| !x.is_string()) {
                return Err(format!(
                    "invalid config: {path}.permissions.deny must be an array of strings"
                )
                .into());
            }
        }
    }
    Ok(Some(serde_json::from_value(raw)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    const MODELS_DEFAULT: &str = r#"{
  "default": "composer-2.5",
  "models": {
    "composer-2.5": {
      "label": "Composer 2.5",
      "family": "composer",
      "price": { "input": 0.5, "output": 2.5, "cacheRead": 0.2, "cacheWrite": 0 }
    },
    "grok-4.5-xhigh": {
      "label": "Grok 4.5",
      "family": "grok",
      "price": { "input": 2, "output": 6, "cacheRead": 0.5, "cacheWrite": 0 }
    }
  }
}"#;

    fn reader(files: HashMap<String, String>) -> ReadFileFn {
        let files = Arc::new(files);
        Box::new(move |path: &str| {
            files.get(path).cloned().ok_or_else(|| IoErr {
                code: Some("ENOENT".into()),
                message: "ENOENT".into(),
            })
        })
    }

    #[test]
    fn loads_models_and_derives_price_map() {
        let mut files = HashMap::new();
        files.insert("/models.json".into(), MODELS_DEFAULT.into());
        let cfg = load_config(LoadConfigOpts {
            models_path: "/models.json".into(),
            host_profile_path: Some("/does-not-exist.json".into()),
            read_file: Some(reader(files)),
        })
        .unwrap();
        assert_eq!(cfg.default, "composer-2.5");
        assert_eq!(cfg.models["composer-2.5"].family, "composer");
        assert_eq!(cfg.price_map["composer-2.5"].input, 0.5);
        assert_eq!(cfg.profile, HostProfile::default());
    }

    #[test]
    fn merges_host_profile() {
        let mut files = HashMap::new();
        files.insert("/models.json".into(), MODELS_DEFAULT.into());
        files.insert(
            "/profile.json".into(),
            serde_json::json!({
                "default": "grok-4.5-xhigh",
                "models": {
                    "gpt-5.5-high": {
                        "label": "GPT-5.5 1M High",
                        "family": "gpt",
                        "price": { "input": 5, "output": 30, "cacheRead": 0.5, "cacheWrite": 0 }
                    }
                },
                "requiredDeny": ["rm -rf /"],
                "deadlineMs": 5000
            })
            .to_string(),
        );
        let cfg = load_config(LoadConfigOpts {
            models_path: "/models.json".into(),
            host_profile_path: Some("/profile.json".into()),
            read_file: Some(reader(files)),
        })
        .unwrap();
        assert_eq!(cfg.default, "grok-4.5-xhigh");
        assert!(cfg.models.contains_key("composer-2.5"));
        assert!(cfg.models.contains_key("gpt-5.5-high"));
        assert_eq!(cfg.models["gpt-5.5-high"].family, "gpt");
        assert_eq!(cfg.price_map["gpt-5.5-high"].output, 30.0);
        assert_eq!(
            cfg.profile.required_deny.as_ref().unwrap(),
            &["rm -rf /".to_string()]
        );
        assert_eq!(cfg.profile.deadline_ms, Some(5000.0));
    }

    #[test]
    fn missing_host_profile_is_empty() {
        let mut files = HashMap::new();
        files.insert("/models.json".into(), MODELS_DEFAULT.into());
        let cfg = load_config(LoadConfigOpts {
            models_path: "/models.json".into(),
            host_profile_path: Some("/does-not-exist.json".into()),
            read_file: Some(reader(files)),
        })
        .unwrap();
        assert_eq!(cfg.profile, HostProfile::default());
        assert_eq!(cfg.default, "composer-2.5");
        assert!(cfg.price_map.contains_key("composer-2.5"));
    }

    #[test]
    fn throws_when_default_absent() {
        let mut files = HashMap::new();
        files.insert("/models.json".into(), MODELS_DEFAULT.into());
        files.insert(
            "/profile.json".into(),
            serde_json::json!({"default":"not-a-real-model"}).to_string(),
        );
        let e = load_config(LoadConfigOpts {
            models_path: "/models.json".into(),
            host_profile_path: Some("/profile.json".into()),
            read_file: Some(reader(files)),
        })
        .unwrap_err();
        assert!(e.to_string().contains("default"));
    }

    #[test]
    fn rejects_non_finite_price() {
        let bad = serde_json::json!({
            "default":"m",
            "models":{"m":{"label":"M","family":"f","price":{"input":null,"output":1,"cacheRead":0,"cacheWrite":0}}}
        })
        .to_string();
        // NaN is not valid JSON; use a missing-like invalid (string) which fails the number check.
        let bad = bad.replace("null", "\"nan\"");
        let mut files = HashMap::new();
        files.insert("/m.json".into(), bad);
        let e = load_config(LoadConfigOpts {
            models_path: "/m.json".into(),
            host_profile_path: Some("/x.json".into()),
            read_file: Some(reader(files)),
        })
        .unwrap_err();
        assert!(e.to_string().contains("price.input"));
    }

    #[test]
    fn rejects_missing_price_field() {
        let bad = serde_json::json!({
            "default":"m",
            "models":{"m":{"label":"M","family":"f","price":{"input":1,"output":1,"cacheRead":0}}}
        })
        .to_string();
        let mut files = HashMap::new();
        files.insert("/m.json".into(), bad);
        let e = load_config(LoadConfigOpts {
            models_path: "/m.json".into(),
            host_profile_path: Some("/x.json".into()),
            read_file: Some(reader(files)),
        })
        .unwrap_err();
        assert!(e.to_string().contains("price.cacheWrite"));
    }

    #[test]
    fn rejects_bad_deadline() {
        let mut files = HashMap::new();
        files.insert("/m.json".into(), MODELS_DEFAULT.into());
        files.insert(
            "/p.json".into(),
            serde_json::json!({"deadlineMs":"soon"}).to_string(),
        );
        let e = load_config(LoadConfigOpts {
            models_path: "/m.json".into(),
            host_profile_path: Some("/p.json".into()),
            read_file: Some(reader(files)),
        })
        .unwrap_err();
        assert!(e.to_string().contains("deadlineMs"));
    }

    #[test]
    fn rejects_bad_required_deny() {
        let mut files = HashMap::new();
        files.insert("/m.json".into(), MODELS_DEFAULT.into());
        files.insert(
            "/p.json".into(),
            serde_json::json!({"requiredDeny":[1,2]}).to_string(),
        );
        let e = load_config(LoadConfigOpts {
            models_path: "/m.json".into(),
            host_profile_path: Some("/p.json".into()),
            read_file: Some(reader(files)),
        })
        .unwrap_err();
        assert!(e.to_string().contains("requiredDeny"));
    }

    #[test]
    fn accepts_null_idle() {
        let mut files = HashMap::new();
        files.insert("/m.json".into(), MODELS_DEFAULT.into());
        files.insert(
            "/p.json".into(),
            serde_json::json!({"idleMs":null,"toolIdleMs":null,"deadlineMs":1000}).to_string(),
        );
        let cfg = load_config(LoadConfigOpts {
            models_path: "/m.json".into(),
            host_profile_path: Some("/p.json".into()),
            read_file: Some(reader(files)),
        })
        .unwrap();
        assert_eq!(cfg.profile.idle_ms, Some(None));
        assert_eq!(cfg.profile.tool_idle_ms, Some(None));
    }
}
