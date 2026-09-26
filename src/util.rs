use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Cancellation flag shared between a request and the work it started.
pub type Abort = Arc<AtomicBool>;

/// Serialize an f64 the way `JSON.stringify` does: integral values without a trailing `.0`.
pub fn js_num<S: serde::Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 9.007_199_254_740_992e15 {
        s.serialize_i64(*v as i64)
    } else if v.is_finite() {
        s.serialize_f64(*v)
    } else {
        s.serialize_none()
    }
}

pub fn js_num_opt<S: serde::Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    match v {
        Some(n) => js_num(n, s),
        None => s.serialize_none(),
    }
}

pub fn tail(s: &str, max_bytes: usize) -> String {
    let buf = s.as_bytes();
    if buf.len() <= max_bytes {
        return s.to_string();
    }
    let slice = &buf[buf.len() - max_bytes..];
    String::from_utf8_lossy(slice).into_owned()
}

pub fn clamp_wait(ms: f64) -> f64 {
    ms.clamp(1000.0, 600_000.0)
}

pub fn random_uuid() -> String {
    let mut bytes = [0u8; 16];
    let rc = unsafe { libc::getentropy(bytes.as_mut_ptr() as *mut libc::c_void, 16) };
    if rc != 0 {
        let mut f = std::fs::File::open("/dev/urandom").expect("urandom");
        f.read_exact(&mut bytes).expect("urandom read");
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

pub fn homedir() -> std::path::PathBuf {
    if let Ok(h) = std::env::var("HOME") {
        return std::path::PathBuf::from(h);
    }
    std::path::PathBuf::from("/")
}

pub fn json_pretty(value: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_string())
}

pub fn json_compact(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

pub fn is_finite_number(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

/// Node `path.resolve` without requiring the path to exist.
pub fn resolve_path(p: &str) -> String {
    let path = if std::path::Path::new(p).is_absolute() {
        std::path::PathBuf::from(p)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(p)
    };
    normalize_path(&path)
}

pub fn normalize_path(path: &std::path::Path) -> String {
    use std::path::{Component, PathBuf};
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(pref) => out.push(pref.as_os_str()),
            Component::RootDir => out.push(std::path::MAIN_SEPARATOR_STR),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(c) => out.push(c),
        }
    }
    if out.as_os_str().is_empty() {
        return std::path::MAIN_SEPARATOR_STR.to_string();
    }
    out.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_short_strings() {
        assert_eq!(tail("hi", 2048), "hi");
    }

    #[test]
    fn js_num_matches_json_stringify() {
        #[derive(serde::Serialize)]
        struct T {
            #[serde(serialize_with = "js_num")]
            a: f64,
            #[serde(serialize_with = "js_num")]
            b: f64,
            #[serde(serialize_with = "js_num_opt")]
            c: Option<f64>,
        }
        let j = serde_json::to_string(&T {
            a: 12.0,
            b: 0.0123,
            c: Some(3.0),
        })
        .unwrap();
        assert_eq!(j, r#"{"a":12,"b":0.0123,"c":3}"#);
    }

    #[test]
    fn clamp_wait_window() {
        assert_eq!(clamp_wait(0.0), 1000.0);
        assert_eq!(clamp_wait(999_999.0), 600_000.0);
        assert_eq!(clamp_wait(5000.0), 5000.0);
    }
}
