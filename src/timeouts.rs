//! Protocol-level deadlines and token lifetime.
//!
//! The protocol owns the limits; a transport or host loop is still responsible
//! for interrupting a blocking read when a [`Deadline`] expires.

use std::time::{SystemTime, UNIX_EPOCH};

/// QUIC connection establishment budget before falling back to TCP.
pub const CONNECT_TIMEOUT_MS: u64 = 3_000;
/// TLS handshake budget.
pub const TLS_TIMEOUT_MS: u64 = 3_000;
/// `Hello -> Challenge` budget.
pub const HELLO_CHALLENGE_TIMEOUT_MS: u64 = 2_000;
/// `Challenge -> Auth` budget.
pub const CHALLENGE_AUTH_TIMEOUT_MS: u64 = 2_000;
/// `Auth -> Welcome` budget.
pub const AUTH_WELCOME_TIMEOUT_MS: u64 = 3_000;
/// `ChannelHello -> ChannelWelcome` budget.
pub const CHANNEL_HELLO_TIMEOUT_MS: u64 = 1_000;
/// A single control request identified by `req_id`.
pub const CONTROL_REQUEST_TIMEOUT_MS: u64 = 5_000;
/// Session token lifetime.
pub const TOKEN_TTL_MS: u64 = 60_000;

/// Handshake stages with independent budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutPhase {
    Connect,
    Tls,
    HelloChallenge,
    ChallengeAuth,
    AuthWelcome,
    ChannelHello,
    ControlRequest,
}

impl TimeoutPhase {
    /// Default budget for this phase.
    pub fn default_ms(self) -> u64 {
        match self {
            Self::Connect => CONNECT_TIMEOUT_MS,
            Self::Tls => TLS_TIMEOUT_MS,
            Self::HelloChallenge => HELLO_CHALLENGE_TIMEOUT_MS,
            Self::ChallengeAuth => CHALLENGE_AUTH_TIMEOUT_MS,
            Self::AuthWelcome => AUTH_WELCOME_TIMEOUT_MS,
            Self::ChannelHello => CHANNEL_HELLO_TIMEOUT_MS,
            Self::ControlRequest => CONTROL_REQUEST_TIMEOUT_MS,
        }
    }
}

/// Configurable copy of the protocol defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutPolicy {
    pub connect_ms: u64,
    pub tls_ms: u64,
    pub hello_challenge_ms: u64,
    pub challenge_auth_ms: u64,
    pub auth_welcome_ms: u64,
    pub channel_hello_ms: u64,
    pub control_request_ms: u64,
    pub token_ttl_ms: u64,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            connect_ms: CONNECT_TIMEOUT_MS,
            tls_ms: TLS_TIMEOUT_MS,
            hello_challenge_ms: HELLO_CHALLENGE_TIMEOUT_MS,
            challenge_auth_ms: CHALLENGE_AUTH_TIMEOUT_MS,
            auth_welcome_ms: AUTH_WELCOME_TIMEOUT_MS,
            channel_hello_ms: CHANNEL_HELLO_TIMEOUT_MS,
            control_request_ms: CONTROL_REQUEST_TIMEOUT_MS,
            token_ttl_ms: TOKEN_TTL_MS,
        }
    }
}

impl TimeoutPolicy {
    pub fn timeout_ms(&self, phase: TimeoutPhase) -> u64 {
        match phase {
            TimeoutPhase::Connect => self.connect_ms,
            TimeoutPhase::Tls => self.tls_ms,
            TimeoutPhase::HelloChallenge => self.hello_challenge_ms,
            TimeoutPhase::ChallengeAuth => self.challenge_auth_ms,
            TimeoutPhase::AuthWelcome => self.auth_welcome_ms,
            TimeoutPhase::ChannelHello => self.channel_hello_ms,
            TimeoutPhase::ControlRequest => self.control_request_ms,
        }
    }

    /// Start a phase at `now_ms`.
    pub fn deadline(&self, phase: TimeoutPhase, now_ms: u64) -> Deadline {
        Deadline::new(now_ms, self.timeout_ms(phase))
    }

    pub fn token_ttl_ms(&self) -> u64 {
        self.token_ttl_ms
    }
}

/// Absolute deadline in milliseconds on the caller's monotonic clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline {
    pub at_ms: u64,
}

impl Deadline {
    pub fn new(now_ms: u64, budget_ms: u64) -> Self {
        Self {
            at_ms: now_ms.saturating_add(budget_ms),
        }
    }

    pub fn expired(self, now_ms: u64) -> bool {
        now_ms >= self.at_ms
    }

    pub fn remaining_ms(self, now_ms: u64) -> u64 {
        self.at_ms.saturating_sub(now_ms)
    }
}

/// Wall-clock milliseconds for token issue timestamps. Token TTL checks use a
/// caller-provided clock so tests and embedded hosts can control it.
pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_matches_protocol_deadlines() {
        let p = TimeoutPolicy::default();
        assert_eq!(p.timeout_ms(TimeoutPhase::Connect), 3_000);
        assert_eq!(p.timeout_ms(TimeoutPhase::HelloChallenge), 2_000);
        assert_eq!(p.timeout_ms(TimeoutPhase::ChannelHello), 1_000);
        assert_eq!(p.timeout_ms(TimeoutPhase::ControlRequest), 5_000);
        assert_eq!(p.token_ttl_ms, 60_000);
    }

    #[test]
    fn deadline_counts_down_and_expires() {
        let d = TimeoutPolicy::default().deadline(TimeoutPhase::ChallengeAuth, 1_000);
        assert_eq!(d.remaining_ms(1_000), 2_000);
        assert_eq!(d.remaining_ms(2_999), 1);
        assert!(!d.expired(2_999));
        assert!(d.expired(3_000));
        assert_eq!(d.remaining_ms(9_000), 0);
    }
}
