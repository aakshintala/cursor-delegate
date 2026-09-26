use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub last_tool: Option<String>,
    pub tokens_so_far: f64,
    pub elapsed_ms: f64,
    pub phase: Option<String>,
    pub job_tag: Option<String>,
}

pub type ProgressSink = Arc<dyn Fn(ProgressUpdate) + Send + Sync>;

pub fn format_progress(u: &ProgressUpdate) -> String {
    let tag = u
        .job_tag
        .as_ref()
        .map(|t| format!("{t} "))
        .unwrap_or_default();
    let label = u.last_tool.as_deref().unwrap_or("thinking");
    let sec = (u.elapsed_ms / 1000.0).round() as i64;
    format!("{tag}{label} · {} tok · {sec}s", u.tokens_so_far)
}

pub struct McpExtra {
    pub progress_token: Option<serde_json::Value>,
    pub send_notification: Option<Arc<dyn Fn(serde_json::Value) + Send + Sync>>,
}

pub fn progress_sink_from(
    extra: Option<&McpExtra>,
    now: impl Fn() -> f64 + Send + Sync + 'static,
) -> Option<ProgressSink> {
    let extra = extra?;
    let token = extra.progress_token.clone()?;
    if token.is_null() {
        return None;
    }
    let send = extra.send_notification.clone()?;
    let last_emit = Mutex::new(0.0_f64);
    let last_tool: Mutex<Option<Option<String>>> = Mutex::new(None);
    let seq = Mutex::new(0_u64);
    Some(Arc::new(move |u: ProgressUpdate| {
        let t = now();
        let mut lt = last_tool.lock().expect("progress");
        let tool_changed = match &*lt {
            None => true,
            Some(prev) => prev != &u.last_tool,
        };
        let mut le = last_emit.lock().expect("progress");
        if !tool_changed && t - *le < 1000.0 {
            return;
        }
        *lt = Some(u.last_tool.clone());
        *le = t;
        let mut s = seq.lock().expect("progress");
        *s += 1;
        let n = *s;
        drop(s);
        drop(le);
        drop(lt);
        send(serde_json::json!({
            "method": "notifications/progress",
            "params": {
                "progressToken": token,
                "progress": n,
                "message": format_progress(&u),
            }
        }));
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn format_with_tool_and_tag() {
        assert_eq!(
            format_progress(&ProgressUpdate {
                last_tool: Some("shell".into()),
                tokens_so_far: 12.0,
                elapsed_ms: 3400.0,
                phase: None,
                job_tag: Some("abc123".into()),
            }),
            "abc123 shell · 12 tok · 3s"
        );
    }

    #[test]
    fn format_falls_back_to_thinking() {
        assert_eq!(
            format_progress(&ProgressUpdate {
                last_tool: None,
                tokens_so_far: 0.0,
                elapsed_ms: 0.0,
                phase: None,
                job_tag: None,
            }),
            "thinking · 0 tok · 0s"
        );
    }

    #[test]
    fn sink_undefined_without_token() {
        assert!(
            progress_sink_from(
                Some(&McpExtra {
                    progress_token: None,
                    send_notification: None,
                }),
                || 0.0
            )
            .is_none()
        );
        assert!(
            progress_sink_from(
                Some(&McpExtra {
                    progress_token: None,
                    send_notification: Some(Arc::new(|_| {})),
                }),
                || 0.0
            )
            .is_none()
        );
    }

    #[test]
    fn sink_throttles_token_only_updates() {
        let sent: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(vec![]));
        let sent2 = Arc::clone(&sent);
        let now = Arc::new(Mutex::new(0.0_f64));
        let now2 = Arc::clone(&now);
        let sink = progress_sink_from(
            Some(&McpExtra {
                progress_token: Some(serde_json::json!("tok-1")),
                send_notification: Some(Arc::new(move |n| {
                    sent2.lock().unwrap().push(n["params"].clone());
                })),
            }),
            move || *now2.lock().unwrap(),
        )
        .unwrap();

        sink(ProgressUpdate {
            last_tool: Some("shell".into()),
            tokens_so_far: 1.0,
            elapsed_ms: 0.0,
            phase: None,
            job_tag: None,
        });
        assert_eq!(sent.lock().unwrap().len(), 1);

        *now.lock().unwrap() = 500.0;
        sink(ProgressUpdate {
            last_tool: Some("shell".into()),
            tokens_so_far: 2.0,
            elapsed_ms: 500.0,
            phase: None,
            job_tag: None,
        });
        assert_eq!(sent.lock().unwrap().len(), 1);

        *now.lock().unwrap() = 600.0;
        sink(ProgressUpdate {
            last_tool: Some("edit".into()),
            tokens_so_far: 3.0,
            elapsed_ms: 600.0,
            phase: None,
            job_tag: None,
        });
        assert_eq!(sent.lock().unwrap().len(), 2);

        *now.lock().unwrap() = 2000.0;
        sink(ProgressUpdate {
            last_tool: Some("edit".into()),
            tokens_so_far: 4.0,
            elapsed_ms: 2000.0,
            phase: None,
            job_tag: None,
        });
        let g = sent.lock().unwrap();
        assert_eq!(g.len(), 3);
        assert_eq!(g[2]["progress"], 3);
    }
}
