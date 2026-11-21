//! 错误码注册表：code + class + need + retry_after_ms。
//! 码段与 V3/V4 一致：0–29 通用/身份/会话，30–49 文件，50–69 视频/输入，70–89 剪贴板/音频。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorClass {
    RetryableTransport,
    Retryable,
    NeedsUser,
    Fatal,
}

impl ErrorClass {
    pub fn id(self) -> u8 {
        match self {
            Self::RetryableTransport => 0,
            Self::Retryable => 1,
            Self::NeedsUser => 2,
            Self::Fatal => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ErrorCode {
    Unspecified = 0,
    UnsupportedVersion = 1,
    UnsupportedCapability = 2,
    Unauthorized = 3,
    PolicyDenied = 4,
    Busy = 5,
    SessionGone = 6,
    Timeout = 7,
    ProtocolError = 8,
    ShuttingDown = 9,
    UnknownKey = 10,
    HostFingerprintMismatch = 11,
    SignatureInvalid = 12,
    TokenInvalid = 13,
    SessionTakenOver = 20,
    ResumeExpired = 21,
    FileTooLarge = 30,
    DiskFull = 31,
    FileLocked = 32,
    PathInvalid = 33,
    ShaMismatch = 34,
    ResumeMetaMismatch = 35,
    CodecUnsupported = 50,
    VideoStalled = 51,
    // 52 保留：V4 新增编码器/解码器错误从 53 开始，避免重编号 V3 已发布码。
    EncoderInitFailed = 53,
    DecoderError = 54,
    CodecFallback = 55,
    InjectFailed = 60,
    NoDesktop = 61,
    UipiBlocked = 62,
    ClipboardTooLarge = 70,
    ClipboardBusy = 71,
    AudioUnsupported = 80,
}

impl ErrorCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Unspecified => "Unspecified",
            Self::UnsupportedVersion => "UnsupportedVersion",
            Self::UnsupportedCapability => "UnsupportedCapability",
            Self::Unauthorized => "Unauthorized",
            Self::PolicyDenied => "PolicyDenied",
            Self::Busy => "Busy",
            Self::SessionGone => "SessionGone",
            Self::Timeout => "Timeout",
            Self::ProtocolError => "ProtocolError",
            Self::ShuttingDown => "ShuttingDown",
            Self::UnknownKey => "UnknownKey",
            Self::HostFingerprintMismatch => "HostFingerprintMismatch",
            Self::SignatureInvalid => "SignatureInvalid",
            Self::TokenInvalid => "TokenInvalid",
            Self::SessionTakenOver => "SessionTakenOver",
            Self::ResumeExpired => "ResumeExpired",
            Self::FileTooLarge => "FileTooLarge",
            Self::DiskFull => "DiskFull",
            Self::FileLocked => "FileLocked",
            Self::PathInvalid => "PathInvalid",
            Self::ShaMismatch => "ShaMismatch",
            Self::ResumeMetaMismatch => "ResumeMetaMismatch",
            Self::CodecUnsupported => "CodecUnsupported",
            Self::VideoStalled => "VideoStalled",
            Self::EncoderInitFailed => "EncoderInitFailed",
            Self::DecoderError => "DecoderError",
            Self::CodecFallback => "CodecFallback",
            Self::InjectFailed => "InjectFailed",
            Self::NoDesktop => "NoDesktop",
            Self::UipiBlocked => "UipiBlocked",
            Self::ClipboardTooLarge => "ClipboardTooLarge",
            Self::ClipboardBusy => "ClipboardBusy",
            Self::AudioUnsupported => "AudioUnsupported",
        }
    }

    pub fn class(self) -> ErrorClass {
        match self {
            Self::Busy
            | Self::Timeout
            | Self::FileLocked
            | Self::ResumeMetaMismatch
            | Self::VideoStalled
            | Self::EncoderInitFailed
            | Self::DecoderError
            | Self::InjectFailed
            | Self::NoDesktop
            | Self::ClipboardBusy => ErrorClass::Retryable,
            Self::SessionGone | Self::ShuttingDown | Self::ResumeExpired | Self::CodecFallback => {
                ErrorClass::RetryableTransport
            }
            Self::UnsupportedVersion
            | Self::UnsupportedCapability
            | Self::Unauthorized
            | Self::PolicyDenied
            | Self::UnknownKey
            | Self::DiskFull
            | Self::ShaMismatch
            | Self::CodecUnsupported
            | Self::UipiBlocked
            | Self::ClipboardTooLarge
            | Self::AudioUnsupported => ErrorClass::NeedsUser,
            _ => ErrorClass::Fatal,
        }
    }

    pub fn from_code(code: u16) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.code() == code)
    }

    pub const ALL: [ErrorCode; 33] = [
        ErrorCode::Unspecified,
        ErrorCode::UnsupportedVersion,
        ErrorCode::UnsupportedCapability,
        ErrorCode::Unauthorized,
        ErrorCode::PolicyDenied,
        ErrorCode::Busy,
        ErrorCode::SessionGone,
        ErrorCode::Timeout,
        ErrorCode::ProtocolError,
        ErrorCode::ShuttingDown,
        ErrorCode::UnknownKey,
        ErrorCode::HostFingerprintMismatch,
        ErrorCode::SignatureInvalid,
        ErrorCode::TokenInvalid,
        ErrorCode::SessionTakenOver,
        ErrorCode::ResumeExpired,
        ErrorCode::FileTooLarge,
        ErrorCode::DiskFull,
        ErrorCode::FileLocked,
        ErrorCode::PathInvalid,
        ErrorCode::ShaMismatch,
        ErrorCode::ResumeMetaMismatch,
        ErrorCode::CodecUnsupported,
        ErrorCode::VideoStalled,
        ErrorCode::EncoderInitFailed,
        ErrorCode::DecoderError,
        ErrorCode::CodecFallback,
        ErrorCode::InjectFailed,
        ErrorCode::NoDesktop,
        ErrorCode::UipiBlocked,
        ErrorCode::ClipboardTooLarge,
        ErrorCode::ClipboardBusy,
        ErrorCode::AudioUnsupported,
    ];
}

/// `Error`/`Bye` 的线上负载（`reason` 只进日志）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: u16,
    pub class: u8,
    #[serde(default)]
    pub need: u64,
    #[serde(default)]
    pub retry_after_ms: u32,
}

impl ErrorInfo {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code: code.code(),
            class: code.class().id(),
            need: 0,
            retry_after_ms: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_roundtrip() {
        for code in ErrorCode::ALL {
            assert_eq!(ErrorCode::from_code(code.code()), Some(code));
        }
        assert_eq!(ErrorCode::from_code(52), None);
        assert_eq!(ErrorCode::from_code(999), None);
    }

    #[test]
    fn published_v3_v4_codes_do_not_move() {
        assert_eq!(ErrorCode::CodecUnsupported.code(), 50);
        assert_eq!(ErrorCode::VideoStalled.code(), 51);
        assert_eq!(ErrorCode::EncoderInitFailed.code(), 53);
        assert_eq!(ErrorCode::DecoderError.code(), 54);
        assert_eq!(ErrorCode::CodecFallback.code(), 55);
        assert_eq!(ErrorCode::InjectFailed.code(), 60);
        assert_eq!(ErrorCode::NoDesktop.code(), 61);
        assert_eq!(ErrorCode::UipiBlocked.code(), 62);
    }

    #[test]
    fn class_mapping() {
        assert_eq!(ErrorCode::Busy.class(), ErrorClass::Retryable);
        assert_eq!(ErrorCode::PolicyDenied.class(), ErrorClass::NeedsUser);
        assert_eq!(ErrorCode::SignatureInvalid.class(), ErrorClass::Fatal);
        assert_eq!(
            ErrorCode::CodecFallback.class(),
            ErrorClass::RetryableTransport
        );
    }

    #[test]
    fn error_info_roundtrip() {
        let e = ErrorInfo::new(ErrorCode::DecoderError);
        let json = serde_json::to_string(&e).unwrap();
        let back: ErrorInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
        assert_eq!(back.class, ErrorClass::Retryable.id());
    }
}
