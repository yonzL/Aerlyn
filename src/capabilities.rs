//! RustDesk 形状的 `SupportedEncoding` / `SupportedDecoding` 兼容层。
//!
//! 参考 `rustdesk/libs/base/protos/message.proto:109-123`、`:656-674`。
//! V4 内部用详细的 `CodecCap`；这一层用于 v2 适配器和"只报支持/不支持"的端点。

use crate::codec::{chroma, Codec, CodecCap};

/// RustDesk `CodecAbility`：一组编码的 4:4:4 能力。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CodecAbility {
    pub vp8: bool,
    pub vp9: bool,
    pub av1: bool,
    pub h264: bool,
    pub h265: bool,
}

impl CodecAbility {
    pub fn supports(self, codec: Codec) -> bool {
        match codec {
            Codec::Vp8 => self.vp8,
            Codec::Vp9 => self.vp9,
            Codec::Av1 => self.av1,
            Codec::H264 => self.h264,
            Codec::H265 => self.h265,
            Codec::Raw => false,
        }
    }

    fn set(&mut self, codec: Codec) {
        match codec {
            Codec::Vp8 => self.vp8 = true,
            Codec::Vp9 => self.vp9 = true,
            Codec::Av1 => self.av1 = true,
            Codec::H264 => self.h264 = true,
            Codec::H265 => self.h265 = true,
            Codec::Raw => {}
        }
    }
}

/// RustDesk `SupportedEncoding`：对端能编/能解的编码集合。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SupportedEncoding {
    pub h264: bool,
    pub h265: bool,
    pub vp8: bool,
    pub av1: bool,
    /// 哪些编码支持 4:4:4。
    pub i444: CodecAbility,
}

impl SupportedEncoding {
    pub fn supports(&self, codec: Codec) -> bool {
        match codec {
            Codec::H264 => self.h264,
            Codec::H265 => self.h265,
            Codec::Vp8 => self.vp8,
            Codec::Av1 => self.av1,
            // RustDesk 的 SupportedEncoding 没有 vp9 字段；V4 仍可通过 CodecCap 声明。
            Codec::Vp9 => false,
            Codec::Raw => false,
        }
    }

    pub fn from_caps(caps: &[CodecCap]) -> Self {
        let mut s = Self::default();
        for cap in caps {
            match cap.codec {
                Codec::H264 => s.h264 = true,
                Codec::H265 => s.h265 = true,
                Codec::Vp8 => s.vp8 = true,
                Codec::Av1 => s.av1 = true,
                _ => {}
            }
            if cap.chroma & chroma::I444 != 0 {
                s.i444.set(cap.codec);
            }
        }
        s
    }

    /// 生成默认能力（1080p60/8Mbps）；详细参数应优先用 `CodecCap`。
    pub fn to_caps(&self) -> Vec<CodecCap> {
        let mut out = Vec::new();
        for (codec, on) in [
            (Codec::H264, self.h264),
            (Codec::H265, self.h265),
            (Codec::Vp8, self.vp8),
            (Codec::Av1, self.av1),
        ] {
            if on {
                let mut c = CodecCap::new(codec);
                if self.i444.supports(codec) {
                    c.chroma |= chroma::I444;
                }
                out.push(c);
            }
        }
        out
    }

    /// 按 V4 默认效率顺序过滤。
    pub fn preference(&self) -> Vec<Codec> {
        Codec::DEFAULT_PREFERENCE
            .iter()
            .copied()
            .filter(|c| self.supports(*c))
            .collect()
    }
}

/// RustDesk `SupportedDecoding.PreferCodec`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreferCodec {
    #[default]
    Auto,
    Vp9,
    H264,
    H265,
    Vp8,
    Av1,
}

impl PreferCodec {
    pub fn id(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::Vp9 => 1,
            Self::H264 => 2,
            Self::H265 => 3,
            Self::Vp8 => 4,
            Self::Av1 => 5,
        }
    }

    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Auto),
            1 => Some(Self::Vp9),
            2 => Some(Self::H264),
            3 => Some(Self::H265),
            4 => Some(Self::Vp8),
            5 => Some(Self::Av1),
            _ => None,
        }
    }

    pub fn codec(self) -> Option<Codec> {
        match self {
            Self::Auto => None,
            Self::Vp9 => Some(Codec::Vp9),
            Self::H264 => Some(Codec::H264),
            Self::H265 => Some(Codec::H265),
            Self::Vp8 => Some(Codec::Vp8),
            Self::Av1 => Some(Codec::Av1),
        }
    }
}

/// RustDesk `SupportedDecoding`：解码能力 + 偏好。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SupportedDecoding {
    pub ability_vp9: i32,
    pub ability_h264: i32,
    pub ability_h265: i32,
    pub prefer: PreferCodec,
    pub ability_vp8: i32,
    pub ability_av1: i32,
    pub i444: CodecAbility,
    /// 0 = 未指定；取值见 `codec::chroma`。
    pub prefer_chroma: u8,
}

impl SupportedDecoding {
    pub fn supports(&self, codec: Codec) -> bool {
        match codec {
            Codec::Vp9 => self.ability_vp9 > 0,
            Codec::H264 => self.ability_h264 > 0,
            Codec::H265 => self.ability_h265 > 0,
            Codec::Vp8 => self.ability_vp8 > 0,
            Codec::Av1 => self.ability_av1 > 0,
            Codec::Raw => false,
        }
    }

    pub fn from_caps(caps: &[CodecCap]) -> Self {
        let mut d = Self::default();
        for cap in caps {
            match cap.codec {
                Codec::Vp9 => d.ability_vp9 = 1,
                Codec::H264 => d.ability_h264 = 1,
                Codec::H265 => d.ability_h265 = 1,
                Codec::Vp8 => d.ability_vp8 = 1,
                Codec::Av1 => d.ability_av1 = 1,
                _ => {}
            }
            if cap.chroma & chroma::I444 != 0 {
                d.i444.set(cap.codec);
            }
        }
        d
    }

    /// 偏好优先，其次 V4 默认顺序。
    pub fn preference(&self) -> Vec<Codec> {
        let mut out = Vec::new();
        if let Some(c) = self.prefer.codec() {
            if self.supports(c) {
                out.push(c);
            }
        }
        for c in Codec::DEFAULT_PREFERENCE {
            if c != Codec::Raw && self.supports(c) && !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    pub fn to_caps(&self) -> Vec<CodecCap> {
        let mut out = Vec::new();
        for (codec, on) in [
            (Codec::Vp9, self.ability_vp9 > 0),
            (Codec::H264, self.ability_h264 > 0),
            (Codec::H265, self.ability_h265 > 0),
            (Codec::Vp8, self.ability_vp8 > 0),
            (Codec::Av1, self.ability_av1 > 0),
        ] {
            if on {
                let mut c = CodecCap::new(codec);
                if self.i444.supports(codec) {
                    c.chroma |= chroma::I444;
                }
                out.push(c);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{select_codec, VideoConstraints};

    #[test]
    fn supported_encoding_roundtrip() {
        let caps = vec![
            CodecCap::new(Codec::H264).with_chroma(chroma::I420 | chroma::I444),
            CodecCap::new(Codec::Av1),
        ];
        let enc = SupportedEncoding::from_caps(&caps);
        assert!(enc.h264 && enc.av1 && !enc.h265 && !enc.vp8);
        assert!(enc.i444.h264);
        assert!(!enc.i444.av1);
        let back = enc.to_caps();
        assert_eq!(back.len(), 2);
        assert!(back.iter().any(|c| c.codec == Codec::H264));
        assert!(back.iter().any(|c| c.codec == Codec::Av1));
    }

    #[test]
    fn supported_decoding_prefers_requested_codec() {
        let d = SupportedDecoding {
            ability_vp9: 1,
            ability_h264: 1,
            ability_av1: 1,
            prefer: PreferCodec::H264,
            ..SupportedDecoding::default()
        };
        assert_eq!(d.preference()[0], Codec::H264);
        assert!(d.preference().contains(&Codec::Av1));
        assert!(!d.preference().contains(&Codec::Vp8));
    }

    #[test]
    fn prefer_codec_ids_roundtrip() {
        for id in 0..=5 {
            assert_eq!(PreferCodec::from_id(id).unwrap().id(), id);
        }
        assert_eq!(PreferCodec::from_id(6), None);
    }

    #[test]
    fn decoding_caps_can_drive_selection() {
        let client = SupportedDecoding {
            ability_h264: 1,
            ability_av1: 1,
            prefer: PreferCodec::Av1,
            ..SupportedDecoding::default()
        };
        let host = SupportedEncoding {
            h264: true,
            av1: true,
            ..SupportedEncoding::default()
        };
        let selected = select_codec(
            &client.preference(),
            &client.to_caps(),
            &host.to_caps(),
            &VideoConstraints::default(),
        )
        .unwrap();
        assert_eq!(selected.codec, Codec::Av1);
    }

    #[test]
    fn codec_ability_i444() {
        let mut a = CodecAbility::default();
        a.h264 = true;
        assert!(a.supports(Codec::H264));
        assert!(!a.supports(Codec::Av1));
    }
}
