//! 编码注册与协商。
//!
//! 规则来自 `docs/45-protocol-spec-v4.md`：
//! - 五种性能编码 + RAW 兜底；
//! - 客户端给有序偏好，host 取第一个双方都支持且满足约束的编码；
//! - profile/level/chroma/位深/分辨率/fps/码率全部协商；
//! - 无共同编码时，若双方都有 RAW 则回落 RAW，否则 `NoCommonCodec`。

/// 视频编码 id（V4 私有命名空间）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Codec {
    H264 = 0,
    H265 = 1,
    Vp8 = 2,
    Vp9 = 3,
    Av1 = 4,
    /// 零依赖兜底：原始 BGRA/BGR565 矩形。
    Raw = 5,
}

impl Codec {
    pub const ALL: [Codec; 6] = [
        Codec::H264,
        Codec::H265,
        Codec::Vp8,
        Codec::Vp9,
        Codec::Av1,
        Codec::Raw,
    ];

    /// 五种性能编码；不含 RAW。
    pub const PERFORMANCE: [Codec; 5] =
        [Codec::H264, Codec::H265, Codec::Vp8, Codec::Vp9, Codec::Av1];

    /// `codec_pref` 为空时 host 的默认排序（效率优先）。
    pub const DEFAULT_PREFERENCE: [Codec; 6] = [
        Codec::Av1,
        Codec::H265,
        Codec::Vp9,
        Codec::H264,
        Codec::Vp8,
        Codec::Raw,
    ];

    pub fn id(self) -> u8 {
        self as u8
    }

    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Codec::H264),
            1 => Some(Codec::H265),
            2 => Some(Codec::Vp8),
            3 => Some(Codec::Vp9),
            4 => Some(Codec::Av1),
            5 => Some(Codec::Raw),
            _ => None,
        }
    }

    pub fn is_raw(self) -> bool {
        matches!(self, Codec::Raw)
    }

    pub fn name(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::H265 => "h265",
            Codec::Vp8 => "vp8",
            Codec::Vp9 => "vp9",
            Codec::Av1 => "av1",
            Codec::Raw => "raw",
        }
    }
}

/// `CodecCap.chroma` 位图。
pub mod chroma {
    pub const I420: u8 = 1 << 0;
    pub const I444: u8 = 1 << 1;
    pub const I422: u8 = 1 << 2;
    /// RAW 用的 BGRA/RGB。
    pub const BGRA: u8 = 1 << 3;
}

/// `CodecCap.bit_depth` 位图。
pub mod bit_depth {
    pub const EIGHT: u8 = 1 << 0;
    pub const TEN: u8 = 1 << 1;
    pub const TWELVE: u8 = 1 << 2;
}

/// `CodecCap.flags`。
pub mod cap_flags {
    pub const HW: u8 = 1 << 0;
    pub const SW: u8 = 1 << 1;
    pub const INTRA_REFRESH: u8 = 1 << 2;
    pub const TEMPORAL_LAYERS: u8 = 1 << 3;
    pub const LOSSLESS: u8 = 1 << 4;
    pub const RAW: u8 = 1 << 5;
}

/// 一个编码端（编码或解码）声明的能力。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodecCap {
    pub codec: Codec,
    /// codec 原生 level 码（H.264 51、H.265 153、VP9 51、AV1 13、VP8 0）。
    pub max_level: u16,
    /// profile 位图；具体位含义见 V4 §7.2。
    pub profiles: u32,
    pub chroma: u8,
    pub bit_depth: u8,
    pub max_w: u16,
    pub max_h: u16,
    pub max_fps: u8,
    pub min_kbps: u32,
    pub max_kbps: u32,
    pub flags: u8,
    pub temporal_layers: u8,
}

impl CodecCap {
    /// 给测试/默认配置用的保守能力：I420 8-bit、1080p60、8 Mbps、软件。
    pub fn new(codec: Codec) -> Self {
        let (chroma, max_fps, max_kbps) = if codec.is_raw() {
            (chroma::BGRA, 5, 100_000)
        } else {
            (chroma::I420, 60, 8_000)
        };
        Self {
            codec,
            max_level: 0,
            profiles: 1,
            chroma,
            bit_depth: bit_depth::EIGHT,
            max_w: 1920,
            max_h: 1080,
            max_fps,
            min_kbps: 0,
            max_kbps,
            flags: cap_flags::SW,
            temporal_layers: 1,
        }
    }

    pub fn with_profiles(mut self, profiles: u32) -> Self {
        self.profiles = profiles;
        self
    }

    pub fn with_level(mut self, level: u16) -> Self {
        self.max_level = level;
        self
    }

    pub fn with_chroma(mut self, chroma: u8) -> Self {
        self.chroma = chroma;
        self
    }

    pub fn with_bit_depth(mut self, bit_depth: u8) -> Self {
        self.bit_depth = bit_depth;
        self
    }

    pub fn with_max_size(mut self, w: u16, h: u16) -> Self {
        self.max_w = w;
        self.max_h = h;
        self
    }

    pub fn with_fps(mut self, fps: u8) -> Self {
        self.max_fps = fps;
        self
    }

    pub fn with_bitrate(mut self, min_kbps: u32, max_kbps: u32) -> Self {
        self.min_kbps = min_kbps;
        self.max_kbps = max_kbps;
        self
    }

    pub fn with_flags(mut self, flags: u8) -> Self {
        self.flags = flags;
        self
    }

    pub fn with_temporal_layers(mut self, layers: u8) -> Self {
        self.temporal_layers = layers;
        self
    }

    pub fn hardware(&self) -> bool {
        self.flags & cap_flags::HW != 0
    }
}

/// 客户端本次想要的画面约束。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoConstraints {
    pub max_w: u16,
    pub max_h: u16,
    /// 0 = 不限制；客户端 `max_fps` 上限。
    pub max_fps: u16,
    /// 0 = 不限制。
    pub max_kbps: u32,
    /// 允许的 chroma 位图；选择时优先 I420，再 I444/I422/BGRA。
    pub chroma_pref: u8,
    /// 允许的位深位图；选择时优先 8-bit，再 10/12。
    pub bit_depth_pref: u8,
}

impl Default for VideoConstraints {
    fn default() -> Self {
        Self {
            max_w: 1920,
            max_h: 1080,
            max_fps: 60,
            max_kbps: 8_000,
            chroma_pref: chroma::I420 | chroma::I444,
            bit_depth_pref: bit_depth::EIGHT | bit_depth::TEN,
        }
    }
}

/// 协商结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedCodec {
    pub codec: Codec,
    /// 选中的 profile 位（单个 bit）。
    pub profile_bit: u32,
    pub chroma: u8,
    pub bit_depth: u8,
    /// host 侧是否用硬件编码。
    pub host_hw: bool,
    /// client 侧是否用硬件解码。
    pub client_hw: bool,
    pub max_fps: u16,
    pub max_kbps: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectError {
    NoCommonCodec,
}

fn highest_bit(v: u32) -> u32 {
    debug_assert_ne!(v, 0);
    1u32 << (31 - v.leading_zeros())
}

fn choose_chroma(available: u8, pref: u8) -> Option<u8> {
    for c in [chroma::I420, chroma::I444, chroma::I422, chroma::BGRA] {
        if available & c != 0 && pref & c != 0 {
            return Some(c);
        }
    }
    None
}

fn choose_bit_depth(available: u8, pref: u8) -> Option<u8> {
    for d in [bit_depth::EIGHT, bit_depth::TEN, bit_depth::TWELVE] {
        if available & d != 0 && pref & d != 0 {
            return Some(d);
        }
    }
    None
}

fn find_cap<'a>(caps: &'a [CodecCap], codec: Codec) -> Option<&'a CodecCap> {
    caps.iter().find(|c| c.codec == codec)
}

fn select_from_pair(
    client: &CodecCap,
    host: &CodecCap,
    constraints: &VideoConstraints,
) -> Option<SelectedCodec> {
    let profiles = client.profiles & host.profiles;
    if profiles == 0 {
        return None;
    }
    let chroma = choose_chroma(client.chroma & host.chroma, constraints.chroma_pref)?;
    let bit_depth = choose_bit_depth(
        client.bit_depth & host.bit_depth,
        constraints.bit_depth_pref,
    )?;

    let max_w = client.max_w.min(host.max_w);
    let max_h = client.max_h.min(host.max_h);
    if (constraints.max_w != 0 && constraints.max_w > max_w)
        || (constraints.max_h != 0 && constraints.max_h > max_h)
    {
        return None;
    }

    let max_fps = client.max_fps.min(host.max_fps) as u16;
    if constraints.max_fps != 0 && constraints.max_fps > max_fps {
        return None;
    }
    if max_fps == 0 {
        return None;
    }

    let max_kbps = client.max_kbps.min(host.max_kbps);
    let min_kbps = client.min_kbps.max(host.min_kbps);
    if max_kbps < min_kbps {
        return None;
    }
    if constraints.max_kbps != 0 && constraints.max_kbps > max_kbps {
        return None;
    }

    Some(SelectedCodec {
        codec: client.codec,
        profile_bit: highest_bit(profiles),
        chroma,
        bit_depth,
        host_hw: host.hardware(),
        client_hw: client.hardware(),
        max_fps: if constraints.max_fps == 0 {
            max_fps
        } else {
            constraints.max_fps.min(max_fps)
        },
        max_kbps: if constraints.max_kbps == 0 {
            max_kbps
        } else {
            constraints.max_kbps.min(max_kbps)
        },
    })
}

fn select_raw(client: &CodecCap, host: &CodecCap, constraints: &VideoConstraints) -> SelectedCodec {
    SelectedCodec {
        codec: Codec::Raw,
        profile_bit: 1,
        chroma: chroma::BGRA,
        bit_depth: bit_depth::EIGHT,
        host_hw: false,
        client_hw: false,
        max_fps: constraints
            .max_fps
            .clamp(1, client.max_fps.min(host.max_fps).max(1) as u16),
        max_kbps: if constraints.max_kbps == 0 {
            client.max_kbps.min(host.max_kbps)
        } else {
            constraints.max_kbps.min(client.max_kbps).min(host.max_kbps)
        },
    }
}

/// 按客户端顺序选择编码；空列表用 `Codec::DEFAULT_PREFERENCE`。
///
/// 五种性能编码都不满足时，若双方都有 RAW，则回落 RAW；否则 `NoCommonCodec`。
pub fn select_codec(
    preference: &[Codec],
    client_caps: &[CodecCap],
    host_caps: &[CodecCap],
    constraints: &VideoConstraints,
) -> Result<SelectedCodec, SelectError> {
    let order: Vec<Codec> = if preference.is_empty() {
        Codec::DEFAULT_PREFERENCE.to_vec()
    } else {
        preference.to_vec()
    };

    for codec in order {
        if codec.is_raw() {
            continue;
        }
        if let (Some(client), Some(host)) =
            (find_cap(client_caps, codec), find_cap(host_caps, codec))
        {
            if let Some(selected) = select_from_pair(client, host, constraints) {
                return Ok(selected);
            }
        }
    }

    if let (Some(client), Some(host)) = (
        find_cap(client_caps, Codec::Raw),
        find_cap(host_caps, Codec::Raw),
    ) {
        return Ok(select_raw(client, host, constraints));
    }

    Err(SelectError::NoCommonCodec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(codec: Codec) -> (CodecCap, CodecCap) {
        (CodecCap::new(codec), CodecCap::new(codec))
    }

    #[test]
    fn default_prefers_av1() {
        let (client, host) = pair(Codec::Av1);
        let selected = select_codec(&[], &[client], &[host], &VideoConstraints::default()).unwrap();
        assert_eq!(selected.codec, Codec::Av1);
        assert_eq!(selected.chroma, chroma::I420);
        assert_eq!(selected.bit_depth, bit_depth::EIGHT);
        assert_eq!(selected.max_fps, 60);
        assert_eq!(selected.max_kbps, 8_000);
    }

    #[test]
    fn client_max_fps_caps_selected_codec() {
        let (client, host) = pair(Codec::H264);
        let constraints = VideoConstraints {
            max_fps: 30,
            ..VideoConstraints::default()
        };
        let selected = select_codec(&[], &[client], &[host], &constraints).unwrap();
        assert_eq!(selected.max_fps, 30);
    }

    #[test]
    fn client_order_wins() {
        let mut caps_c = vec![];
        let mut caps_h = vec![];
        for codec in [Codec::H264, Codec::Av1] {
            let (c, h) = pair(codec);
            caps_c.push(c);
            caps_h.push(h);
        }
        let selected = select_codec(
            &[Codec::H264, Codec::Av1],
            &caps_c,
            &caps_h,
            &VideoConstraints::default(),
        )
        .unwrap();
        assert_eq!(selected.codec, Codec::H264);
    }

    #[test]
    fn skips_codec_without_common_profile() {
        let av1_c = CodecCap::new(Codec::Av1).with_profiles(1 << 1);
        let av1_h = CodecCap::new(Codec::Av1).with_profiles(1 << 2);
        let (h265_c, h265_h) = pair(Codec::H265);
        let selected = select_codec(
            &[],
            &[av1_c, h265_c],
            &[av1_h, h265_h],
            &VideoConstraints::default(),
        )
        .unwrap();
        assert_eq!(selected.codec, Codec::H265);
    }

    #[test]
    fn skips_codec_that_cannot_meet_resolution() {
        let av1_c = CodecCap::new(Codec::Av1).with_max_size(1280, 720);
        let av1_h = CodecCap::new(Codec::Av1).with_max_size(1280, 720);
        let constraints = VideoConstraints {
            max_w: 1920,
            max_h: 1080,
            ..VideoConstraints::default()
        };
        assert_eq!(
            select_codec(&[], &[av1_c], &[av1_h], &constraints),
            Err(SelectError::NoCommonCodec)
        );
    }

    #[test]
    fn skips_codec_that_cannot_meet_fps() {
        let (client, host) = (
            CodecCap::new(Codec::H264).with_fps(30),
            CodecCap::new(Codec::H264).with_fps(30),
        );
        let constraints = VideoConstraints {
            max_fps: 60,
            ..VideoConstraints::default()
        };
        assert_eq!(
            select_codec(&[], &[client], &[host], &constraints),
            Err(SelectError::NoCommonCodec)
        );
    }

    #[test]
    fn falls_back_to_raw_when_no_performance_codec_fits() {
        let client = vec![
            CodecCap::new(Codec::H264).with_fps(30),
            CodecCap::new(Codec::Raw),
        ];
        let host = vec![
            CodecCap::new(Codec::H264).with_fps(30),
            CodecCap::new(Codec::Raw),
        ];
        let constraints = VideoConstraints {
            max_fps: 60,
            ..VideoConstraints::default()
        };
        let selected = select_codec(&[], &client, &host, &constraints).unwrap();
        assert_eq!(selected.codec, Codec::Raw);
        assert_eq!(selected.chroma, chroma::BGRA);
        assert_eq!(selected.max_fps, 5);
    }

    #[test]
    fn prefers_i444_when_requested_and_available() {
        let client = CodecCap::new(Codec::Vp9)
            .with_chroma(chroma::I444)
            .with_profiles(1 << 1);
        let host = CodecCap::new(Codec::Vp9)
            .with_chroma(chroma::I444)
            .with_profiles(1 << 1);
        let constraints = VideoConstraints {
            chroma_pref: chroma::I444,
            ..VideoConstraints::default()
        };
        let selected = select_codec(&[], &[client], &[host], &constraints).unwrap();
        assert_eq!(selected.codec, Codec::Vp9);
        assert_eq!(selected.chroma, chroma::I444);
    }

    #[test]
    fn prefers_ten_bit_when_requested_and_available() {
        let client = CodecCap::new(Codec::Av1).with_bit_depth(bit_depth::TEN);
        let host = CodecCap::new(Codec::Av1).with_bit_depth(bit_depth::TEN);
        let constraints = VideoConstraints {
            bit_depth_pref: bit_depth::TEN,
            ..VideoConstraints::default()
        };
        let selected = select_codec(&[], &[client], &[host], &constraints).unwrap();
        assert_eq!(selected.bit_depth, bit_depth::TEN);
    }

    #[test]
    fn picks_highest_common_profile() {
        let client = CodecCap::new(Codec::H264).with_profiles((1 << 0) | (1 << 2));
        let host = CodecCap::new(Codec::H264).with_profiles((1 << 1) | (1 << 2));
        let selected = select_codec(
            &[Codec::H264],
            &[client],
            &[host],
            &VideoConstraints::default(),
        )
        .unwrap();
        assert_eq!(selected.profile_bit, 1 << 2);
    }

    #[test]
    fn no_common_codec() {
        assert_eq!(
            select_codec(&[], &[], &[], &VideoConstraints::default()),
            Err(SelectError::NoCommonCodec)
        );
    }

    #[test]
    fn codec_roundtrip_ids() {
        for codec in Codec::ALL {
            assert_eq!(Codec::from_id(codec.id()), Some(codec));
        }
        assert_eq!(Codec::from_id(6), None);
        assert_eq!(Codec::from_id(255), None);
    }
}
