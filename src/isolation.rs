use crate::types::Isolation;
use crate::util::resolve_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationResult {
    pub flags: Vec<String>,
    pub cwd: String,
    pub path: Option<String>,
}

pub fn map_isolation(isolation: &Isolation, server_cwd: &str) -> IsolationResult {
    match isolation {
        Isolation::None => IsolationResult {
            flags: vec![],
            cwd: server_cwd.to_string(),
            path: Some(resolve_path(server_cwd)),
        },
        Isolation::CallerProvided { path } => IsolationResult {
            flags: vec!["--workspace".into(), path.clone()],
            cwd: path.clone(),
            path: Some(resolve_path(path)),
        },
        Isolation::BackendProvided { name, base } => {
            let mut flags = vec!["--worktree".to_string()];
            if let Some(n) = name.as_ref().filter(|s| !s.is_empty()) {
                flags.push(n.clone());
            }
            if let Some(b) = base.as_ref().filter(|s| !s.is_empty()) {
                flags.push("--worktree-base".into());
                flags.push(b.clone());
            }
            IsolationResult {
                flags,
                cwd: server_cwd.to_string(),
                path: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::resolve_path;

    #[test]
    fn none_uses_server_cwd_and_locks() {
        let r = map_isolation(&Isolation::None, "/srv");
        assert!(r.flags.is_empty());
        assert_eq!(r.cwd, "/srv");
        assert_eq!(r.path.as_deref(), Some(resolve_path("/srv").as_str()));
    }

    #[test]
    fn caller_provided_canonicalizes_equivalent_paths() {
        let r = map_isolation(
            &Isolation::CallerProvided {
                path: "/repo/.".into(),
            },
            "/srv",
        );
        assert_eq!(r.flags, ["--workspace", "/repo/."]);
        assert_eq!(r.cwd, "/repo/.");
        assert_eq!(r.path.as_deref(), Some(resolve_path("/repo/.").as_str()));
        assert_eq!(r.path.as_deref(), Some(resolve_path("/repo").as_str()));
    }

    #[test]
    fn caller_provided_maps_workspace_and_lock() {
        let r = map_isolation(
            &Isolation::CallerProvided {
                path: "/repo".into(),
            },
            "/srv",
        );
        assert_eq!(r.flags, ["--workspace", "/repo"]);
        assert_eq!(r.cwd, "/repo");
        assert_eq!(r.path.as_deref(), Some("/repo"));
    }

    #[test]
    fn backend_provided_with_name_and_base() {
        let r = map_isolation(
            &Isolation::BackendProvided {
                name: Some("wt1".into()),
                base: Some("main".into()),
            },
            "/srv",
        );
        assert_eq!(r.flags, ["--worktree", "wt1", "--worktree-base", "main"]);
        assert_eq!(r.cwd, "/srv");
        assert_eq!(r.path, None);
    }

    #[test]
    fn backend_provided_bare_worktree() {
        let r = map_isolation(
            &Isolation::BackendProvided {
                name: None,
                base: None,
            },
            "/srv",
        );
        assert_eq!(r.flags, ["--worktree"]);
    }

    #[test]
    fn equivalent_paths_share_lock_key() {
        let none = map_isolation(&Isolation::None, "/repo");
        let caller = map_isolation(
            &Isolation::CallerProvided {
                path: "/repo/.".into(),
            },
            "/repo",
        );
        assert_eq!(none.path, caller.path);
    }
}
