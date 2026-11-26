//! Stable, binding-friendly constants for Aerlyn Wire.
//!
//! This crate intentionally has no dependencies, no state machine and no
//! transport. Swift/Kotlin/JS bindings can consume these values without
//! pulling in the Rust protocol engine.

pub const PROTO_VERSION: u32 = 400;
pub const ALPN: &str = "aerlyn/4";
pub const ALPN_TCP: &str = "aerlyn/4-tcp";

pub const MEDIA_HEADER_LEN: usize = 36;
pub const MAX_RECTS: usize = 27;
pub const CURSOR_POS_LEN: usize = 15;
pub const CURSOR_STATE_LEN: usize = 22;
pub const AUDIO_HEADER_LEN: usize = 12;
pub const FILE_ACK_LEN: usize = 10;
pub const FILE_END_LEN: usize = 32;

pub const PRIVACY_REASON_MAX: usize = 256;
pub const PREVIEW_MAX_FPS: u16 = 240;
pub const PREVIEW_MAX_KBPS: u32 = 1_000_000;
pub const PREVIEW_MAX_DIM: u16 = 8192;
pub const PREVIEW_QUALITY_MAX: u8 = 3;

/// Highest built-in semantic cursor value (0..=28).
pub const CURSOR_SEMANTIC_MAX: u16 = 28;
/// Feature bit for `CURSOR_STATE`.
pub const FEATURE_SEMANTIC_CURSOR: u64 = 1 << 49;
