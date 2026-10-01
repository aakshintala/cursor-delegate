pub mod cursor;
pub mod types;

use crate::types::{Capability, JobSpec};
use types::Spawned;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Cursor,
}

impl Backend {
    /// None for a backend that is not implemented (pi, claude until #16/#17).
    pub fn from_name(name: &str) -> Option<Backend> {
        match name {
            "cursor" => Some(Self::Cursor),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
        }
    }

    /// Resolved binary path (CURSOR_AGENT_BIN, then PATH, then ~/.local/bin).
    pub fn bin(self) -> String {
        match self {
            Self::Cursor => cursor::resolve_bin(None),
        }
    }

    /// Full argv (without the binary) and whether the run may write.
    pub fn argv(
        self,
        model: &str,
        capability: Capability,
        session: Option<&str>,
        prompt: &str,
    ) -> (Vec<String>, bool) {
        match self {
            Self::Cursor => cursor::argv(model, capability, session, prompt),
        }
    }

    /// Spawn and drive the child; same contract as today's `Backend::run`.
    pub fn spawn(self, spec: &JobSpec) -> Spawned {
        match self {
            Self::Cursor => cursor::spawn(spec),
        }
    }

    pub(crate) fn fill_doctor(
        self,
        report: &mut crate::types::DoctorReport,
        opts: &crate::doctor::RunDoctorOpts<'_>,
    ) {
        match self {
            Self::Cursor => cursor::doctor::fill(report, opts),
        }
    }

    pub(crate) fn doctor_lines(self, report: &crate::types::DoctorReport) -> (String, bool) {
        match self {
            Self::Cursor => cursor::doctor::lines(report),
        }
    }
}

impl types::Runner for Backend {
    fn run(&self, spec: &JobSpec) -> Spawned {
        self.spawn(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_name_maps_cursor_and_rejects_the_rest() {
        assert_eq!(Backend::from_name("cursor"), Some(Backend::Cursor));
        assert_eq!(Backend::Cursor.name(), "cursor");
        assert!(Backend::from_name("pi").is_none());
        assert!(Backend::from_name("claude").is_none());
    }
}
