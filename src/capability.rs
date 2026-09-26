use crate::types::Capability;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityResult {
    pub flags: Vec<String>,
    pub is_write: bool,
    pub forced: bool,
    pub downgraded: bool,
}

pub fn map_capability(capability: Capability, allow_unsandboxed: bool) -> CapabilityResult {
    match capability {
        Capability::Ask => CapabilityResult {
            flags: vec!["--mode".into(), "ask".into(), "--force".into()],
            is_write: false,
            forced: true,
            downgraded: false,
        },
        Capability::Plan => CapabilityResult {
            flags: vec!["--mode".into(), "plan".into(), "--force".into()],
            is_write: false,
            forced: true,
            downgraded: false,
        },
        Capability::Write => CapabilityResult {
            flags: vec!["--sandbox".into(), "enabled".into(), "--force".into()],
            is_write: true,
            forced: true,
            downgraded: false,
        },
        Capability::WriteUnsandboxed => {
            if allow_unsandboxed {
                CapabilityResult {
                    flags: vec!["--sandbox".into(), "disabled".into(), "--force".into()],
                    is_write: true,
                    forced: true,
                    downgraded: false,
                }
            } else {
                CapabilityResult {
                    flags: vec!["--sandbox".into(), "enabled".into(), "--force".into()],
                    is_write: true,
                    forced: true,
                    downgraded: true,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_carries_force() {
        let r = map_capability(Capability::Ask, false);
        assert_eq!(r.flags, ["--mode", "ask", "--force"]);
        assert!(!r.is_write);
        assert!(r.forced);
        assert!(!r.downgraded);
    }

    #[test]
    fn plan_carries_force() {
        let r = map_capability(Capability::Plan, false);
        assert_eq!(r.flags, ["--mode", "plan", "--force"]);
        assert!(!r.is_write);
        assert!(r.forced);
        assert!(!r.downgraded);
    }

    #[test]
    fn write_is_sandboxed_and_forced() {
        let r = map_capability(Capability::Write, false);
        assert_eq!(r.flags, ["--sandbox", "enabled", "--force"]);
        assert!(r.is_write);
        assert!(r.forced);
        assert!(!r.downgraded);
    }

    #[test]
    fn write_unsandboxed_with_allow() {
        let r = map_capability(Capability::WriteUnsandboxed, true);
        assert_eq!(r.flags, ["--sandbox", "disabled", "--force"]);
        assert!(r.is_write);
        assert!(r.forced);
        assert!(!r.downgraded);
    }

    #[test]
    fn write_unsandboxed_without_signal_downgrades() {
        let r = map_capability(Capability::WriteUnsandboxed, false);
        assert_eq!(r.flags, ["--sandbox", "enabled", "--force"]);
        assert!(r.is_write);
        assert!(r.forced);
        assert!(r.downgraded);
    }

    #[test]
    fn default_capability_is_ask() {
        let r = map_capability(Capability::Ask, false);
        assert_eq!(r.flags, ["--mode", "ask", "--force"]);
        assert!(r.forced);
    }
}
