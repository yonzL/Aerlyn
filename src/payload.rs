//! 变长载荷：固定头 + 紧邻的像素/压缩块/音频字节。
//!
//! 这些类型只负责线格式与边界校验，不做像素转换、zstd 解压或音频解码。
//! 长度上限与 `transfer` 的窗口配置保持一致，避免“头合法、体越界”。

use crate::data::{AudioHeader, ClipDataHeader, InputError};
use crate::plane::{CursorShapeHeader, FileChunkZstd};

/// 光标图像上限（BGRA/PNG/mask 任一种形态）。
pub const MAX_CURSOR_BYTES: usize = 1 << 20;
/// 光标任一边上限；超过视为非法形状，避免 `w*h*4` 溢出。
pub const MAX_CURSOR_DIM: u16 = 1024;
/// 剪贴板单块上限，与 `DEFAULT_CLIP_BLOCK` 一致。
pub const MAX_CLIP_CHUNK: usize = 256 << 10;
/// 音频单帧上限；48 kHz/2ch/10 ms 的 PCM 仅约 3.8 KiB，64 KiB 足够覆盖
/// 高码率压缩帧与 20 ms 块。
pub const MAX_AUDIO_FRAME: usize = 64 << 10;
/// 文件普通块/zstd 压缩块上限，与 `MAX_FILE_BLOCK` 一致。
pub const MAX_FILE_CHUNK: usize = 4 << 20;
/// ch3 `type=4` 的 UTF-8 详情上限；8 B 固定头之外只放诊断文本。
pub const MAX_FILE_ERROR_DETAIL: usize = 4 << 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadError {
    TooShort,
    Empty,
    TooLarge,
    BadFormat,
    BadDimensions,
    Trailing,
    BadHeader(InputError),
}

impl std::fmt::Display for PayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => write!(f, "payload too short"),
            Self::Empty => write!(f, "payload empty"),
            Self::TooLarge => write!(f, "payload too large"),
            Self::BadFormat => write!(f, "unsupported cursor format"),
            Self::BadDimensions => write!(f, "bad cursor dimensions"),
            Self::Trailing => write!(f, "trailing bytes"),
            Self::BadHeader(e) => write!(f, "bad header: {e}"),
        }
    }
}

impl std::error::Error for PayloadError {}

impl From<InputError> for PayloadError {
    fn from(value: InputError) -> Self {
        Self::BadHeader(value)
    }
}

/// ch4 `type=1`：15 B 头 + 光标数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorShape {
    pub header: CursorShapeHeader,
    pub data: Vec<u8>,
}

impl CursorShape {
    pub fn encode(&self) -> Result<Vec<u8>, PayloadError> {
        self.validate()?;
        let mut out = Vec::with_capacity(CursorShapeHeader::LEN + self.data.len());
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.data);
        Ok(out)
    }

    pub fn decode(buf: &[u8]) -> Result<Self, PayloadError> {
        if buf.len() < CursorShapeHeader::LEN {
            return Err(PayloadError::TooShort);
        }
        let header = CursorShapeHeader::decode(buf)?;
        let value = Self {
            header,
            data: buf[CursorShapeHeader::LEN..].to_vec(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), PayloadError> {
        let w = self.header.width;
        let h = self.header.height;
        if w == 0 || h == 0 || w > MAX_CURSOR_DIM || h > MAX_CURSOR_DIM {
            return Err(PayloadError::BadDimensions);
        }
        if self.data.is_empty() {
            return Err(PayloadError::Empty);
        }
        if self.data.len() > MAX_CURSOR_BYTES {
            return Err(PayloadError::TooLarge);
        }
        let expected = match self.header.format {
            // BGRA8。
            0 => usize::from(w) * usize::from(h) * 4,
            // PNG：不解析图像头，只限制非空与总量。
            1 => self.data.len(),
            // mask+AND/XOR：两个 1bpp 平面，每行按字节对齐。
            2 => {
                let stride = usize::from(w).div_ceil(8);
                2 * stride * usize::from(h)
            }
            _ => return Err(PayloadError::BadFormat),
        };
        if self.data.len() != expected {
            return Err(PayloadError::BadFormat);
        }
        Ok(())
    }
}

/// ch3 `type=1`：8 B offset + 文件块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChunk {
    pub offset: u64,
    pub data: Vec<u8>,
}

impl FileChunk {
    pub const HEADER_LEN: usize = 8;

    pub fn encode(&self) -> Result<Vec<u8>, PayloadError> {
        self.validate()?;
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.data.len());
        out.extend_from_slice(&self.offset.to_be_bytes());
        out.extend_from_slice(&self.data);
        Ok(out)
    }

    pub fn decode(buf: &[u8]) -> Result<Self, PayloadError> {
        if buf.len() < Self::HEADER_LEN {
            return Err(PayloadError::TooShort);
        }
        let offset = u64::from_be_bytes([
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
        ]);
        let value = Self {
            offset,
            data: buf[Self::HEADER_LEN..].to_vec(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), PayloadError> {
        if self.data.is_empty() {
            return Err(PayloadError::Empty);
        }
        if self.data.len() > MAX_FILE_CHUNK {
            return Err(PayloadError::TooLarge);
        }
        Ok(())
    }
}

/// ch3 `type=5`：12 B 头 + zstd 压缩块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChunkZstdPayload {
    pub header: FileChunkZstd,
    pub data: Vec<u8>,
}

impl FileChunkZstdPayload {
    pub fn encode(&self) -> Result<Vec<u8>, PayloadError> {
        self.validate()?;
        let mut out = Vec::with_capacity(FileChunkZstd::LEN + self.data.len());
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.data);
        Ok(out)
    }

    pub fn decode(buf: &[u8]) -> Result<Self, PayloadError> {
        if buf.len() < FileChunkZstd::LEN {
            return Err(PayloadError::TooShort);
        }
        let header = FileChunkZstd::decode(buf)?;
        let value = Self {
            header,
            data: buf[FileChunkZstd::LEN..].to_vec(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), PayloadError> {
        if self.data.is_empty() {
            return Err(PayloadError::Empty);
        }
        if self.data.len() > MAX_FILE_CHUNK
            || self.header.uncompressed_len == 0
            || self.header.uncompressed_len as usize > MAX_FILE_CHUNK
        {
            return Err(PayloadError::TooLarge);
        }
        Ok(())
    }
}

/// ch7 `type=1`：10 B 头 + 剪贴板块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipData {
    pub header: ClipDataHeader,
    pub data: Vec<u8>,
}

impl ClipData {
    pub fn encode(&self) -> Result<Vec<u8>, PayloadError> {
        self.validate()?;
        let mut out =
            Vec::with_capacity(ClipDataHeader::encode(&self.header).len() + self.data.len());
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.data);
        Ok(out)
    }

    pub fn decode(buf: &[u8]) -> Result<Self, PayloadError> {
        if buf.len() < 10 {
            return Err(PayloadError::TooShort);
        }
        let header = ClipDataHeader::decode(buf)?;
        let value = Self {
            header,
            data: buf[10..].to_vec(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), PayloadError> {
        if self.data.is_empty() {
            return Err(PayloadError::Empty);
        }
        if self.data.len() > MAX_CLIP_CHUNK {
            return Err(PayloadError::TooLarge);
        }
        Ok(())
    }
}

/// ch5 `type=2`：12 B 头 + 音频字节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFrame {
    pub header: AudioHeader,
    pub data: Vec<u8>,
}

impl AudioFrame {
    pub fn encode(&self) -> Result<Vec<u8>, PayloadError> {
        self.validate()?;
        let mut out = Vec::with_capacity(AudioHeader::encode(&self.header).len() + self.data.len());
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.data);
        Ok(out)
    }

    pub fn decode(buf: &[u8]) -> Result<Self, PayloadError> {
        if buf.len() < 12 {
            return Err(PayloadError::TooShort);
        }
        let header = AudioHeader::decode(buf)?;
        let value = Self {
            header,
            data: buf[12..].to_vec(),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), PayloadError> {
        if self.data.is_empty() {
            return Err(PayloadError::Empty);
        }
        if self.data.len() > MAX_AUDIO_FRAME {
            return Err(PayloadError::TooLarge);
        }
        Ok(())
    }
}

/// ch3 `type=4`：8 B 固定头 + UTF-8 诊断详情。
///
/// 头布局：`task_id:u32`、`code:u16`、`detail_len:u16`，全部大端。
/// 这是文件数据面的错误帧；控制面的 `FileErrorInfo` 仍是 JSON，两者不混用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileErrorPayload {
    pub task_id: u32,
    pub code: u16,
    pub detail: String,
}

impl FileErrorPayload {
    pub const LEN: usize = 8;

    pub fn encode(&self) -> Result<Vec<u8>, PayloadError> {
        self.validate()?;
        let detail = self.detail.as_bytes();
        let mut out = Vec::with_capacity(Self::LEN + detail.len());
        out.extend_from_slice(&self.task_id.to_be_bytes());
        out.extend_from_slice(&self.code.to_be_bytes());
        out.extend_from_slice(&(detail.len() as u16).to_be_bytes());
        out.extend_from_slice(detail);
        Ok(out)
    }

    pub fn decode(buf: &[u8]) -> Result<Self, PayloadError> {
        if buf.len() < Self::LEN {
            return Err(PayloadError::TooShort);
        }
        let task_id = u32::from_be_bytes(buf[0..4].try_into().unwrap());
        let code = u16::from_be_bytes(buf[4..6].try_into().unwrap());
        let detail_len = usize::from(u16::from_be_bytes(buf[6..8].try_into().unwrap()));
        if detail_len > MAX_FILE_ERROR_DETAIL {
            return Err(PayloadError::TooLarge);
        }
        if buf.len() != Self::LEN + detail_len {
            return Err(if buf.len() < Self::LEN + detail_len {
                PayloadError::TooShort
            } else {
                PayloadError::Trailing
            });
        }
        let detail = std::str::from_utf8(&buf[Self::LEN..])
            .map_err(|_| PayloadError::BadFormat)?
            .to_owned();
        Ok(Self {
            task_id,
            code,
            detail,
        })
    }

    fn validate(&self) -> Result<(), PayloadError> {
        if self.detail.len() > MAX_FILE_ERROR_DETAIL {
            return Err(PayloadError::TooLarge);
        }
        if self.detail.len() > usize::from(u16::MAX) {
            return Err(PayloadError::TooLarge);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(format: u8, w: u16, h: u16, data: Vec<u8>) -> CursorShape {
        CursorShape {
            header: CursorShapeHeader {
                cursor_id: 7,
                generation: 2,
                hx: -1,
                hy: 3,
                width: w,
                height: h,
                format,
            },
            data,
        }
    }

    #[test]
    fn cursor_bgra_png_mask_roundtrip() {
        let bgra = cursor(0, 2, 2, vec![0xAB; 16]);
        assert_eq!(CursorShape::decode(&bgra.encode().unwrap()).unwrap(), bgra);

        let png = cursor(1, 32, 32, b"fake-png".to_vec());
        assert_eq!(CursorShape::decode(&png.encode().unwrap()).unwrap(), png);

        let mask = cursor(2, 9, 2, vec![0xFF; 8]);
        assert_eq!(CursorShape::decode(&mask.encode().unwrap()).unwrap(), mask);
    }

    #[test]
    fn cursor_rejects_bad_sizes_and_formats() {
        assert_eq!(
            cursor(0, 2, 2, vec![0; 15]).encode(),
            Err(PayloadError::BadFormat)
        );
        assert_eq!(
            cursor(2, 9, 2, vec![0; 5]).encode(),
            Err(PayloadError::BadFormat)
        );
        assert_eq!(
            cursor(9, 2, 2, vec![0; 16]).encode(),
            Err(PayloadError::BadFormat)
        );
        assert_eq!(
            cursor(0, 0, 2, vec![0; 16]).encode(),
            Err(PayloadError::BadDimensions)
        );
        assert_eq!(
            cursor(1, 32, 32, Vec::new()).encode(),
            Err(PayloadError::Empty)
        );
        assert_eq!(CursorShape::decode(&[0; 14]), Err(PayloadError::TooShort));
    }

    #[test]
    fn file_chunks_roundtrip() {
        let chunk = FileChunk {
            offset: 1 << 30,
            data: vec![1, 2, 3],
        };
        assert_eq!(FileChunk::decode(&chunk.encode().unwrap()).unwrap(), chunk);
        assert_eq!(
            FileChunk {
                offset: 0,
                data: Vec::new()
            }
            .encode(),
            Err(PayloadError::Empty)
        );
    }

    #[test]
    fn zstd_chunk_roundtrip_and_bounds() {
        let chunk = FileChunkZstdPayload {
            header: FileChunkZstd {
                offset: 4096,
                uncompressed_len: 64 << 10,
            },
            data: vec![0x28, 0xB5, 0x2F, 0xFD],
        };
        assert_eq!(
            FileChunkZstdPayload::decode(&chunk.encode().unwrap()).unwrap(),
            chunk
        );
        let bad = FileChunkZstdPayload {
            header: FileChunkZstd {
                offset: 0,
                uncompressed_len: 0,
            },
            data: vec![1],
        };
        assert_eq!(bad.encode(), Err(PayloadError::TooLarge));
    }

    #[test]
    fn file_error_roundtrip_and_bounds() {
        let value = FileErrorPayload {
            task_id: 7,
            code: 61,
            detail: "permission denied".to_owned(),
        };
        let encoded = value.encode().unwrap();
        assert_eq!(encoded.len(), FileErrorPayload::LEN + value.detail.len());
        assert_eq!(FileErrorPayload::decode(&encoded).unwrap(), value);

        let empty = FileErrorPayload {
            task_id: 1,
            code: 1,
            detail: String::new(),
        };
        assert_eq!(
            FileErrorPayload::decode(&empty.encode().unwrap()).unwrap(),
            empty
        );

        let too_long = FileErrorPayload {
            task_id: 1,
            code: 1,
            detail: "x".repeat(MAX_FILE_ERROR_DETAIL + 1),
        };
        assert_eq!(too_long.encode(), Err(PayloadError::TooLarge));

        let mut bad_len = encoded.clone();
        bad_len[6..8].copy_from_slice(&u16::MAX.to_be_bytes());
        assert_eq!(
            FileErrorPayload::decode(&bad_len),
            Err(PayloadError::TooLarge)
        );

        let mut short = encoded.clone();
        short.pop();
        assert_eq!(
            FileErrorPayload::decode(&short),
            Err(PayloadError::TooShort)
        );

        let mut trailing = encoded;
        trailing.push(0);
        assert_eq!(
            FileErrorPayload::decode(&trailing),
            Err(PayloadError::Trailing)
        );

        let mut bad_utf8 = FileErrorPayload {
            task_id: 1,
            code: 1,
            detail: "ok".to_owned(),
        }
        .encode()
        .unwrap();
        bad_utf8[8] = 0xFF;
        assert_eq!(
            FileErrorPayload::decode(&bad_utf8),
            Err(PayloadError::BadFormat)
        );
    }

    #[test]
    fn clip_and_audio_roundtrip() {
        let clip = ClipData {
            header: ClipDataHeader {
                seq: 3,
                format: 1,
                offset: 512,
            },
            data: b"hello".to_vec(),
        };
        assert_eq!(ClipData::decode(&clip.encode().unwrap()).unwrap(), clip);

        let audio = AudioFrame {
            header: AudioHeader {
                seq: 9,
                pts_us: 123_456,
            },
            data: vec![0xAA; 960],
        };
        assert_eq!(AudioFrame::decode(&audio.encode().unwrap()).unwrap(), audio);
    }

    #[test]
    fn clip_and_audio_reject_empty_and_oversize() {
        let clip = ClipData {
            header: ClipDataHeader {
                seq: 1,
                format: 0,
                offset: 0,
            },
            data: Vec::new(),
        };
        assert_eq!(clip.encode(), Err(PayloadError::Empty));

        let clip = ClipData {
            header: ClipDataHeader {
                seq: 1,
                format: 0,
                offset: 0,
            },
            data: vec![0; MAX_CLIP_CHUNK + 1],
        };
        assert_eq!(clip.encode(), Err(PayloadError::TooLarge));

        let audio = AudioFrame {
            header: AudioHeader { seq: 1, pts_us: 0 },
            data: vec![0; MAX_AUDIO_FRAME + 1],
        };
        assert_eq!(audio.encode(), Err(PayloadError::TooLarge));
        assert_eq!(AudioFrame::decode(&[0; 11]), Err(PayloadError::TooShort));
    }
}
