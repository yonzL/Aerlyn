//! V4 媒体头（36 字节，网络序）与 codec 配置校验。
//!
//! 结构参考 Airhand `airhand-proto/src/media.rs` 的 `VideoHeader`：
//! 保留未知 codec（`decode_any`），让传输层仍能读序号并确认；
//! 只有解码器需要过滤不认识的编码。V4 用大端网络序，字段更多（rect/frag/layer）。

use crate::codec::Codec;

pub const MEDIA_HEADER_LEN: usize = 36;
/// 兼容命名。
pub const FRAME_HEADER_LEN: usize = MEDIA_HEADER_LEN;
/// 36 + 8×N ≤ 255 → 最多 27 个矩形。
pub const MAX_RECTS: usize = 27;

pub mod frame_flags {
    pub const KEY: u8 = 1 << 0;
    pub const CURSOR_EXCLUDED: u8 = 1 << 1;
    pub const LAST_FRAG: u8 = 1 << 2;
    pub const INTRA_REFRESH: u8 = 1 << 3;
    pub const REGION: u8 = 1 << 4;
    pub const LAYER_SHIFT: u8 = 5;
    pub const LAYER_MASK: u8 = 0b111;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.x.to_be_bytes());
        out.extend_from_slice(&self.y.to_be_bytes());
        out.extend_from_slice(&self.w.to_be_bytes());
        out.extend_from_slice(&self.h.to_be_bytes());
    }

    fn decode(buf: &[u8]) -> Self {
        Self {
            x: u16::from_be_bytes([buf[0], buf[1]]),
            y: u16::from_be_bytes([buf[2], buf[3]]),
            w: u16::from_be_bytes([buf[4], buf[5]]),
            h: u16::from_be_bytes([buf[6], buf[7]]),
        }
    }
}

/// 视频/媒体头。`codec_id` 保留原始编号：未知编码也能被传输层读取。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaHeader {
    pub flags: u8,
    pub codec_id: u8,
    pub frame_id: u32,
    pub request_id: u32,
    pub frag_idx: u16,
    pub frag_count: u16,
    pub pts_us: u64,
    pub capture_ms: u32,
    pub encode_ms: u32,
    pub rects: Vec<Rect>,
}

/// 兼容 V4 文档里的命名。
pub type FrameHeader = MediaHeader;

impl MediaHeader {
    pub fn new(codec: Codec, frame_id: u32, pts_us: u64) -> Self {
        Self {
            flags: 0,
            codec_id: codec.id(),
            frame_id,
            request_id: 0,
            frag_idx: 0,
            frag_count: 1,
            pts_us,
            capture_ms: 0,
            encode_ms: 0,
            rects: Vec::new(),
        }
    }

    /// 已知编码返回 `Some`；未知编码返回 `None`（对应 Airhand `Codec::Unknown`）。
    pub fn codec(&self) -> Option<Codec> {
        Codec::from_id(self.codec_id)
    }

    pub fn set_codec(&mut self, codec: Codec) {
        self.codec_id = codec.id();
    }

    pub fn key(&self) -> bool {
        self.flags & frame_flags::KEY != 0
    }

    pub fn set_key(&mut self, on: bool) {
        if on {
            self.flags |= frame_flags::KEY;
        } else {
            self.flags &= !frame_flags::KEY;
        }
    }

    pub fn cursor_excluded(&self) -> bool {
        self.flags & frame_flags::CURSOR_EXCLUDED != 0
    }

    pub fn set_cursor_excluded(&mut self, on: bool) {
        if on {
            self.flags |= frame_flags::CURSOR_EXCLUDED;
        } else {
            self.flags &= !frame_flags::CURSOR_EXCLUDED;
        }
    }

    pub fn last_frag(&self) -> bool {
        self.flags & frame_flags::LAST_FRAG != 0
    }

    pub fn set_last_frag(&mut self, on: bool) {
        if on {
            self.flags |= frame_flags::LAST_FRAG;
        } else {
            self.flags &= !frame_flags::LAST_FRAG;
        }
    }

    pub fn intra_refresh(&self) -> bool {
        self.flags & frame_flags::INTRA_REFRESH != 0
    }

    pub fn region_coded(&self) -> bool {
        self.flags & frame_flags::REGION != 0
    }

    pub fn layer(&self) -> u8 {
        (self.flags >> frame_flags::LAYER_SHIFT) & frame_flags::LAYER_MASK
    }

    pub fn set_layer(&mut self, layer: u8) {
        debug_assert!(layer <= frame_flags::LAYER_MASK);
        self.flags = (self.flags & !(frame_flags::LAYER_MASK << frame_flags::LAYER_SHIFT))
            | ((layer & frame_flags::LAYER_MASK) << frame_flags::LAYER_SHIFT);
    }

    pub fn hdr_len(&self) -> usize {
        MEDIA_HEADER_LEN + 8 * self.rects.len()
    }

    pub fn encode(&self) -> Result<Vec<u8>, MediaError> {
        if self.rects.len() > MAX_RECTS {
            return Err(MediaError::TooManyRects);
        }
        let hdr_len = self.hdr_len();
        let mut out = Vec::with_capacity(hdr_len);
        out.push(1); // fmt
        out.push(self.flags);
        out.push(self.codec_id);
        out.push(hdr_len as u8);
        out.extend_from_slice(&self.frame_id.to_be_bytes());
        out.extend_from_slice(&self.request_id.to_be_bytes());
        out.extend_from_slice(&self.frag_idx.to_be_bytes());
        out.extend_from_slice(&self.frag_count.to_be_bytes());
        out.extend_from_slice(&self.pts_us.to_be_bytes());
        out.extend_from_slice(&self.capture_ms.to_be_bytes());
        out.extend_from_slice(&self.encode_ms.to_be_bytes());
        out.extend_from_slice(&(self.rects.len() as u16).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // reserved
        for rect in &self.rects {
            rect.encode_into(&mut out);
        }
        debug_assert_eq!(out.len(), hdr_len);
        Ok(out)
    }

    /// 严格解析：未知 codec 视为错误。
    pub fn decode(buf: &[u8]) -> Result<Self, MediaError> {
        let h = Self::decode_any(buf)?;
        if Codec::from_id(h.codec_id).is_none() {
            return Err(MediaError::BadCodec);
        }
        Ok(h)
    }

    /// 宽松解析：保留未知 codec（Airhand `decode_any` 的做法）。
    pub fn decode_any(buf: &[u8]) -> Result<Self, MediaError> {
        if buf.len() < MEDIA_HEADER_LEN {
            return Err(MediaError::TooShort);
        }
        if buf[0] != 1 {
            return Err(MediaError::BadFmt);
        }
        let hdr_len = buf[3] as usize;
        if hdr_len < MEDIA_HEADER_LEN || hdr_len > buf.len() {
            return Err(MediaError::BadHdrLen);
        }
        let rect_count = u16::from_be_bytes([buf[32], buf[33]]) as usize;
        let expected = MEDIA_HEADER_LEN + 8 * rect_count;
        if expected > hdr_len {
            return Err(MediaError::BadRectCount);
        }
        let mut rects = Vec::with_capacity(rect_count);
        for i in 0..rect_count {
            let off = MEDIA_HEADER_LEN + 8 * i;
            rects.push(Rect::decode(&buf[off..off + 8]));
        }
        Ok(Self {
            flags: buf[1],
            codec_id: buf[2],
            frame_id: u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
            request_id: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
            frag_idx: u16::from_be_bytes([buf[12], buf[13]]),
            frag_count: u16::from_be_bytes([buf[14], buf[15]]),
            pts_us: u64::from_be_bytes([
                buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23],
            ]),
            capture_ms: u32::from_be_bytes([buf[24], buf[25], buf[26], buf[27]]),
            encode_ms: u32::from_be_bytes([buf[28], buf[29], buf[30], buf[31]]),
            rects,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaError {
    TooShort,
    BadFmt,
    BadHdrLen,
    BadRectCount,
    BadCodec,
    TooManyRects,
}

/// 兼容命名。
pub type FrameError = MediaError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    Empty,
    NotAnnexB,
    MissingParameterSet,
    NotAv1SequenceHeader,
}

/// 校验 `CodecConfig.extradata` 是否包含解码器初始化所需的参数集。
///
/// 参考 H.264/H.265 的 Annex B 参数集与 AV1 的 sequence header OBU；VP8/VP9 无需 extradata。
pub fn validate_extradata(codec: Codec, data: &[u8]) -> Result<(), ConfigError> {
    match codec {
        Codec::H264 => {
            if data.is_empty() {
                return Err(ConfigError::Empty);
            }
            let mut has_sps = false;
            let mut has_pps = false;
            for nal in annex_b_nals(data) {
                match nal & 0x1f {
                    7 => has_sps = true,
                    8 => has_pps = true,
                    _ => {}
                }
            }
            if has_sps && has_pps {
                Ok(())
            } else {
                Err(ConfigError::MissingParameterSet)
            }
        }
        Codec::H265 => {
            if data.is_empty() {
                return Err(ConfigError::Empty);
            }
            let mut has_vps = false;
            let mut has_sps = false;
            let mut has_pps = false;
            for nal in annex_b_nals(data) {
                match (nal >> 1) & 0x3f {
                    32 => has_vps = true,
                    33 => has_sps = true,
                    34 => has_pps = true,
                    _ => {}
                }
            }
            if has_vps && has_sps && has_pps {
                Ok(())
            } else {
                Err(ConfigError::MissingParameterSet)
            }
        }
        Codec::Av1 => {
            if data.is_empty() {
                return Err(ConfigError::Empty);
            }
            let obu_type = (data[0] >> 3) & 0x0f;
            if obu_type == 1 {
                Ok(())
            } else {
                Err(ConfigError::NotAv1SequenceHeader)
            }
        }
        Codec::Vp8 | Codec::Vp9 => Ok(()),
        Codec::Raw => Ok(()),
    }
}

/// 遍历 Annex B 起始码后的 NAL header 第一字节。
fn annex_b_nals(data: &[u8]) -> impl Iterator<Item = u8> + '_ {
    let mut i = 0;
    std::iter::from_fn(move || {
        while i + 3 <= data.len() {
            let start = if data[i..].starts_with(&[0, 0, 1]) {
                Some(i + 3)
            } else if i + 4 <= data.len() && data[i..].starts_with(&[0, 0, 0, 1]) {
                Some(i + 4)
            } else {
                None
            };
            if let Some(nal_start) = start {
                i = nal_start + 1;
                if nal_start < data.len() {
                    return Some(data[nal_start]);
                }
            } else {
                i += 1;
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MediaHeader {
        let mut h = MediaHeader::new(Codec::Av1, 42, 1_700_000_000_123);
        h.set_key(true);
        h.set_cursor_excluded(true);
        h.set_last_frag(true);
        h.set_layer(3);
        h.request_id = 7;
        h.frag_idx = 1;
        h.frag_count = 2;
        h.capture_ms = 5;
        h.encode_ms = 9;
        h.rects = vec![Rect::new(10, 20, 300, 200), Rect::new(0, 0, 1920, 1080)];
        h
    }

    #[test]
    fn header_roundtrip() {
        let h = sample();
        let bytes = h.encode().unwrap();
        assert_eq!(bytes.len(), MEDIA_HEADER_LEN + 16);
        let back = MediaHeader::decode(&bytes).unwrap();
        assert_eq!(back, h);
        assert!(back.key());
        assert!(back.cursor_excluded());
        assert!(back.last_frag());
        assert_eq!(back.layer(), 3);
        assert_eq!(back.codec(), Some(Codec::Av1));
    }

    #[test]
    fn unknown_codec_survives_loose_decode() {
        let mut bytes = sample().encode().unwrap();
        bytes[2] = 99;
        assert_eq!(
            MediaHeader::decode(&bytes),
            Err(MediaError::BadCodec),
            "strict decode must reject unknown codec"
        );
        let h = MediaHeader::decode_any(&bytes).unwrap();
        assert_eq!(h.codec_id, 99);
        assert_eq!(h.codec(), None);
        assert_eq!(h.frame_id, 42);
    }

    #[test]
    fn too_many_rects_rejected() {
        let mut h = MediaHeader::new(Codec::H264, 1, 0);
        h.rects = vec![Rect::new(0, 0, 1, 1); MAX_RECTS + 1];
        assert_eq!(h.encode(), Err(MediaError::TooManyRects));
    }

    #[test]
    fn truncated_rejected() {
        let bytes = sample().encode().unwrap();
        assert_eq!(
            MediaHeader::decode(&bytes[..MEDIA_HEADER_LEN - 1]),
            Err(MediaError::TooShort)
        );
    }

    #[test]
    fn bad_hdr_len_rejected() {
        let mut bytes = sample().encode().unwrap();
        bytes[3] = 10;
        assert_eq!(MediaHeader::decode(&bytes), Err(MediaError::BadHdrLen));
    }

    #[test]
    fn bad_rect_count_rejected() {
        let mut bytes = sample().encode().unwrap();
        bytes[32..34].copy_from_slice(&99u16.to_be_bytes());
        assert_eq!(
            MediaHeader::decode_any(&bytes),
            Err(MediaError::BadRectCount)
        );
    }

    #[test]
    fn extra_header_bytes_are_ignored() {
        let h = sample();
        let bytes = h.encode().unwrap();
        let mut extended = bytes.clone();
        extended.splice(MEDIA_HEADER_LEN + 16..MEDIA_HEADER_LEN + 16, [0u8; 4]);
        extended[3] = (MEDIA_HEADER_LEN + 16 + 4) as u8;
        let back = MediaHeader::decode_any(&extended).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn h264_extradata_requires_sps_and_pps() {
        let sps_pps = [0, 0, 0, 1, 0x67, 0x64, 0, 0, 0, 1, 0x68, 0xee, 0x3c];
        assert_eq!(validate_extradata(Codec::H264, &sps_pps), Ok(()));
        assert_eq!(
            validate_extradata(Codec::H264, &[0, 0, 0, 1, 0x67, 0x64]),
            Err(ConfigError::MissingParameterSet)
        );
    }

    #[test]
    fn h265_extradata_requires_vps_sps_pps() {
        let vps_sps_pps = [
            0, 0, 0, 1, 0x40, 0x01, 0, 0, 0, 1, 0x42, 0x01, 0, 0, 0, 1, 0x44, 0x01,
        ];
        assert_eq!(validate_extradata(Codec::H265, &vps_sps_pps), Ok(()));
        assert_eq!(
            validate_extradata(Codec::H265, &[0, 0, 0, 1, 0x40, 0x01]),
            Err(ConfigError::MissingParameterSet)
        );
    }

    #[test]
    fn av1_extradata_must_start_with_sequence_header() {
        assert_eq!(validate_extradata(Codec::Av1, &[0x08, 0x00]), Ok(()));
        assert_eq!(
            validate_extradata(Codec::Av1, &[0x10, 0x00]),
            Err(ConfigError::NotAv1SequenceHeader)
        );
    }
}
