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
    fn with_and_without_preserve_other_bits() {
        let caps = VideoCaps::NONE.with(VIDEO_VP9).with(1 << 50);
        assert!(caps.supports(Codec::Vp9));
        let caps = caps.without(VIDEO_VP9);
        assert!(!caps.supports(Codec::Vp9));
        assert_eq!(caps.bits(), 1 << 50);
    }
}
