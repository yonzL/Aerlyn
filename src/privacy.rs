//! Client-side replica of the host privacy screen state.

use crate::control::{privacy_mode, PrivacyState};

/// Applies host revisions monotonically and tracks a local overlay dismissal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrivacyTracker {
    current: Option<PrivacyState>,
    local_unlock_revision: u64,
}

impl PrivacyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> Option<&PrivacyState> {
        self.current.as_ref()
    }

    /// Returns true when the state was newer and therefore applied.
    pub fn apply(&mut self, state: PrivacyState) -> bool {
        if state.validate().is_err() {
            return false;
        }
        if let Some(current) = &self.current {
            if state.revision <= current.revision {
                return false;
            }
        }
        if state.local_unlock_revision > self.local_unlock_revision {
            self.local_unlock_revision = state.local_unlock_revision;
        }
        self.current = Some(state);
        true
    }

    /// Dismisses the current host revision locally; does not mutate host state.
    pub fn local_unlock(&mut self) -> bool {
        let Some(current) = &self.current else {
            return false;
        };
        if self.local_unlock_revision >= current.revision {
            return false;
        }
        self.local_unlock_revision = current.revision;
        true
    }

    pub fn local_unlock_revision(&self) -> u64 {
        self.local_unlock_revision
    }

    pub fn overlay_visible(&self) -> bool {
        let Some(current) = &self.current else {
            return false;
        };
        current.state == privacy_mode::ON
            && current.overlay_active
            && self.local_unlock_revision != current.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(revision: u64, mode: u8, overlay: bool) -> PrivacyState {
        PrivacyState {
            revision,
            state: mode,
            overlay_active: overlay,
            reason: String::new(),
            local_unlock_revision: 0,
        }
    }

    #[test]
    fn applies_only_newer_revisions() {
        let mut t = PrivacyTracker::new();
        assert!(t.apply(state(2, privacy_mode::ON, true)));
        assert!(!t.apply(state(2, privacy_mode::OFF, false)));
        assert!(!t.apply(state(1, privacy_mode::OFF, false)));
        assert_eq!(t.current().unwrap().revision, 2);
    }

    #[test]
    fn local_unlock_hides_only_current_revision() {
        let mut t = PrivacyTracker::new();
        assert!(t.apply(state(4, privacy_mode::ON, true)));
        assert!(t.overlay_visible());
        assert!(t.local_unlock());
        assert!(!t.overlay_visible());
        assert!(!t.local_unlock());
        assert!(t.apply(state(5, privacy_mode::ON, true)));
        assert!(t.overlay_visible());
    }

    #[test]
    fn invalid_state_is_ignored() {
        let mut t = PrivacyTracker::new();
        let mut bad = state(1, 9, true);
        bad.local_unlock_revision = 2;
        assert!(!t.apply(bad));
        assert!(t.current().is_none());
    }
}
