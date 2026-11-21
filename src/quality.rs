//! V4 质量档（借用 VNC 质量级 + RDP AVC444 的 4:4:4 思路）。

use crate::codec::Codec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum QualityMode {
    Lossless = 0,
    Text = 1,
    Balanced = 2,
    Motion = 3,
}

impl QualityMode {
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Lossless),
            1 => Some(Self::Text),
            2 => Some(Self::Balanced),
            3 => Some(Self::Motion),
            _ => None,
        }
    }

    pub fn id(self) -> u8 {
        self as u8
    }

    /// 该档的默认编码顺序；`Lossless` 优先 AV1（支持 q=0）。
    pub fn preference(self) -> &'static [Codec] {
        match self {
            Self::Lossless => &[Codec::Av1, Codec::H265, Codec::H264],
            Self::Text => &[Codec::Av1, Codec::H265, Codec::H264, Codec::Vp9],
            Self::Balanced => &[Codec::Av1, Codec::H265, Codec::Vp9, Codec::H264, Codec::Vp8],
            Self::Motion => &[Codec::H264, Codec::Av1, Codec::Vp9],
        }
    }

    pub fn suggested_fps(self) -> u16 {
        match self {
            Self::Lossless => 15,
            Self::Text => 30,
            Self::Balanced => 60,
            Self::Motion => 60,
        }
    }

    pub fn quality(self) -> u8 {
        match self {
            Self::Lossless => 100,
            Self::Text => 90,
            Self::Balanced => 70,
            Self::Motion => 50,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_prefers_av1() {
        assert_eq!(QualityMode::Lossless.preference()[0], Codec::Av1);
        assert_eq!(QualityMode::Lossless.suggested_fps(), 15);
        assert_eq!(QualityMode::Lossless.quality(), 100);
    }

    #[test]
    fn motion_prefers_h264() {
        assert_eq!(QualityMode::Motion.preference()[0], Codec::H264);
        assert_eq!(QualityMode::Motion.suggested_fps(), 60);
    }

    #[test]
    fn ids_roundtrip() {
        for id in 0..=3 {
            assert_eq!(QualityMode::from_id(id).unwrap().id(), id);
        }
        assert_eq!(QualityMode::from_id(4), None);
    }
}
