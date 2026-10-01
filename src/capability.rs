use crate::types::Capability;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityResult {
    pub flags: Vec<String>,
    pub is_write: bool,
}

pub fn map_capability(capability: Capability, allow_unsandboxed: bool) -> CapabilityResult {
    match capability {
        Capability::Ask => CapabilityResult {
            flags: vec!["--mode".into(), "ask".into(), "--force".into()],
            is_write: false,
        },
        Capability::WriteUnsandboxed => {
            let sandbox = if allow_unsandboxed {
                "disabled"
            } else {
                "enabled"
            };
            CapabilityResult {
                flags: vec!["--sandbox".into(), sandbox.into(), "--force".into()],
                is_write: true,
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
    }

    #[test]
    fn write_unsandboxed_with_allow() {
        let r = map_capability(Capability::WriteUnsandboxed, true);
        assert_eq!(r.flags, ["--sandbox", "disabled", "--force"]);
        assert!(r.is_write);
    }

    #[test]
    fn write_unsandboxed_without_allow_stays_sandboxed() {
        let r = map_capability(Capability::WriteUnsandboxed, false);
        assert_eq!(r.flags, ["--sandbox", "enabled", "--force"]);
        assert!(r.is_write);
    }
}
