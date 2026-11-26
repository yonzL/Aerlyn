//! V4 `Hello.caps` / `Challenge.caps` / `Welcome.caps` 的快速能力位。
//!
//! 线格式仍是裸 `u64`；本模块只提供命名常量与类型化视图，避免调用方散落
//! `1 << 12` 之类的魔数。未知位必须保留，不得因新位出现而拒绝握手。

use crate::codec::Codec;

/// 支持 H.264 编解码。
pub const VIDEO_H264: u64 = 1 << 12;
/// 支持 H.265 编解码。
pub const VIDEO_H265: u64 = 1 << 13;
/// 支持 VP8 编解码。
pub const VIDEO_VP8: u64 = 1 << 14;
/// 支持 VP9 编解码。
pub const VIDEO_VP9: u64 = 1 << 15;
/// 支持 AV1 编解码。
pub const VIDEO_AV1: u64 = 1 << 16;
/// 支持 RAW 兜底。
pub const VIDEO_RAW: u64 = 1 << 34;

/// 支持隐私屏状态机与本地输入阻断。
pub const FEATURE_PRIVACY: u64 = 1 << 40;
/// 支持显示模式枚举与切换。
pub const FEATURE_DISPLAY_MODES: u64 = 1 << 41;
/// 支持长文本注入（上限由 `MAX_INPUT_TEXT` 声明）。
pub const FEATURE_TEXT_INPUT: u64 = 1 << 42;
/// 支持逐 peer 预览订阅。
pub const FEATURE_PREVIEW: u64 = 1 << 43;
/// 支持按 `display_key` 持久化分辨率偏好。
pub const FEATURE_RESOLUTION_PREF: u64 = 1 << 44;
/// 支持 15 fps 档。
pub const FEATURE_FPS15: u64 = 1 << 45;
/// 支持逐 peer 带宽预算。
pub const FEATURE_BANDWIDTH: u64 = 1 << 46;
/// 支持文件浏览。
pub const FEATURE_FILE_BROWSER: u64 = 1 << 47;
/// 支持文件剪贴板。
pub const FEATURE_FILE_CLIPBOARD: u64 = 1 << 48;
/// 支持 `CURSOR_STATE` 语义光标。
pub const FEATURE_SEMANTIC_CURSOR: u64 = 1 << 49;

/// 功能能力位的类型化视图。
///
/// 功能位与 `version` 无关：新端只打开自己实现的功能位，旧端未识别的位
/// 必须原样保留；不得因为出现未知功能位而拒绝握手。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeatureCaps(u64);

impl FeatureCaps {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(
        FEATURE_PRIVACY
            | FEATURE_DISPLAY_MODES
            | FEATURE_TEXT_INPUT
            | FEATURE_PREVIEW
            | FEATURE_RESOLUTION_PREF
            | FEATURE_FPS15
            | FEATURE_BANDWIDTH
            | FEATURE_FILE_BROWSER
            | FEATURE_FILE_CLIPBOARD
            | FEATURE_SEMANTIC_CURSOR,
    );

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, bit: u64) -> bool {
        self.0 & bit == bit
    }

    pub const fn with(self, bit: u64) -> Self {
        Self(self.0 | bit)
    }

    pub const fn without(self, bit: u64) -> Self {
        Self(self.0 & !bit)
    }
}

/// 视频能力位的类型化视图；保留未识别位，`bits()` 可原样写回。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoCaps(u64);

impl VideoCaps {
    pub const NONE: Self = Self(0);
    pub const ALL: Self =
        Self(VIDEO_H264 | VIDEO_H265 | VIDEO_VP8 | VIDEO_VP9 | VIDEO_AV1 | VIDEO_RAW);

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, bit: u64) -> bool {
        self.0 & bit == bit
    }

    pub const fn supports(self, codec: Codec) -> bool {
        self.contains(bit_for(codec))
    }

    pub const fn with(self, bit: u64) -> Self {
        Self(self.0 | bit)
    }

    pub const fn without(self, bit: u64) -> Self {
        Self(self.0 & !bit)
    }
}

/// 编码对应的 caps 位；RAW 也有独立位。
pub const fn bit_for(codec: Codec) -> u64 {
    match codec {
        Codec::H264 => VIDEO_H264,
        Codec::H265 => VIDEO_H265,
        Codec::Vp8 => VIDEO_VP8,
        Codec::Vp9 => VIDEO_VP9,
        Codec::Av1 => VIDEO_AV1,
        Codec::Raw => VIDEO_RAW,
    }
}

/// 从裸 `u64` 构造视频能力视图。
pub const fn from_bits(bits: u64) -> VideoCaps {
    VideoCaps::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_v4_quick_bits_and_unknown_bits_survive() {
        assert_eq!(VIDEO_H264, 1 << 12);
        assert_eq!(VIDEO_AV1, 1 << 16);
        assert_eq!(VIDEO_RAW, 1 << 34);
        let caps = VideoCaps::from_bits(VIDEO_H264 | VIDEO_AV1 | (1 << 40));
        assert!(caps.supports(Codec::H264));
        assert!(caps.supports(Codec::Av1));
        assert!(!caps.supports(Codec::Vp9));
        assert_eq!(caps.bits() & (1 << 40), 1 << 40);
    }

    #[test]
    fn feature_bits_are_separate_from_video_bits_and_unknown_bits_survive() {
        let caps = FeatureCaps::from_bits(FEATURE_PRIVACY | FEATURE_PREVIEW | (1 << 60));
        assert!(caps.contains(FEATURE_PRIVACY));
        assert!(caps.contains(FEATURE_PREVIEW));
        assert!(!caps.contains(FEATURE_FILE_BROWSER));
        assert_eq!(caps.bits() & (1 << 60), 1 << 60);
        assert_eq!(
            FeatureCaps::from_bits(VIDEO_H264 | FEATURE_PRIVACY).bits(),
            VIDEO_H264 | FEATURE_PRIVACY
        );
    }

    #[test]
    fn with_and_without_preserve_other_bits() {
        let caps = VideoCaps::NONE.with(VIDEO_VP9).with(1 << 50);
        assert!(caps.supports(Codec::Vp9));
        let caps = caps.without(VIDEO_VP9);
        assert!(!caps.supports(Codec::Vp9));
        assert_eq!(caps.bits(), 1 << 50);
    }
}
