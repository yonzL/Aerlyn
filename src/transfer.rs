//! ch3/ch7 的字节窗口：块大小、credit、按 offset 顺序收块与 ack。
//!
//! 这里只做纯逻辑，不读文件、不压缩、不校验 SHA-256；调用方把 `FileEnd.sha256`
//! 和落盘内容交给自己的哈希实现。协议约束见 docs/45 §7.6。

use std::collections::BTreeMap;

use crate::control::{FileAccept, FileOffer};
use crate::data::{ClipAck, ClipDataHeader, FileAck};
use crate::payload::{ClipData, FileChunk};

pub const DEFAULT_FILE_BLOCK: u32 = 1 << 20;
pub const MIN_FILE_BLOCK: u32 = 64 << 10;
pub const MAX_FILE_BLOCK: u32 = 4 << 20;
pub const DEFAULT_FILE_CREDIT: u16 = 8;
pub const MAX_FILE_BYTES: u64 = 8 << 30;
pub const MAX_FILE_BLOCKS: u64 = 8192;
pub const MAX_FILE_BITMAP_BYTES: usize = (MAX_FILE_BLOCKS as usize + 7) / 8;

pub const DEFAULT_CLIP_BLOCK: u32 = 256 << 10;
pub const DEFAULT_CLIP_CREDIT: u16 = 4;
pub const MAX_CLIP_BYTES: u32 = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferError {
    BadConfig(&'static str),
    ZeroLength,
    BeyondEnd,
    BadBlock {
        expected: u64,
        got: u64,
    },
    BadResumeOffset {
        offset: u64,
    },
    OutOfOrder {
        expected: u64,
        got: u64,
    },
    Overlap {
        expected: u64,
        got: u64,
    },
    AckBeyondSent {
        sent: u64,
        ack: u64,
    },
    SequenceMismatch {
        expected: u32,
        got: u32,
    },
    BlockMismatch {
        offset: u64,
    },
    BadBitmapLen {
        expected: usize,
        got: usize,
    },
    BadBitmapBlockCount {
        expected: u32,
        got: u32,
    },
    BadBitmapBits {
        block_count: u32,
    },
    BadBlockIndex {
        index: u32,
        count: u32,
    },
    BadDigestLen {
        field: &'static str,
        expected: usize,
        got: usize,
    },
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadConfig(s) => write!(f, "bad transfer config: {s}"),
            Self::ZeroLength => write!(f, "zero-length chunk"),
            Self::BeyondEnd => write!(f, "chunk beyond declared size"),
            Self::BadBlock { expected, got } => {
                write!(f, "bad block boundary: expected {expected}, got {got}")
            }
            Self::BadResumeOffset { offset } => {
                write!(f, "bad resume offset: {offset}")
            }
            Self::OutOfOrder { expected, got } => {
                write!(f, "out-of-order chunk: expected {expected}, got {got}")
            }
            Self::Overlap { expected, got } => {
                write!(f, "overlapping chunk: expected {expected}, got {got}")
            }
            Self::AckBeyondSent { sent, ack } => {
                write!(f, "ack {ack} beyond sent {sent}")
            }
            Self::SequenceMismatch { expected, got } => {
                write!(
                    f,
                    "clipboard sequence mismatch: expected {expected}, got {got}"
                )
            }
            Self::BlockMismatch { offset } => {
                write!(f, "duplicate block at {offset} has different bytes")
            }
            Self::BadBitmapLen { expected, got } => {
                write!(f, "bad bitmap length: expected {expected}, got {got}")
            }
            Self::BadBitmapBlockCount { expected, got } => {
                write!(f, "bad bitmap block count: expected {expected}, got {got}")
            }
            Self::BadBitmapBits { block_count } => {
                write!(f, "bitmap has set bits beyond block count {block_count}")
            }
            Self::BadBlockIndex { index, count } => {
                write!(f, "block index {index} out of range 0..{count}")
            }
            Self::BadDigestLen {
                field,
                expected,
                got,
            } => write!(
                f,
                "bad digest length for {field}: expected {expected}, got {got}"
            ),
        }
    }
}

impl std::error::Error for TransferError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileWindowConfig {
    pub size: u64,
    pub block_size: u32,
    pub credit: u16,
}

impl FileWindowConfig {
    pub fn new(size: u64) -> Result<Self, TransferError> {
        Self::with_block(size, DEFAULT_FILE_BLOCK, DEFAULT_FILE_CREDIT)
    }

    pub fn with_block(size: u64, block_size: u32, credit: u16) -> Result<Self, TransferError> {
        if !(MIN_FILE_BLOCK..=MAX_FILE_BLOCK).contains(&block_size) {
            return Err(TransferError::BadConfig(
                "file block must be 64 KiB..=4 MiB",
            ));
        }
        if credit == 0 {
            return Err(TransferError::BadConfig("file credit must be > 0"));
        }
        if size > MAX_FILE_BYTES {
            return Err(TransferError::BadConfig("file size must be <= 8 GiB"));
        }
        let blocks = size.div_ceil(u64::from(block_size));
        if blocks > MAX_FILE_BLOCKS {
            return Err(TransferError::BadConfig("file block count must be <= 8192"));
        }
        Ok(Self {
            size,
            block_size,
            credit,
        })
    }

    /// 按单程 RTT 与可用带宽估算 BDP，再折算每块大小与在途 credit。
    ///
    /// 只应在文件任务开始时调用：传输中途改变块大小会改变 offset 边界。
    pub fn adaptive(size: u64, rtt_ms: u32, available_kbps: u32) -> Result<Self, TransferError> {
        if available_kbps == 0 {
            return Err(TransferError::BadConfig("available bandwidth must be > 0"));
        }
        let bdp_bytes = u64::from(available_kbps) * u64::from(rtt_ms.max(1)) / 8;
        let target_block = (bdp_bytes / u64::from(DEFAULT_FILE_CREDIT))
            .clamp(u64::from(MIN_FILE_BLOCK), u64::from(MAX_FILE_BLOCK));
        // 4 KiB 对齐，避免无意义的奇数块。
        let block_size = ((target_block / 4096) * 4096)
            .clamp(u64::from(MIN_FILE_BLOCK), u64::from(MAX_FILE_BLOCK))
            as u32;
        let credit = bdp_bytes.div_ceil(u64::from(block_size)).clamp(2, 64) as u16;
        Self::with_block(size, block_size, credit)
    }

    pub fn block_count(&self) -> u64 {
        self.size.div_ceil(u64::from(self.block_size))
    }

    fn block_len_at(&self, offset: u64) -> u64 {
        (self.size - offset).min(u64::from(self.block_size))
    }

    pub fn offset_of_block(&self, index: u32) -> Option<u64> {
        if u64::from(index) >= self.block_count() {
            None
        } else {
            Some(u64::from(index) * u64::from(self.block_size))
        }
    }
}

/// `FileAccept.received_bitmap` 的位图：第 `i` 位为 1 表示块 `i` 已在接收方。
///
/// 位序固定为 LSB-first：`byte[i / 8] & (1 << (i % 8))`。未使用的尾位必须为 0，
/// 避免同一份位图出现多种编码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileBitmap {
    block_count: u32,
    count: u32,
    bytes: Vec<u8>,
}

impl FileBitmap {
    pub fn new(block_count: u32) -> Result<Self, TransferError> {
        if u64::from(block_count) > MAX_FILE_BLOCKS {
            return Err(TransferError::BadConfig("file block count must be <= 8192"));
        }
        let len = (block_count as usize + 7) / 8;
        Ok(Self {
            block_count,
            count: 0,
            bytes: vec![0; len],
        })
    }

    pub fn from_bytes(block_count: u32, bytes: &[u8]) -> Result<Self, TransferError> {
        let mut bitmap = Self::new(block_count)?;
        if bytes.len() != bitmap.bytes.len() {
            return Err(TransferError::BadBitmapLen {
                expected: bitmap.bytes.len(),
                got: bytes.len(),
            });
        }
        if block_count % 8 != 0 {
            let mask = (1u8 << (block_count % 8)) - 1;
            let last = *bytes.last().unwrap_or(&0);
            if last & !mask != 0 {
                return Err(TransferError::BadBitmapBits { block_count });
            }
        }
        bitmap.bytes.copy_from_slice(bytes);
        bitmap.count = bitmap.bytes.iter().map(|b| b.count_ones()).sum();
        Ok(bitmap)
    }

    pub fn block_count(&self) -> u32 {
        self.block_count
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn get(&self, index: u32) -> Result<bool, TransferError> {
        self.check(index)?;
        Ok(self.bytes[index as usize / 8] & (1 << (index % 8)) != 0)
    }

    pub fn set(&mut self, index: u32) -> Result<(), TransferError> {
        self.check(index)?;
        let mask = 1 << (index % 8);
        let byte = &mut self.bytes[index as usize / 8];
        if *byte & mask == 0 {
            *byte |= mask;
            self.count += 1;
        }
        Ok(())
    }

    pub fn clear(&mut self, index: u32) -> Result<(), TransferError> {
        self.check(index)?;
        let mask = 1 << (index % 8);
        let byte = &mut self.bytes[index as usize / 8];
        if *byte & mask != 0 {
            *byte &= !mask;
            self.count -= 1;
        }
        Ok(())
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn is_full(&self) -> bool {
        self.count == self.block_count
    }

    pub fn first_missing(&self) -> Option<u32> {
        for (byte_index, &byte) in self.bytes.iter().enumerate() {
            let missing = !byte;
            if missing == 0 {
                continue;
            }
            let bit = missing.trailing_zeros();
            let index = byte_index as u32 * 8 + bit;
            if index < self.block_count {
                return Some(index);
            }
        }
        None
    }

    fn check(&self, index: u32) -> Result<(), TransferError> {
        if index >= self.block_count {
            Err(TransferError::BadBlockIndex {
                index,
                count: self.block_count,
            })
        } else {
            Ok(())
        }
    }
}

/// 校验 `FileOffer` 的块参数与摘要长度，返回可用的文件窗口配置。
pub fn validate_file_offer(offer: &FileOffer) -> Result<FileWindowConfig, TransferError> {
    let config = FileWindowConfig::with_block(offer.size, offer.block_size, offer.credit)?;
    if !offer.sha256.is_empty() && offer.sha256.len() != 32 {
        return Err(TransferError::BadDigestLen {
            field: "FileOffer.sha256",
            expected: 32,
            got: offer.sha256.len(),
        });
    }
    if !offer.block_sha_root.is_empty() && offer.block_sha_root.len() != 32 {
        return Err(TransferError::BadDigestLen {
            field: "FileOffer.block_sha_root",
            expected: 32,
            got: offer.block_sha_root.len(),
        });
    }
    Ok(config)
}

/// 校验 `FileAccept` 的位图与块摘要表，返回解析后的位图。
pub fn validate_file_accept(
    accept: &FileAccept,
    config: &FileWindowConfig,
) -> Result<FileBitmap, TransferError> {
    if accept.resume_offset > config.size {
        return Err(TransferError::BeyondEnd);
    }
    let block_size = u64::from(config.block_size);
    if accept.resume_offset % block_size != 0 && accept.resume_offset != config.size {
        return Err(TransferError::BadBlock {
            expected: accept.resume_offset / block_size * block_size,
            got: accept.resume_offset,
        });
    }
    let block_count = config.block_count() as u32;
    let bitmap = if accept.received_bitmap.is_empty() {
        FileBitmap::new(block_count)?
    } else {
        FileBitmap::from_bytes(block_count, &accept.received_bitmap)?
    };
    if !accept.block_sha256.is_empty() {
        if accept.block_sha256.len() != block_count as usize {
            return Err(TransferError::BadDigestLen {
                field: "FileAccept.block_sha256",
                expected: block_count as usize,
                got: accept.block_sha256.len(),
            });
        }
        if let Some(bad) = accept.block_sha256.iter().find(|hash| hash.len() != 32) {
            return Err(TransferError::BadDigestLen {
                field: "FileAccept.block_sha256[]",
                expected: 32,
                got: bad.len(),
            });
        }
    }
    Ok(bitmap)
}

/// 发送侧：只有 `acked_upto` 之后的块算在途，`peer_credit` 是接收方最近一次广告的窗口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSendWindow {
    config: FileWindowConfig,
    next_offset: u64,
    acked_upto: u64,
    in_flight: u16,
    peer_credit: u16,
}

impl FileSendWindow {
    pub fn new(config: FileWindowConfig) -> Self {
        Self {
            config,
            next_offset: 0,
            acked_upto: 0,
            in_flight: 0,
            peer_credit: config.credit,
        }
    }

    pub fn config(&self) -> FileWindowConfig {
        self.config
    }

    pub fn next_offset(&self) -> u64 {
        self.next_offset
    }

    pub fn acked_upto(&self) -> u64 {
        self.acked_upto
    }

    pub fn in_flight(&self) -> u16 {
        self.in_flight
    }

    pub fn peer_credit(&self) -> u16 {
        self.peer_credit
    }

    pub fn available(&self) -> u16 {
        self.peer_credit.saturating_sub(self.in_flight)
    }

    pub fn is_complete(&self) -> bool {
        self.acked_upto >= self.config.size
    }

    /// 从已确认的前缀偏移继续发送；`offset` 必须落在块边界上，且不能超过文件尾。
    pub fn resume_at(&mut self, offset: u64) -> Result<(), TransferError> {
        if offset > self.config.size
            || (offset != self.config.size && offset % u64::from(self.config.block_size) != 0)
        {
            return Err(TransferError::BadResumeOffset { offset });
        }
        self.next_offset = offset;
        self.acked_upto = offset;
        self.in_flight = 0;
        self.peer_credit = self.config.credit;
        Ok(())
    }

    /// 取下一块；返回 `(offset, len)`。窗口满或已发完时返回 `None`。
    pub fn next_chunk(&mut self) -> Option<(u64, u32)> {
        if self.next_offset >= self.config.size || self.available() == 0 {
            return None;
        }
        let offset = self.next_offset;
        let len = self.config.block_len_at(offset);
        self.next_offset += len;
        self.in_flight += 1;
        Some((offset, len as u32))
    }

    /// 接收方 ack；`credit` 是它当前愿意接收的未确认块数。
    pub fn on_ack(&mut self, ack: FileAck) -> Result<(), TransferError> {
        if ack.received_upto > self.next_offset {
            return Err(TransferError::AckBeyondSent {
                sent: self.next_offset,
                ack: ack.received_upto,
            });
        }
        if ack.received_upto > self.acked_upto {
            self.acked_upto = ack.received_upto;
        }
        let unacked_bytes = self.next_offset - self.acked_upto;
        self.in_flight = unacked_bytes
            .div_ceil(u64::from(self.config.block_size))
            .min(u64::from(u16::MAX)) as u16;
        self.peer_credit = ack.credit;
        Ok(())
    }
}

/// 接收侧：只接受与 `received_upto` 对齐的块；重复块回 ack，不重复推进。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecvWindow {
    config: FileWindowConfig,
    received_upto: u64,
    credit: u16,
}

impl FileRecvWindow {
    pub fn new(config: FileWindowConfig) -> Self {
        Self {
            config,
            received_upto: 0,
            credit: config.credit,
        }
    }

    pub fn config(&self) -> FileWindowConfig {
        self.config
    }

    pub fn received_upto(&self) -> u64 {
        self.received_upto
    }

    pub fn expected_offset(&self) -> u64 {
        self.received_upto
    }

    pub fn is_complete(&self) -> bool {
        self.received_upto >= self.config.size
    }

    /// 从已经校验过的前缀继续接收；`offset` 必须是块边界。
    pub fn resume_at(&mut self, offset: u64) -> Result<(), TransferError> {
        if offset > self.config.size
            || (offset != self.config.size && offset % u64::from(self.config.block_size) != 0)
        {
            return Err(TransferError::BadResumeOffset { offset });
        }
        self.received_upto = offset;
        Ok(())
    }

    /// 接收一个块并返回应回给发送方的 ack。
    pub fn accept(&mut self, offset: u64, len: u32) -> Result<FileAck, TransferError> {
        if len == 0 {
            return Err(TransferError::ZeroLength);
        }
        let end = offset
            .checked_add(u64::from(len))
            .ok_or(TransferError::BeyondEnd)?;
        if end > self.config.size {
            return Err(TransferError::BeyondEnd);
        }
        let expected_len = self.config.block_len_at(offset);
        if expected_len != u64::from(len) {
            return Err(TransferError::BadBlock {
                expected: expected_len,
                got: u64::from(len),
            });
        }
        if offset < self.received_upto {
            if end <= self.received_upto {
                return Ok(self.ack());
            }
            return Err(TransferError::Overlap {
                expected: self.received_upto,
                got: offset,
            });
        }
        if offset > self.received_upto {
            return Err(TransferError::OutOfOrder {
                expected: self.received_upto,
                got: offset,
            });
        }
        self.received_upto = end;
        Ok(self.ack())
    }

    pub fn ack(&self) -> FileAck {
        FileAck {
            received_upto: self.received_upto,
            credit: self.credit,
        }
    }
}

/// 稀疏发送窗口：从 `FileAccept.received_bitmap` 出发，跳过接收方已有的块，
/// 允许按块乱序发送；`FileAck` 仍只确认连续前缀。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSparseSendWindow {
    config: FileWindowConfig,
    acked: FileBitmap,
    sent: FileBitmap,
    in_flight: u16,
    peer_credit: u16,
}

impl FileSparseSendWindow {
    pub fn new(config: FileWindowConfig, acked: FileBitmap) -> Result<Self, TransferError> {
        let expected = config.block_count() as u32;
        if acked.block_count() != expected {
            return Err(TransferError::BadBitmapBlockCount {
                expected,
                got: acked.block_count(),
            });
        }
        Ok(Self {
            config,
            acked,
            sent: FileBitmap::new(expected)?,
            in_flight: 0,
            peer_credit: config.credit,
        })
    }

    pub fn config(&self) -> FileWindowConfig {
        self.config
    }

    pub fn available(&self) -> u16 {
        self.peer_credit.saturating_sub(self.in_flight)
    }

    pub fn in_flight(&self) -> u16 {
        self.in_flight
    }

    pub fn is_complete(&self) -> bool {
        self.acked.is_full()
    }

    pub fn bitmap(&self) -> &FileBitmap {
        &self.acked
    }

    /// 取下一个尚未确认、尚未发送的块；窗口满时返回 `None`。
    pub fn next_chunk(&mut self) -> Option<(u64, u32)> {
        if self.is_complete() || self.available() == 0 {
            return None;
        }
        let count = self.config.block_count() as u32;
        for index in 0..count {
            if self.acked.get(index).ok()? || self.sent.get(index).ok()? {
                continue;
            }
            let offset = self.config.offset_of_block(index)?;
            let len = self.config.block_len_at(offset) as u32;
            self.sent.set(index).ok()?;
            self.in_flight += 1;
            return Some((offset, len));
        }
        None
    }

    /// 接收方只确认连续前缀；非前缀的已发送块仍留在 `in_flight`。
    pub fn on_ack(&mut self, ack: FileAck) -> Result<(), TransferError> {
        if ack.received_upto > self.config.size {
            return Err(TransferError::AckBeyondSent {
                sent: self.config.size,
                ack: ack.received_upto,
            });
        }
        if ack.received_upto != self.config.size
            && ack.received_upto % u64::from(self.config.block_size) != 0
        {
            return Err(TransferError::BadBlock {
                expected: ack.received_upto / u64::from(self.config.block_size)
                    * u64::from(self.config.block_size),
                got: ack.received_upto,
            });
        }
        let count = self.config.block_count() as u32;
        for index in 0..count {
            let offset = self.config.offset_of_block(index).unwrap();
            if offset + self.config.block_len_at(offset) <= ack.received_upto {
                self.acked.set(index)?;
            }
        }
        let pending = (0..count)
            .filter(|index| {
                self.sent.get(*index).unwrap_or(false) && !self.acked.get(*index).unwrap_or(false)
            })
            .count();
        self.in_flight = pending.min(usize::from(u16::MAX)) as u16;
        self.peer_credit = ack.credit;
        Ok(())
    }
}

/// 稀疏接收窗口：保留任意顺序到达的块，但 `FileAck` 只累计连续前缀。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSparseRecvWindow {
    config: FileWindowConfig,
    received: FileBitmap,
    received_upto: u64,
    credit: u16,
}

impl FileSparseRecvWindow {
    pub fn new(config: FileWindowConfig, received: FileBitmap) -> Result<Self, TransferError> {
        let expected = config.block_count() as u32;
        if received.block_count() != expected {
            return Err(TransferError::BadBitmapBlockCount {
                expected,
                got: received.block_count(),
            });
        }
        let mut window = Self {
            config,
            received,
            received_upto: 0,
            credit: config.credit,
        };
        window.advance();
        Ok(window)
    }

    pub fn config(&self) -> FileWindowConfig {
        self.config
    }

    pub fn received_upto(&self) -> u64 {
        self.received_upto
    }

    pub fn is_complete(&self) -> bool {
        self.received.is_full()
    }

    pub fn bitmap(&self) -> &FileBitmap {
        &self.received
    }

    /// 接受任意块；重复块幂等，空洞不会推进 `received_upto`。
    pub fn accept(&mut self, offset: u64, len: u32) -> Result<FileAck, TransferError> {
        if len == 0 {
            return Err(TransferError::ZeroLength);
        }
        let end = offset
            .checked_add(u64::from(len))
            .ok_or(TransferError::BeyondEnd)?;
        if end > self.config.size {
            return Err(TransferError::BeyondEnd);
        }
        if offset % u64::from(self.config.block_size) != 0 {
            return Err(TransferError::BadBlock {
                expected: offset / u64::from(self.config.block_size)
                    * u64::from(self.config.block_size),
                got: offset,
            });
        }
        let expected_len = self.config.block_len_at(offset);
        if expected_len != u64::from(len) {
            return Err(TransferError::BadBlock {
                expected: expected_len,
                got: u64::from(len),
            });
        }
        let index = (offset / u64::from(self.config.block_size)) as u32;
        self.received.set(index)?;
        self.advance();
        Ok(self.ack())
    }

    pub fn ack(&self) -> FileAck {
        FileAck {
            received_upto: self.received_upto,
            credit: self.credit,
        }
    }

    fn advance(&mut self) {
        while self.received_upto < self.config.size {
            let index = (self.received_upto / u64::from(self.config.block_size)) as u32;
            if !self.received.get(index).unwrap_or(false) {
                break;
            }
            self.received_upto += self.config.block_len_at(self.received_upto);
        }
    }
}

/// 以一段内存源驱动 [`FileSendWindow`]；把窗口的 `(offset,len)` 转成线上
/// `FileChunk`。真正的文件读取、zstd 压缩和 SHA-256 由调用方在更外层完成。
#[derive(Debug, Clone)]
pub struct FileSendCursor<'a> {
    window: FileSendWindow,
    data: &'a [u8],
}

impl<'a> FileSendCursor<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, TransferError> {
        let config = FileWindowConfig::new(data.len() as u64)?;
        Self::with_config(data, config)
    }

    pub fn with_config(data: &'a [u8], config: FileWindowConfig) -> Result<Self, TransferError> {
        if config.size != data.len() as u64 {
            return Err(TransferError::BadConfig("source length != window size"));
        }
        Ok(Self {
            window: FileSendWindow::new(config),
            data,
        })
    }

    pub fn window(&self) -> &FileSendWindow {
        &self.window
    }

    pub fn next_chunk(&mut self) -> Option<FileChunk> {
        let (offset, len) = self.window.next_chunk()?;
        let start = offset as usize;
        let end = start + len as usize;
        Some(FileChunk {
            offset,
            data: self.data[start..end].to_vec(),
        })
    }

    pub fn on_ack(&mut self, ack: FileAck) -> Result<(), TransferError> {
        self.window.on_ack(ack)
    }

    pub fn is_complete(&self) -> bool {
        self.window.is_complete()
    }
}

/// 接收侧的内存落地缓冲：按 offset 保存块，重复块校验字节一致后幂等返回 ack。
#[derive(Debug, Clone)]
pub struct FileRecvBuffer {
    window: FileRecvWindow,
    blocks: BTreeMap<u64, Vec<u8>>,
}

impl FileRecvBuffer {
    pub fn new(config: FileWindowConfig) -> Self {
        Self {
            window: FileRecvWindow::new(config),
            blocks: BTreeMap::new(),
        }
    }

    pub fn window(&self) -> &FileRecvWindow {
        &self.window
    }

    pub fn accept(&mut self, chunk: &FileChunk) -> Result<FileAck, TransferError> {
        let len = chunk.data.len() as u32;
        let ack = self.window.accept(chunk.offset, len)?;
        if let Some(existing) = self.blocks.get(&chunk.offset) {
            if existing != &chunk.data {
                return Err(TransferError::BlockMismatch {
                    offset: chunk.offset,
                });
            }
        } else {
            self.blocks.insert(chunk.offset, chunk.data.clone());
        }
        Ok(ack)
    }

    pub fn is_complete(&self) -> bool {
        self.window.is_complete()
    }

    /// 收齐后按 offset 拼成完整字节串；未收齐返回 `None`。
    pub fn assemble(&self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }
        let mut out = Vec::new();
        for data in self.blocks.values() {
            out.extend_from_slice(data);
        }
        if out.len() as u64 == self.window.config().size {
            Some(out)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipWindowConfig {
    pub seq: u32,
    pub size: u32,
    pub block_size: u32,
    pub credit: u16,
}

impl ClipWindowConfig {
    pub fn new(seq: u32, size: u32) -> Result<Self, TransferError> {
        if size > MAX_CLIP_BYTES {
            return Err(TransferError::BadConfig("clipboard item must be <= 16 MiB"));
        }
        if size == 0 {
            return Err(TransferError::ZeroLength);
        }
        Ok(Self {
            seq,
            size,
            block_size: DEFAULT_CLIP_BLOCK,
            credit: DEFAULT_CLIP_CREDIT,
        })
    }

    fn block_len_at(&self, offset: u32) -> u32 {
        (self.size - offset).min(self.block_size)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipSendWindow {
    config: ClipWindowConfig,
    next_offset: u32,
    acked_upto: u32,
    in_flight: u16,
}

impl ClipSendWindow {
    pub fn new(config: ClipWindowConfig) -> Self {
        Self {
            config,
            next_offset: 0,
            acked_upto: 0,
            in_flight: 0,
        }
    }

    pub fn config(&self) -> ClipWindowConfig {
        self.config
    }

    pub fn next_offset(&self) -> u32 {
        self.next_offset
    }

    pub fn acked_upto(&self) -> u32 {
        self.acked_upto
    }

    pub fn is_complete(&self) -> bool {
        self.acked_upto >= self.config.size
    }

    /// 取下一块；返回带 seq/format/offset 的头和块长度。
    pub fn next_chunk(&mut self, format: u16) -> Option<(ClipDataHeader, u32)> {
        if self.next_offset >= self.config.size || self.in_flight >= self.config.credit {
            return None;
        }
        let offset = self.next_offset;
        let len = self.config.block_len_at(offset);
        self.next_offset += len;
        self.in_flight += 1;
        Some((
            ClipDataHeader {
                seq: self.config.seq,
                format,
                offset,
            },
            len,
        ))
    }

    pub fn on_ack(&mut self, ack: ClipAck) -> Result<(), TransferError> {
        if ack.seq != self.config.seq {
            return Err(TransferError::SequenceMismatch {
                expected: self.config.seq,
                got: ack.seq,
            });
        }
        if ack.received_upto > self.next_offset {
            return Err(TransferError::AckBeyondSent {
                sent: u64::from(self.next_offset),
                ack: u64::from(ack.received_upto),
            });
        }
        if ack.received_upto > self.acked_upto {
            self.acked_upto = ack.received_upto;
        }
        self.in_flight = (self.next_offset - self.acked_upto)
            .div_ceil(self.config.block_size)
            .min(u32::from(u16::MAX)) as u16;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipRecvWindow {
    config: ClipWindowConfig,
    received_upto: u32,
}

impl ClipRecvWindow {
    pub fn new(config: ClipWindowConfig) -> Self {
        Self {
            config,
            received_upto: 0,
        }
    }

    pub fn config(&self) -> ClipWindowConfig {
        self.config
    }

    pub fn received_upto(&self) -> u32 {
        self.received_upto
    }

    pub fn is_complete(&self) -> bool {
        self.received_upto >= self.config.size
    }

    pub fn accept(&mut self, header: ClipDataHeader, len: u32) -> Result<ClipAck, TransferError> {
        if header.seq != self.config.seq {
            return Err(TransferError::SequenceMismatch {
                expected: self.config.seq,
                got: header.seq,
            });
        }
        if len == 0 {
            return Err(TransferError::ZeroLength);
        }
        let end = header
            .offset
            .checked_add(len)
            .ok_or(TransferError::BeyondEnd)?;
        if end > self.config.size {
            return Err(TransferError::BeyondEnd);
        }
        let expected_len = self.config.block_len_at(header.offset);
        if expected_len != len {
            return Err(TransferError::BadBlock {
                expected: u64::from(expected_len),
                got: u64::from(len),
            });
        }
        if header.offset < self.received_upto {
            if end <= self.received_upto {
                return Ok(self.ack());
            }
            return Err(TransferError::Overlap {
                expected: u64::from(self.received_upto),
                got: u64::from(header.offset),
            });
        }
        if header.offset > self.received_upto {
            return Err(TransferError::OutOfOrder {
                expected: u64::from(self.received_upto),
                got: u64::from(header.offset),
            });
        }
        self.received_upto = end;
        Ok(self.ack())
    }

    pub fn ack(&self) -> ClipAck {
        ClipAck {
            seq: self.config.seq,
            received_upto: self.received_upto,
            credit: self.config.credit,
        }
    }
}

/// 以一段内存剪贴板内容驱动 [`ClipSendWindow`]，产出带 `seq/format/offset`
/// 的 `ClipData`。
#[derive(Debug, Clone)]
pub struct ClipSendCursor<'a> {
    window: ClipSendWindow,
    data: &'a [u8],
    format: u16,
}

impl<'a> ClipSendCursor<'a> {
    pub fn new(seq: u32, format: u16, data: &'a [u8]) -> Result<Self, TransferError> {
        let config = ClipWindowConfig::new(seq, data.len() as u32)?;
        Ok(Self {
            window: ClipSendWindow::new(config),
            data,
            format,
        })
    }

    pub fn window(&self) -> &ClipSendWindow {
        &self.window
    }

    pub fn next_chunk(&mut self) -> Option<ClipData> {
        let (header, len) = self.window.next_chunk(self.format)?;
        let start = header.offset as usize;
        let end = start + len as usize;
        Some(ClipData {
            header,
            data: self.data[start..end].to_vec(),
        })
    }

    pub fn on_ack(&mut self, ack: ClipAck) -> Result<(), TransferError> {
        self.window.on_ack(ack)
    }

    pub fn is_complete(&self) -> bool {
        self.window.is_complete()
    }
}

/// 剪贴板接收缓冲：按 offset 保存块，重复块校验字节后幂等 ack。
#[derive(Debug, Clone)]
pub struct ClipRecvBuffer {
    window: ClipRecvWindow,
    blocks: BTreeMap<u32, Vec<u8>>,
}

impl ClipRecvBuffer {
    pub fn new(config: ClipWindowConfig) -> Self {
        Self {
            window: ClipRecvWindow::new(config),
            blocks: BTreeMap::new(),
        }
    }

    pub fn window(&self) -> &ClipRecvWindow {
        &self.window
    }

    pub fn accept(&mut self, chunk: &ClipData) -> Result<ClipAck, TransferError> {
        let len = chunk.data.len() as u32;
        let ack = self.window.accept(chunk.header, len)?;
        if let Some(existing) = self.blocks.get(&chunk.header.offset) {
            if existing != &chunk.data {
                return Err(TransferError::BlockMismatch {
                    offset: u64::from(chunk.header.offset),
                });
            }
        } else {
            self.blocks.insert(chunk.header.offset, chunk.data.clone());
        }
        Ok(ack)
    }

    pub fn is_complete(&self) -> bool {
        self.window.is_complete()
    }

    pub fn assemble(&self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }
        let mut out = Vec::new();
        for data in self.blocks.values() {
            out.extend_from_slice(data);
        }
        if out.len() as u32 == self.window.config().size {
            Some(out)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_send_respects_credit_and_ack() {
        let cfg = FileWindowConfig::with_block(150_000, 64 << 10, 2).unwrap();
        let mut send = FileSendWindow::new(cfg);
        assert_eq!(send.next_chunk(), Some((0, 65_536)));
        assert_eq!(send.next_chunk(), Some((65_536, 65_536)));
        assert_eq!(send.next_chunk(), None);
        send.on_ack(FileAck {
            received_upto: 65_536,
            credit: 2,
        })
        .unwrap();
        assert_eq!(send.in_flight(), 1);
        assert_eq!(send.next_chunk(), Some((131_072, 18_928)));
        assert_eq!(send.next_chunk(), None);
        send.on_ack(FileAck {
            received_upto: 150_000,
            credit: 2,
        })
        .unwrap();
        assert!(send.is_complete());
    }

    #[test]
    fn file_recv_orders_duplicates_and_gaps() {
        let cfg = FileWindowConfig::with_block(150_000, 64 << 10, 8).unwrap();
        let mut recv = FileRecvWindow::new(cfg);
        assert_eq!(recv.accept(0, 65_536).unwrap().received_upto, 65_536);
        assert_eq!(recv.accept(0, 65_536).unwrap().received_upto, 65_536);
        assert_eq!(
            recv.accept(131_072, 18_928),
            Err(TransferError::OutOfOrder {
                expected: 65_536,
                got: 131_072
            })
        );
        assert_eq!(recv.accept(65_536, 65_536).unwrap().received_upto, 131_072);
        assert_eq!(recv.accept(131_072, 18_928).unwrap().received_upto, 150_000);
        assert!(recv.is_complete());
    }

    #[test]
    fn file_config_bounds() {
        assert!(FileWindowConfig::with_block(MAX_FILE_BYTES, 1 << 20, 8).is_ok());
        assert!(FileWindowConfig::with_block(MAX_FILE_BYTES + 1, 1 << 20, 8).is_err());
        assert!(FileWindowConfig::with_block(MAX_FILE_BYTES, 64 << 10, 8).is_err());
        assert!(FileWindowConfig::with_block(1, 1 << 20, 0).is_err());
    }

    #[test]
    fn adaptive_file_window_scales_with_bdp() {
        let low = FileWindowConfig::adaptive(8 << 20, 10, 1_000).unwrap();
        assert_eq!(low.block_size, MIN_FILE_BLOCK);
        assert_eq!(low.credit, 2);

        let high = FileWindowConfig::adaptive(8 << 20, 100, 100_000).unwrap();
        assert!(high.block_size > MIN_FILE_BLOCK);
        assert!(high.block_size <= MAX_FILE_BLOCK);
        assert!(high.credit > 2);
        assert!(high.credit <= 64);
        assert!(high.block_count() <= MAX_FILE_BLOCKS);
    }

    #[test]
    fn file_cursor_and_buffer_roundtrip_source() {
        let data: Vec<u8> = (0..130_000).map(|i| (i % 251) as u8).collect();
        let cfg = FileWindowConfig::with_block(data.len() as u64, 64 << 10, 8).unwrap();
        let mut cursor = FileSendCursor::with_config(&data, cfg).unwrap();
        let mut recv = FileRecvBuffer::new(cfg);

        let mut first = None;
        while let Some(chunk) = cursor.next_chunk() {
            if first.is_none() {
                first = Some(chunk.clone());
            }
            let ack = recv.accept(&chunk).unwrap();
            cursor.on_ack(ack).unwrap();
        }
        assert!(cursor.is_complete());
        assert!(recv.is_complete());
        assert_eq!(recv.assemble().unwrap(), data);

        let mut tampered = first.expect("at least one chunk");
        tampered.data[0] ^= 0xFF;
        assert_eq!(
            recv.accept(&tampered),
            Err(TransferError::BlockMismatch { offset: 0 })
        );
    }

    #[test]
    fn clip_send_and_recv_sequence() {
        let cfg = ClipWindowConfig::new(7, 300_000).unwrap();
        let mut send = ClipSendWindow::new(cfg);
        let mut recv = ClipRecvWindow::new(cfg);
        while let Some((head, len)) = send.next_chunk(0) {
            let ack = recv.accept(head, len).unwrap();
            send.on_ack(ack).unwrap();
        }
        assert!(send.is_complete());
        assert!(recv.is_complete());
        assert_eq!(
            recv.accept(
                ClipDataHeader {
                    seq: 8,
                    format: 0,
                    offset: 0
                },
                1
            ),
            Err(TransferError::SequenceMismatch {
                expected: 7,
                got: 8
            })
        );
    }

    #[test]
    fn clip_cursor_and_buffer_roundtrip_source() {
        let data: Vec<u8> = (0..600_000).map(|i| (i % 239) as u8).collect();
        let config = ClipWindowConfig::new(9, data.len() as u32).unwrap();
        let mut cursor = ClipSendCursor::new(9, 3, &data).unwrap();
        let mut recv = ClipRecvBuffer::new(config);

        let mut first = None;
        while let Some(chunk) = cursor.next_chunk() {
            if first.is_none() {
                first = Some(chunk.clone());
            }
            let ack = recv.accept(&chunk).unwrap();
            cursor.on_ack(ack).unwrap();
        }
        assert!(cursor.is_complete());
        assert!(recv.is_complete());
        assert_eq!(recv.assemble().unwrap(), data);

        let mut tampered = first.expect("at least one chunk");
        tampered.data[0] ^= 0xFF;
        assert_eq!(
            recv.accept(&tampered),
            Err(TransferError::BlockMismatch { offset: 0 })
        );
    }

    #[test]
    fn sparse_windows_skip_known_blocks_and_keep_out_of_order_blocks() {
        const BLOCK: u32 = 64 << 10;
        let config = FileWindowConfig::with_block(u64::from(BLOCK) * 6, BLOCK, 8).unwrap();
        let mut initial = FileBitmap::new(6).unwrap();
        initial.set(1).unwrap();
        initial.set(3).unwrap();

        let mut send = FileSparseSendWindow::new(config, initial.clone()).unwrap();
        let mut recv = FileSparseRecvWindow::new(config, initial).unwrap();
        assert_eq!(recv.received_upto(), 0);

        let mut chunks = Vec::new();
        while let Some(chunk) = send.next_chunk() {
            chunks.push(chunk);
        }
        assert_eq!(
            chunks.iter().map(|(offset, _)| *offset).collect::<Vec<_>>(),
            vec![
                0,
                2 * u64::from(BLOCK),
                4 * u64::from(BLOCK),
                5 * u64::from(BLOCK)
            ]
        );

        // 先到 5，再补 2、0、4；ack 只随连续前缀推进。
        let ack = recv.accept(chunks[3].0, chunks[3].1).unwrap();
        assert_eq!(ack.received_upto, 0);
        send.on_ack(ack).unwrap();
        assert_eq!(send.in_flight(), 4);

        let ack = recv.accept(chunks[1].0, chunks[1].1).unwrap();
        assert_eq!(ack.received_upto, 0);
        send.on_ack(ack).unwrap();

        let ack = recv.accept(chunks[0].0, chunks[0].1).unwrap();
        assert_eq!(ack.received_upto, 4 * u64::from(BLOCK));
        send.on_ack(ack).unwrap();

        let ack = recv.accept(chunks[2].0, chunks[2].1).unwrap();
        assert_eq!(ack.received_upto, 6 * u64::from(BLOCK));
        send.on_ack(ack).unwrap();
        assert!(recv.is_complete());
        assert!(send.is_complete());
        assert_eq!(send.in_flight(), 0);

        // 重复块幂等。
        let duplicate = recv.accept(chunks[0].0, chunks[0].1).unwrap();
        assert_eq!(duplicate.received_upto, 6 * u64::from(BLOCK));
    }

    #[test]
    fn file_bitmap_is_lsb_first_and_rejects_padding_bits() {
        let mut bitmap = FileBitmap::new(5).unwrap();
        assert_eq!(bitmap.as_bytes(), &[0]);
        bitmap.set(0).unwrap();
        bitmap.set(2).unwrap();
        assert_eq!(bitmap.as_bytes(), &[0b0000_0101]);
        assert_eq!(bitmap.count(), 2);
        assert_eq!(bitmap.first_missing(), Some(1));
        assert!(!bitmap.is_full());
        assert_eq!(
            bitmap.get(5),
            Err(TransferError::BadBlockIndex { index: 5, count: 5 })
        );

        assert_eq!(
            FileBitmap::from_bytes(5, &[0b1000_0000]),
            Err(TransferError::BadBitmapBits { block_count: 5 })
        );
        assert_eq!(
            FileBitmap::from_bytes(9, &[0]),
            Err(TransferError::BadBitmapLen {
                expected: 2,
                got: 1
            })
        );
        assert_eq!(
            FileBitmap::from_bytes(5, &[0b0000_0101])
                .unwrap()
                .into_bytes(),
            vec![0b0000_0101]
        );
    }

    #[test]
    fn first_missing_scans_across_byte_boundaries() {
        let mut bitmap = FileBitmap::new(17).unwrap();
        for index in 0..8 {
            bitmap.set(index).unwrap();
        }
        assert_eq!(bitmap.first_missing(), Some(8));
        for index in 8..16 {
            bitmap.set(index).unwrap();
        }
        assert_eq!(bitmap.first_missing(), Some(16));
        bitmap.set(16).unwrap();
        assert_eq!(bitmap.first_missing(), None);
        assert!(bitmap.is_full());
        assert_eq!(bitmap.count(), 17);
    }

    #[test]
    fn file_offer_carries_credit_into_window() {
        use crate::control::FileOffer;

        let offer = FileOffer {
            task_id: 1,
            dir: String::new(),
            name: "a.bin".into(),
            size: 150_000,
            sha256: Vec::new(),
            block_size: 64 << 10,
            credit: 3,
            block_sha_root: Vec::new(),
            mtime_ms: 0,
            conflict: 1,
            compress: false,
            resume: false,
            extensions: Vec::new(),
        };
        let config = validate_file_offer(&offer).unwrap();
        assert_eq!(config.credit, 3);
    }

    #[test]
    fn ack_keeps_receiver_credit() {
        let config = FileWindowConfig::with_block(150_000, 64 << 10, 3).unwrap();
        let mut send = FileSendWindow::new(config);
        let mut recv = FileRecvWindow::new(config);
        assert!(send.next_chunk().is_some());
        let ack = recv.accept(0, 64 << 10).unwrap();
        assert_eq!(ack.credit, 3);
        send.on_ack(ack).unwrap();
        assert_eq!(send.peer_credit(), 3);
    }

    #[test]
    fn file_offer_and_accept_validate_digests_and_bitmap() {
        use crate::control::{FileAccept, FileOffer};

        let offer = FileOffer {
            task_id: 1,
            dir: String::new(),
            name: "a.bin".into(),
            size: 300_000,
            sha256: vec![1; 32],
            block_size: 64 << 10,
            credit: DEFAULT_FILE_CREDIT,
            block_sha_root: vec![2; 32],
            mtime_ms: 0,
            conflict: 1,
            compress: false,
            resume: true,
            extensions: Vec::new(),
        };
        let config = validate_file_offer(&offer).unwrap();
        assert_eq!(config.block_count(), 5);

        let mut bad_offer = offer.clone();
        bad_offer.block_sha_root = vec![2; 31];
        assert_eq!(
            validate_file_offer(&bad_offer),
            Err(TransferError::BadDigestLen {
                field: "FileOffer.block_sha_root",
                expected: 32,
                got: 31
            })
        );

        let accept = FileAccept {
            task_id: 1,
            resume_offset: 0,
            received_bitmap: vec![0b0000_0101],
            block_sha256: vec![vec![3; 32]; 5],
            final_name: None,
            error: None,
            extensions: Vec::new(),
        };
        let bitmap = validate_file_accept(&accept, &config).unwrap();
        assert_eq!(bitmap.count(), 2);
        assert_eq!(bitmap.first_missing(), Some(1));

        let mut bad_accept = accept.clone();
        bad_accept.received_bitmap = vec![0b0000_0101, 0];
        assert_eq!(
            validate_file_accept(&bad_accept, &config),
            Err(TransferError::BadBitmapLen {
                expected: 1,
                got: 2
            })
        );

        let mut bad_hashes = accept;
        bad_hashes.block_sha256.pop();
        assert_eq!(
            validate_file_accept(&bad_hashes, &config),
            Err(TransferError::BadDigestLen {
                field: "FileAccept.block_sha256",
                expected: 5,
                got: 4
            })
        );
    }
}
