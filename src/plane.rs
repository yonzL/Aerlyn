//! 非 ch0 的二进制热路径消息头。
//!
//! 视频帧本体继续用 36B [`crate::media::MediaHeader`]；这里补齐 V3 附录 B
//! 中其余固定头：StreamHello/StreamEnd、文件尾、zstd 块、光标形状、音频格式、
//! 网络报告与 SliceNack。变长数据（像素、压缩块、音频帧）由调用方紧跟在头后。

use crate::data::InputError;

fn get_u16(buf: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([buf[off], buf[off + 1]])
}

fn get_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn get_u64(buf: &[u8], off: usize) -> u64 {
    u64::from_be_bytes([
        buf[off],
        buf[off + 1],
        buf[off + 2],
        buf[off + 3],
        buf[off + 4],
        buf[off + 5],
        buf[off + 6],
        buf[off + 7],
    ])
}

fn need(buf: &[u8], n: usize) -> Result<(), InputError> {
    if buf.len() < n {
        Err(InputError::TooShort)
    } else {
        Ok(())
    }
}

/// ch2 `type=0`，14 B。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamHello {
    pub channel: u8,
    pub display: u8,
    pub codec: u8,
    pub profile: u8,
    pub chroma: u8,
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub kbps: u32,
}

impl StreamHello {
    pub const LEN: usize = 14;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.channel;
        out[1] = self.display;
        out[2] = self.codec;
        out[3] = self.profile;
        out[4] = self.chroma;
        out[5..7].copy_from_slice(&self.width.to_be_bytes());
        out[7..9].copy_from_slice(&self.height.to_be_bytes());
        out[9] = self.fps;
        out[10..14].copy_from_slice(&self.kbps.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        need(buf, Self::LEN)?;
        Ok(Self {
            channel: buf[0],
            display: buf[1],
            codec: buf[2],
            profile: buf[3],
            chroma: buf[4],
            width: get_u16(buf, 5),
            height: get_u16(buf, 7),
            fps: buf[9],
            kbps: get_u32(buf, 10),
        })
    }
}

/// ch2 `type=2`，1 B。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamEnd {
    pub reason: u8,
}

impl StreamEnd {
    pub const LEN: usize = 1;

    /// 旧流因会话内编码切换而结束。
    pub const CODEC_SWITCH: u8 = 3;

    pub fn encode(&self) -> [u8; Self::LEN] {
        [self.reason]
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        need(buf, Self::LEN)?;
        Ok(Self { reason: buf[0] })
    }
}

/// ch3 `type=3`，32 B。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileEnd {
    pub sha256: [u8; 32],
}

impl FileEnd {
    pub const LEN: usize = 32;

    pub fn encode(&self) -> [u8; Self::LEN] {
        self.sha256
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        need(buf, Self::LEN)?;
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&buf[..32]);
        Ok(Self { sha256 })
    }
}

/// ch3 `type=5`，12 B；payload 为 zstd 压缩块。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileChunkZstd {
    pub offset: u64,
    pub uncompressed_len: u32,
}

impl FileChunkZstd {
    pub const LEN: usize = 12;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..8].copy_from_slice(&self.offset.to_be_bytes());
        out[8..12].copy_from_slice(&self.uncompressed_len.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        need(buf, Self::LEN)?;
        Ok(Self {
            offset: get_u64(buf, 0),
            uncompressed_len: get_u32(buf, 8),
        })
    }
}

/// ch3 `type=6`，12 B；接收方要求发送方从 `resume_offset` 继续。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileResumeHint {
    pub task_id: u32,
    pub resume_offset: u64,
}

impl FileResumeHint {
    pub const LEN: usize = 12;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..4].copy_from_slice(&self.task_id.to_be_bytes());
        out[4..12].copy_from_slice(&self.resume_offset.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        need(buf, Self::LEN)?;
        Ok(Self {
            task_id: get_u32(buf, 0),
            resume_offset: get_u64(buf, 4),
        })
    }
}

/// ch4 `type=1`，15 B；payload 为像素/PNG/mask 数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorShapeHeader {
    pub cursor_id: u32,
    pub generation: u16,
    pub hx: i16,
    pub hy: i16,
    pub width: u16,
    pub height: u16,
    pub format: u8,
}

impl CursorShapeHeader {
    pub const LEN: usize = 15;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..4].copy_from_slice(&self.cursor_id.to_be_bytes());
        out[4..6].copy_from_slice(&self.generation.to_be_bytes());
        out[6..8].copy_from_slice(&self.hx.to_be_bytes());
        out[8..10].copy_from_slice(&self.hy.to_be_bytes());
        out[10..12].copy_from_slice(&self.width.to_be_bytes());
        out[12..14].copy_from_slice(&self.height.to_be_bytes());
        out[14] = self.format;
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        need(buf, Self::LEN)?;
        Ok(Self {
            cursor_id: get_u32(buf, 0),
            generation: get_u16(buf, 4),
            hx: i16::from_be_bytes([buf[6], buf[7]]),
            hy: i16::from_be_bytes([buf[8], buf[9]]),
            width: get_u16(buf, 10),
            height: get_u16(buf, 12),
            format: buf[14],
        })
    }
}

/// ch4 `type=2`，2 B。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorReset {
    pub generation: u16,
}

impl CursorReset {
    pub const LEN: usize = 2;

    pub fn encode(&self) -> [u8; Self::LEN] {
        self.generation.to_be_bytes()
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        need(buf, Self::LEN)?;
        Ok(Self {
            generation: get_u16(buf, 0),
        })
    }
}

/// ch5 `type=1`，7 B。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFmt {
    pub codec: u8,
    pub rate: u32,
    pub channels: u8,
    pub frame_ms: u8,
}

impl AudioFmt {
    pub const LEN: usize = 7;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0] = self.codec;
        out[1..5].copy_from_slice(&self.rate.to_be_bytes());
        out[5] = self.channels;
        out[6] = self.frame_ms;
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        need(buf, Self::LEN)?;
        Ok(Self {
            codec: buf[0],
            rate: get_u32(buf, 1),
            channels: buf[5],
            frame_ms: buf[6],
        })
    }
}

/// ch6 `type=2`，18 B。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetReport {
    pub rtt_us: u32,
    pub loss_ppm: u32,
    pub recv_bps: u64,
    pub queue_ms: u16,
}

impl NetReport {
    pub const LEN: usize = 18;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..4].copy_from_slice(&self.rtt_us.to_be_bytes());
        out[4..8].copy_from_slice(&self.loss_ppm.to_be_bytes());
        out[8..16].copy_from_slice(&self.recv_bps.to_be_bytes());
        out[16..18].copy_from_slice(&self.queue_ms.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        need(buf, Self::LEN)?;
        Ok(Self {
            rtt_us: get_u32(buf, 0),
            loss_ppm: get_u32(buf, 4),
            recv_bps: get_u64(buf, 8),
            queue_ms: get_u16(buf, 16),
        })
    }
}

/// ch6 `type=3`，最小 12 B；当缺片超过 64 时追加 `u64` 字。
///
/// 线格式：`frame_id:u32 | bitmap[0]:u64 | bitmap[1..]:u64*`。
/// 小于等于 64 片的旧实现保持 12 B 不变；最多 16 个 `u64`（1024 片）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SliceNack {
    pub frame_id: u32,
    pub missing_bitmap: u64,
    pub extra: Vec<u64>,
}

impl SliceNack {
    /// 兼容旧 12 B 线格式的最小长度。
    pub const LEN: usize = 12;
    pub const MAX_WORDS: usize = 16;
    pub const MAX_LEN: usize = Self::LEN + (Self::MAX_WORDS - 1) * 8;

    pub fn new(frame_id: u32, missing_bitmap: u64) -> Self {
        Self {
            frame_id,
            missing_bitmap,
            extra: Vec::new(),
        }
    }

    /// 用最多 16 个 `u64` 构造；第一个字写入 `missing_bitmap`。
    pub fn with_words(frame_id: u32, words: &[u64]) -> Self {
        let mut iter = words.iter().copied();
        let first = iter.next().unwrap_or(0);
        let extra: Vec<u64> = iter.take(Self::MAX_WORDS - 1).collect();
        Self {
            frame_id,
            missing_bitmap: first,
            extra,
        }
    }

    pub fn words(&self) -> impl Iterator<Item = u64> + '_ {
        std::iter::once(self.missing_bitmap).chain(self.extra.iter().copied())
    }

    pub fn missing_count(&self) -> u32 {
        self.words().map(|word| word.count_ones()).sum()
    }

    pub fn encode(&self) -> Vec<u8> {
        let extra = &self.extra[..self.extra.len().min(Self::MAX_WORDS - 1)];
        let mut out = Vec::with_capacity(Self::LEN + extra.len() * 8);
        out.extend_from_slice(&self.frame_id.to_be_bytes());
        out.extend_from_slice(&self.missing_bitmap.to_be_bytes());
        for word in extra {
            out.extend_from_slice(&word.to_be_bytes());
        }
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() < Self::LEN || (buf.len() - Self::LEN) % 8 != 0 {
            return Err(InputError::TooShort);
        }
        if buf.len() > Self::MAX_LEN {
            return Err(InputError::Trailing);
        }
        let mut frame_id = [0u8; 4];
        frame_id.copy_from_slice(&buf[0..4]);
        let mut missing = [0u8; 8];
        missing.copy_from_slice(&buf[4..12]);
        let extra = (Self::LEN..buf.len())
            .step_by(8)
            .map(|offset| {
                let mut word = [0u8; 8];
                word.copy_from_slice(&buf[offset..offset + 8]);
                u64::from_be_bytes(word)
            })
            .collect();
        Ok(Self {
            frame_id: u32::from_be_bytes(frame_id),
            missing_bitmap: u64::from_be_bytes(missing),
            extra,
        })
    }
}

/// V3 附录 B 的 type 注册表；新增 type 只能追加。
pub mod type_id {
    pub mod input {
        /// ch1 `type=1`；DATAGRAM 与可靠流共用同一载荷。
        pub const INPUT_BATCH: u16 = 1;
    }
    pub mod video {
        pub const STREAM_HELLO: u16 = 0;
        pub const VIDEO_FRAME: u16 = 1;
        pub const STREAM_END: u16 = 2;
    }
    pub mod file {
        pub const FILE_CHUNK: u16 = 1;
        pub const FILE_ACK: u16 = 2;
        pub const FILE_END: u16 = 3;
        pub const FILE_ERROR: u16 = 4;
        pub const FILE_CHUNK_ZSTD: u16 = 5;
        pub const FILE_RESUME_HINT: u16 = 6;
    }
    pub mod cursor {
        pub const CURSOR_SHAPE: u16 = 1;
        pub const CURSOR_RESET: u16 = 2;
        pub const CURSOR_POS: u16 = 3;
        pub const CURSOR_STATE: u16 = 4;
    }
    pub mod audio {
        pub const AUDIO_FMT: u16 = 1;
        pub const AUDIO_FRAME: u16 = 2;
    }
    pub mod feedback {
        pub const FRAME_ACK: u16 = 1;
        pub const NET_REPORT: u16 = 2;
        pub const SLICE_NACK: u16 = 3;
    }
    pub mod clipboard {
        pub const CLIP_DATA: u16 = 1;
        pub const CLIP_ACK: u16 = 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_hello_roundtrip() {
        let h = StreamHello {
            channel: 2,
            display: 0,
            codec: 4,
            profile: 1,
            chroma: 1,
            width: 1920,
            height: 1080,
            fps: 60,
            kbps: 8_000,
        };
        assert_eq!(StreamHello::decode(&h.encode()).unwrap(), h);
    }

    #[test]
    fn fixed_headers_roundtrip() {
        let end = StreamEnd { reason: 3 };
        assert_eq!(StreamEnd::decode(&end.encode()).unwrap(), end);

        let f = FileEnd { sha256: [7u8; 32] };
        assert_eq!(FileEnd::decode(&f.encode()).unwrap(), f);

        let z = FileChunkZstd {
            offset: 1 << 30,
            uncompressed_len: 1 << 20,
        };
        assert_eq!(FileChunkZstd::decode(&z.encode()).unwrap(), z);

        let h = FileResumeHint {
            task_id: 9,
            resume_offset: 3 << 20,
        };
        assert_eq!(FileResumeHint::decode(&h.encode()).unwrap(), h);

        let c = CursorShapeHeader {
            cursor_id: 42,
            generation: 3,
            hx: -2,
            hy: 5,
            width: 32,
            height: 32,
            format: 0,
        };
        assert_eq!(CursorShapeHeader::decode(&c.encode()).unwrap(), c);

        let r = CursorReset { generation: 9 };
        assert_eq!(CursorReset::decode(&r.encode()).unwrap(), r);

        let a = AudioFmt {
            codec: 0,
            rate: 48_000,
            channels: 2,
            frame_ms: 10,
        };
        assert_eq!(AudioFmt::decode(&a.encode()).unwrap(), a);

        let n = NetReport {
            rtt_us: 40_000,
            loss_ppm: 2_000,
            recv_bps: 8_000_000,
            queue_ms: 20,
        };
        assert_eq!(NetReport::decode(&n.encode()).unwrap(), n);

        let s = SliceNack {
            frame_id: 7,
            missing_bitmap: 0b1010,
            ..Default::default()
        };
        assert_eq!(SliceNack::decode(&s.encode()).unwrap(), s);
    }

    #[test]
    fn short_buffers_are_rejected() {
        assert_eq!(StreamHello::decode(&[0; 13]), Err(InputError::TooShort));
        assert_eq!(NetReport::decode(&[0; 17]), Err(InputError::TooShort));
        assert_eq!(FileEnd::decode(&[0; 31]), Err(InputError::TooShort));
        assert_eq!(FileResumeHint::decode(&[0; 11]), Err(InputError::TooShort));
    }
}
