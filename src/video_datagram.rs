//! 视频 DATAGRAM 分片与重组。
//!
//! 视频走 DATAGRAM 时，单个 datagram 限制在 1100 B 以内；本模块只负责
//! 分片头、分片顺序和重组，不解码视频，也不依赖 QUIC 类型。真正的
//! `send_datagram`/`recv_datagram` 由 `Connection` 实现。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::codec::Codec;

/// 单个视频 DATAGRAM 的硬上限，含 20 B 分片头。
pub const VIDEO_DATAGRAM_MTU: usize = 1100;
/// 分片头长度：frame_id(4) + frag_index(2) + frag_count(2) + flags(2)
/// + codec(1) + reserved(1) + pts_us(8)。
pub const VIDEO_DATAGRAM_HEADER: usize = 20;
/// 单个分片可携带的视频字节数。
pub const VIDEO_DATAGRAM_PAYLOAD: usize = VIDEO_DATAGRAM_MTU - VIDEO_DATAGRAM_HEADER;
/// 单帧最多 1024 片，约 1.05 MiB；超出应走可靠视频流。
pub const MAX_VIDEO_FRAGMENTS: u16 = 1024;
/// 分片头 flags 的 KEY 位。
pub const VIDEO_DATAGRAM_KEY: u16 = 1 << 0;
/// 分片头 flags 的 FEC parity 位；`frag_index` 此时表示 parity group 序号，
/// `reserved` 表示 group 大小。
pub const VIDEO_DATAGRAM_PARITY: u16 = 1 << 1;
/// 默认每 16 个数据分片生成 1 个 XOR parity 分片。
pub const VIDEO_DATAGRAM_FEC_GROUP: u8 = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoDatagramHeader {
    pub frame_id: u32,
    pub frag_index: u16,
    pub frag_count: u16,
    pub flags: u16,
    pub codec: u8,
    pub reserved: u8,
    pub pts_us: u64,
}

impl VideoDatagramHeader {
    pub fn is_key(&self) -> bool {
        self.flags & VIDEO_DATAGRAM_KEY != 0
    }

    pub fn is_parity(&self) -> bool {
        self.flags & VIDEO_DATAGRAM_PARITY != 0
    }

    pub fn encode(&self) -> [u8; VIDEO_DATAGRAM_HEADER] {
        let mut out = [0u8; VIDEO_DATAGRAM_HEADER];
        out[0..4].copy_from_slice(&self.frame_id.to_be_bytes());
        out[4..6].copy_from_slice(&self.frag_index.to_be_bytes());
        out[6..8].copy_from_slice(&self.frag_count.to_be_bytes());
        out[8..10].copy_from_slice(&self.flags.to_be_bytes());
        out[10] = self.codec;
        out[11] = self.reserved;
        out[12..20].copy_from_slice(&self.pts_us.to_be_bytes());
        out
    }

    pub fn decode(data: &[u8]) -> Result<Self, VideoDatagramError> {
        if data.len() < VIDEO_DATAGRAM_HEADER {
            return Err(VideoDatagramError::ShortHeader);
        }
        let mut frame_id = [0u8; 4];
        frame_id.copy_from_slice(&data[0..4]);
        let mut frag_index = [0u8; 2];
        frag_index.copy_from_slice(&data[4..6]);
        let mut frag_count = [0u8; 2];
        frag_count.copy_from_slice(&data[6..8]);
        let mut flags = [0u8; 2];
        flags.copy_from_slice(&data[8..10]);
        let mut pts_us = [0u8; 8];
        pts_us.copy_from_slice(&data[12..20]);
        let header = Self {
            frame_id: u32::from_be_bytes(frame_id),
            frag_index: u16::from_be_bytes(frag_index),
            frag_count: u16::from_be_bytes(frag_count),
            flags: u16::from_be_bytes(flags),
            codec: data[10],
            reserved: data[11],
            pts_us: u64::from_be_bytes(pts_us),
        };
        header.validate()?;
        Ok(header)
    }

    fn validate(&self) -> Result<(), VideoDatagramError> {
        if self.frag_count == 0 || self.frag_count > MAX_VIDEO_FRAGMENTS {
            return Err(VideoDatagramError::BadFragmentCount(self.frag_count));
        }
        if self.frag_index >= self.frag_count {
            return Err(VideoDatagramError::BadFragmentIndex {
                index: self.frag_index,
                count: self.frag_count,
            });
        }
        if self.is_parity() {
            let group = u16::from(self.reserved);
            if !(2..=64).contains(&group) {
                return Err(VideoDatagramError::BadFecGroup(self.reserved));
            }
            if u32::from(self.frag_index) >= u32::from(self.frag_count) / u32::from(group) {
                return Err(VideoDatagramError::BadFecGroup(self.reserved));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoDatagram {
    pub header: VideoDatagramHeader,
    pub payload: Vec<u8>,
}

impl VideoDatagram {
    pub fn encode(&self) -> Result<Vec<u8>, VideoDatagramError> {
        let total = VIDEO_DATAGRAM_HEADER + self.payload.len();
        if total > VIDEO_DATAGRAM_MTU {
            return Err(VideoDatagramError::TooLarge(total));
        }
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.payload);
        Ok(out)
    }

    pub fn decode(data: &[u8]) -> Result<Self, VideoDatagramError> {
        if data.len() > VIDEO_DATAGRAM_MTU {
            return Err(VideoDatagramError::TooLarge(data.len()));
        }
        let header = VideoDatagramHeader::decode(data)?;
        Ok(Self {
            header,
            payload: data[VIDEO_DATAGRAM_HEADER..].to_vec(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoDatagramFrame {
    pub frame_id: u32,
    pub key: bool,
    pub codec: u8,
    pub pts_us: u64,
    pub payload: Vec<u8>,
}

/// 把一帧视频切成不超过 1100 B 的 datagram。
pub fn fragment_video(
    frame_id: u32,
    pts_us: u64,
    key: bool,
    codec: Codec,
    payload: &[u8],
) -> Result<Vec<VideoDatagram>, VideoDatagramError> {
    let count = if payload.is_empty() {
        1
    } else {
        payload.len().div_ceil(VIDEO_DATAGRAM_PAYLOAD)
    };
    if count > MAX_VIDEO_FRAGMENTS as usize {
        return Err(VideoDatagramError::FrameTooLarge(payload.len()));
    }
    let frag_count = count as u16;
    let flags = if key { VIDEO_DATAGRAM_KEY } else { 0 };
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let begin = index * VIDEO_DATAGRAM_PAYLOAD;
        let end = (begin + VIDEO_DATAGRAM_PAYLOAD).min(payload.len());
        out.push(VideoDatagram {
            header: VideoDatagramHeader {
                frame_id,
                frag_index: index as u16,
                frag_count,
                flags,
                codec: codec.id(),
                reserved: 0,
                pts_us,
            },
            payload: payload[begin..end].to_vec(),
        });
    }
    Ok(out)
}

/// 在 `fragment_video` 的数据分片后追加 XOR FEC parity。
///
/// 只保护完整 group；最后一组不足 `group_size` 的尾片不生成 parity，避免
/// 需要额外传输原始帧长度。每个 group 可恢复任意 1 个丢片。
pub fn fragment_video_fec(
    frame_id: u32,
    pts_us: u64,
    key: bool,
    codec: Codec,
    payload: &[u8],
    group_size: u8,
) -> Result<Vec<VideoDatagram>, VideoDatagramError> {
    if !(2..=64).contains(&group_size) {
        return Err(VideoDatagramError::BadFecGroup(group_size));
    }
    let mut out = fragment_video(frame_id, pts_us, key, codec, payload)?;
    let count = out.len();
    let group = usize::from(group_size);
    let key_flag = if key { VIDEO_DATAGRAM_KEY } else { 0 };
    for group_index in 0..(count / group) {
        let start = group_index * group;
        let mut parity = vec![0u8; VIDEO_DATAGRAM_PAYLOAD];
        for packet in out.iter().skip(start).take(group) {
            for (dst, src) in parity.iter_mut().zip(&packet.payload) {
                *dst ^= *src;
            }
        }
        out.push(VideoDatagram {
            header: VideoDatagramHeader {
                frame_id,
                frag_index: group_index as u16,
                frag_count: count as u16,
                flags: key_flag | VIDEO_DATAGRAM_PARITY,
                codec: codec.id(),
                reserved: group_size,
                pts_us,
            },
            payload: parity,
        });
    }
    Ok(out)
}

#[derive(Debug, Default)]
pub struct VideoDatagramReassembler {
    pending: BTreeMap<u32, PendingFrame>,
    completed: BTreeSet<u32>,
}

#[derive(Debug)]
struct PendingFrame {
    key: bool,
    codec: u8,
    pts_us: u64,
    frag_count: u16,
    parts: Vec<Option<Vec<u8>>>,
    received: usize,
    fec_group: u8,
    parity: BTreeMap<u16, Vec<u8>>,
}

impl PendingFrame {
    fn recover_fec(&mut self) {
        let group = usize::from(self.fec_group);
        if group == 0 {
            return;
        }
        loop {
            let mut recovered = false;
            for (&group_index, parity) in &self.parity {
                let start = group_index as usize * group;
                if start + group > self.parts.len() {
                    continue;
                }
                let missing: Vec<usize> = (start..start + group)
                    .filter(|index| self.parts[*index].is_none())
                    .collect();
                if missing.len() != 1 {
                    continue;
                }
                let target = missing[0];
                let mut data = parity.clone();
                for index in start..start + group {
                    if index == target {
                        continue;
                    }
                    if let Some(part) = &self.parts[index] {
                        for (dst, src) in data.iter_mut().zip(part) {
                            *dst ^= *src;
                        }
                    }
                }
                self.parts[target] = Some(data);
                self.received += 1;
                recovered = true;
            }
            if !recovered {
                break;
            }
        }
    }

    fn is_complete(&self) -> bool {
        self.received == self.parts.len()
    }
}

impl VideoDatagramReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending_frames(&self) -> usize {
        self.pending.len()
    }

    /// 指定帧的缺片位图，按 64 位分组；最多 16 个字（1024 片）。
    pub fn missing_words(&self, frame_id: u32) -> Vec<u64> {
        let Some(frame) = self.pending.get(&frame_id) else {
            return Vec::new();
        };
        let mut out = vec![0u64; frame.parts.len().div_ceil(64)];
        for (index, part) in frame.parts.iter().enumerate() {
            if part.is_none() {
                out[index / 64] |= 1u64 << (index % 64);
            }
        }
        out
    }

    pub fn push(
        &mut self,
        datagram: VideoDatagram,
    ) -> Result<Option<VideoDatagramFrame>, VideoDatagramError> {
        let header = datagram.header;
        header.validate()?;
        if self.completed.contains(&header.frame_id) {
            return Ok(None);
        }
        let entry = self
            .pending
            .entry(header.frame_id)
            .or_insert_with(|| PendingFrame {
                key: header.is_key(),
                codec: header.codec,
                pts_us: header.pts_us,
                frag_count: header.frag_count,
                parts: vec![None; header.frag_count as usize],
                received: 0,
                fec_group: 0,
                parity: BTreeMap::new(),
            });
        if entry.frag_count != header.frag_count
            || entry.key != header.is_key()
            || entry.codec != header.codec
            || entry.pts_us != header.pts_us
        {
            return Err(VideoDatagramError::FrameMismatch(header.frame_id));
        }
        if header.is_parity() {
            if entry.fec_group != 0 && entry.fec_group != header.reserved {
                return Err(VideoDatagramError::FecMismatch(header.frame_id));
            }
            entry.fec_group = header.reserved;
            let slot = &mut entry.parity;
            match slot.get(&header.frag_index) {
                Some(existing) if existing == &datagram.payload => return Ok(None),
                Some(_) => return Err(VideoDatagramError::DuplicateConflict(header.frame_id)),
                None => {
                    slot.insert(header.frag_index, datagram.payload);
                }
            }
        } else {
            let slot = &mut entry.parts[header.frag_index as usize];
            match slot {
                Some(existing) if existing == &datagram.payload => return Ok(None),
                Some(_) => return Err(VideoDatagramError::DuplicateConflict(header.frame_id)),
                None => {
                    *slot = Some(datagram.payload);
                    entry.received += 1;
                }
            }
        }
        entry.recover_fec();
        if !entry.is_complete() {
            return Ok(None);
        }
        let pending = self.pending.remove(&header.frame_id).expect("entry exists");
        self.completed.insert(header.frame_id);
        while self.completed.len() > 1024 {
            if let Some(oldest) = self.completed.iter().next().copied() {
                self.completed.remove(&oldest);
            }
        }
        let mut payload = Vec::new();
        for part in pending.parts {
            payload.extend_from_slice(&part.expect("complete frame"));
        }
        Ok(Some(VideoDatagramFrame {
            frame_id: header.frame_id,
            key: pending.key,
            codec: pending.codec,
            pts_us: pending.pts_us,
            payload,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoDatagramError {
    ShortHeader,
    TooLarge(usize),
    BadFragmentCount(u16),
    BadFragmentIndex { index: u16, count: u16 },
    FrameTooLarge(usize),
    FrameMismatch(u32),
    DuplicateConflict(u32),
    BadFecGroup(u8),
    FecMismatch(u32),
}

impl fmt::Display for VideoDatagramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShortHeader => write!(f, "video datagram shorter than header"),
            Self::TooLarge(n) => {
                write!(f, "video datagram is {n} bytes, limit {VIDEO_DATAGRAM_MTU}")
            }
            Self::BadFragmentCount(n) => write!(f, "invalid video fragment count {n}"),
            Self::BadFragmentIndex { index, count } => {
                write!(f, "video fragment index {index} out of range {count}")
            }
            Self::FrameTooLarge(n) => write!(
                f,
                "video frame is {n} bytes, exceeds datagram fragment limit"
            ),
            Self::FrameMismatch(id) => write!(f, "video frame {id} fragment metadata mismatch"),
            Self::DuplicateConflict(id) => {
                write!(f, "video frame {id} has conflicting duplicate fragment")
            }
            Self::BadFecGroup(group) => write!(f, "invalid video FEC group size {group}"),
            Self::FecMismatch(id) => write!(f, "video frame {id} FEC metadata mismatch"),
        }
    }
}

impl std::error::Error for VideoDatagramError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_payload(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn fragments_roundtrip_and_reassemble_out_of_order() {
        let payload = frame_payload(VIDEO_DATAGRAM_PAYLOAD * 2 + 17);
        let fragments = fragment_video(7, 123_456, true, Codec::H264, &payload).unwrap();
        assert_eq!(fragments.len(), 3);
        assert!(fragments.iter().all(|d| d.encode().unwrap().len() <= 1100));

        let mut reassembler = VideoDatagramReassembler::new();
        assert!(reassembler.push(fragments[2].clone()).unwrap().is_none());
        assert!(reassembler.push(fragments[0].clone()).unwrap().is_none());
        let frame = reassembler.push(fragments[1].clone()).unwrap().unwrap();
        assert_eq!(frame.frame_id, 7);
        assert!(frame.key);
        assert_eq!(frame.codec, Codec::H264.id());
        assert_eq!(frame.pts_us, 123_456);
        assert_eq!(frame.payload, payload);
        assert_eq!(reassembler.pending_frames(), 0);
    }

    #[test]
    fn duplicate_fragment_is_idempotent_and_conflict_is_rejected() {
        let payload = frame_payload(VIDEO_DATAGRAM_PAYLOAD + 10);
        let fragments = fragment_video(9, 1, false, Codec::Av1, &payload).unwrap();
        assert!(fragments.len() >= 2);
        let mut reassembler = VideoDatagramReassembler::new();
        assert!(reassembler.push(fragments[0].clone()).unwrap().is_none());
        assert!(reassembler.push(fragments[0].clone()).unwrap().is_none());

        let mut changed = fragments[0].clone();
        changed.payload[0] ^= 0xff;
        assert_eq!(
            reassembler.push(changed),
            Err(VideoDatagramError::DuplicateConflict(9))
        );
    }

    #[test]
    fn frame_metadata_mismatch_is_rejected() {
        let payload = frame_payload(VIDEO_DATAGRAM_PAYLOAD + 10);
        let fragments = fragment_video(11, 1, false, Codec::Vp9, &payload).unwrap();
        assert!(fragments.len() >= 2);
        let mut reassembler = VideoDatagramReassembler::new();
        assert!(reassembler.push(fragments[0].clone()).unwrap().is_none());
        let mut changed = fragments[0].clone();
        changed.header.pts_us = 2;
        assert_eq!(
            reassembler.push(changed),
            Err(VideoDatagramError::FrameMismatch(11))
        );
    }

    #[test]
    fn fec_recovers_one_missing_fragment_per_group() {
        let payload = frame_payload(VIDEO_DATAGRAM_PAYLOAD * 17 + 5);
        let fragments = fragment_video_fec(21, 7, true, Codec::H265, &payload, 8).unwrap();
        assert_eq!(fragments.len(), 18 + 2);
        let mut reassembler = VideoDatagramReassembler::new();
        let mut frame = None;
        for (index, datagram) in fragments.into_iter().enumerate() {
            if index == 3 {
                continue;
            }
            frame = reassembler.push(datagram).unwrap().or(frame);
        }
        let frame = frame.expect("FEC should recover the missing fragment");
        assert_eq!(frame.frame_id, 21);
        assert_eq!(frame.payload, payload);
        assert_eq!(reassembler.pending_frames(), 0);
    }

    #[test]
    fn large_frame_nack_covers_more_than_64_fragments() {
        use crate::fragment::SliceRecovery;
        let payload = frame_payload(VIDEO_DATAGRAM_PAYLOAD * 70);
        let fragments = fragment_video(31, 9, true, Codec::H264, &payload).unwrap();
        assert_eq!(fragments.len(), 70);
        let mut reassembler = VideoDatagramReassembler::new();
        for (index, datagram) in fragments.into_iter().enumerate() {
            if index == 3 || index == 67 {
                continue;
            }
            assert!(reassembler.push(datagram).unwrap().is_none());
        }
        let words = reassembler.missing_words(31);
        assert_eq!(words.len(), 2);
        let mut recovery = SliceRecovery::default();
        let nack = recovery.observe_words(31, &words, 0).unwrap();
        assert_eq!(nack.words().count(), 2);
        assert_eq!(nack.missing_count(), 2);
        let encoded = nack.encode();
        assert_eq!(encoded.len(), 12 + 8);
        assert_eq!(crate::plane::SliceNack::decode(&encoded).unwrap(), nack);
    }

    #[test]
    fn oversize_datagram_is_rejected() {
        let err = VideoDatagram::decode(&vec![0u8; VIDEO_DATAGRAM_MTU + 1]).unwrap_err();
        assert_eq!(err, VideoDatagramError::TooLarge(VIDEO_DATAGRAM_MTU + 1));
    }
}
