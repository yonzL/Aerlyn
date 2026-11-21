//! 会话角色与状态机的最小定义，供 service/client 共用。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Control,
    Monitor,
}

impl Role {
    pub fn can_write(self) -> bool {
        matches!(self, Self::Control)
    }

    pub fn can_read(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connecting,
    Authenticating,
    Ready,
    Streaming,
    Closing,
    Closed,
}

impl SessionState {
    pub fn is_open(self) -> bool {
        matches!(self, Self::Ready | Self::Streaming)
    }

    pub fn can_accept_control(self) -> bool {
        matches!(self, Self::Ready | Self::Streaming | Self::Closing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_cannot_write() {
        assert!(Role::Control.can_write());
        assert!(!Role::Monitor.can_write());
    }

    #[test]
    fn open_states() {
        assert!(SessionState::Ready.is_open());
        assert!(SessionState::Streaming.is_open());
        assert!(!SessionState::Authenticating.is_open());
        assert!(SessionState::Closing.can_accept_control());
    }
}
