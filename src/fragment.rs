//! 视频大帧切片/重组（V3 §11.2 / V4 §3）。
//!
//! 单个视频帧最多 16 MiB，按 256 KiB 切片，`frag_count <= 64`。同一
//! `frame_id` 的切片顺序发送，`LAST_FRAG` 只出现在最后一片。

use std::collections::{BTreeMap, VecDeque};

use crate::channel::MAX_FRAME_LEN;
use crate::control::RequestKeyframe;
use crate::media::{frame_flags, MediaHeader};
use crate::plane::SliceNack;

/// 每片最大载荷。
pub const FRAGMENT_BYTES: usize = 256 << 10;
/// 每帧最大片数。
pub const MAX_FRAGMENTS: usize = 64;
/// 每帧最大载荷（64 × 256 KiB）。
pub const MAX_FRAGMENTED_FRAME_BYTES: usize = FRAGMENT_BYTES * MAX_FRAGMENTS;
/// 同时跟踪的未完成帧数；超过后淘汰最早接收的帧。
pub const MAX_PENDING_FRAMES: usize = 8;

/// 缺片后等待重传的时间；超时仍缺则请求关键帧。
pub const SLICE_RECOVERY_MS: u64 = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FragmentError {
    EmptyFrame,
    TooLarge { bytes: usize },
    BadFragCount { count: u16 },
    BadFragIndex { index: u16, count: u16 },
    BadFragmentSize { index: u16, size: usize, max: usize },
    WrongFrameId { expected: u32, got: u32 },
    FrameMismatch { frame_id: u32 },
    InconsistentCount { expected: u16, got: u16 },
    DuplicateFragment { index: u16 },
    MissingFragments { count: u16 },
    BadLastFlag { index: u16, count: u16 },
}

impl std::fmt::Display for FragmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyFrame => write!(f, "empty video frame"),
            Self::TooLarge { bytes } => write!(f, "video frame too large: {bytes} bytes"),
            Self::BadFragCount { count } => write!(f, "bad fragment count: {count}"),
            Self::BadFragIndex { index, count } => {
                write!(f, "fragment index {index} out of range for count {count}")
            }
            Self::BadFragmentSize { index, size, max } => {
                write!(f, "fragment {index} size {size} exceeds {max}")
            }
            Self::WrongFrameId { expected, got } => {
                write!(f, "expected frame_id {expected}, got {got}")
            }
            Self::FrameMismatch { frame_id } => {
                write!(f, "frame {frame_id} fragment metadata mismatch")
            }
            Self::InconsistentCount { expected, got } => {
                write!(f, "expected frag_count {expected}, got {got}")
            }
            Self::DuplicateFragment { index } => write!(f, "duplicate fragment {index}"),
            Self::MissingFragments { count } => write!(f, "missing {count} fragments"),
            Self::BadLastFlag { index, count } => {
                write!(f, "LAST_FRAG mismatch at fragment {index}/{count}")
            }
        }
    }
}

impl std::error::Error for FragmentError {}

/// 把一帧切成 `(MediaHeader, payload)` 切片。输入 header 的 `frag_idx` /
/// `frag_count` 会被覆盖。单帧也返回 1 片并置 `LAST_FRAG`。
pub fn fragment_video(
    header: &MediaHeader,
    payload: &[u8],
) -> Result<Vec<(MediaHeader, Vec<u8>)>, FragmentError> {
    if payload.is_empty() {
        return Err(FragmentError::EmptyFrame);
    }
    if payload.len() > MAX_FRAGMENTED_FRAME_BYTES || payload.len() > MAX_FRAME_LEN {
        return Err(FragmentError::TooLarge {
            bytes: payload.len(),
        });
    }
    let count = payload.len().div_ceil(FRAGMENT_BYTES);
    if count == 0 || count > MAX_FRAGMENTS {
        return Err(FragmentError::BadFragCount {
            count: count as u16,
        });
    }
    let count = count as u16;
    let mut out = Vec::with_capacity(count as usize);
    for (index, chunk) in payload.chunks(FRAGMENT_BYTES).enumerate() {
        let mut h = header.clone();
        h.frag_idx = index as u16;
        h.frag_count = count;
        h.set_last_frag(index + 1 == count as usize);
        out.push((h, chunk.to_vec()));
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReassembledFrame {
    pub header: MediaHeader,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
struct PendingFrame {
    header: MediaHeader,
    parts: Vec<Option<Vec<u8>>>,
    total: usize,
}

/// 按 `frame_id` 重组视频切片。允许切片乱序到达，并可同时跟踪多帧。
///
/// 默认最多跟踪 [`MAX_PENDING_FRAMES`] 个未完成帧；超过后淘汰最早接收的
/// 未完成帧。`missing_bitmap()` 仍返回最近一帧的 64 位视图，供可靠流使用。
#[derive(Debug)]
pub struct VideoReassembler {
    frames: BTreeMap<u32, PendingFrame>,
    order: VecDeque<u32>,
    max_frames: usize,
    active: Option<u32>,
}

impl Default for VideoReassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoReassembler {
    pub fn new() -> Self {
        Self::with_capacity(MAX_PENDING_FRAMES)
    }

    pub fn with_capacity(max_frames: usize) -> Self {
        Self {
            frames: BTreeMap::new(),
            order: VecDeque::new(),
            max_frames: max_frames.max(1),
            active: None,
        }
    }

    pub fn reset(&mut self) {
        self.frames.clear();
        self.order.clear();
        self.active = None;
    }

    /// 所有未完成帧中已到达的切片总数。
    pub fn pending(&self) -> usize {
        self.frames
            .values()
            .map(|frame| frame.parts.iter().filter(|part| part.is_some()).count())
            .sum()
    }

    /// 未完成帧数量。
    pub fn pending_frames(&self) -> usize {
        self.frames.len()
    }

    /// 最近接收的未完成帧。
    pub fn frame_id(&self) -> Option<u32> {
        self.active
    }

    /// 当前未到齐切片的位图（bit i = 第 i 片缺失）。
    ///
    /// 未开始重组或已完整时返回 0；位图只覆盖前 64 片，与可靠流
    /// `MAX_FRAGMENTS` 对齐。大帧请使用 [`Self::missing_words_for`]。
    pub fn missing_bitmap(&self) -> u64 {
        self.active
            .and_then(|id| self.frames.get(&id))
            .map(|frame| {
                frame.parts.iter().take(MAX_FRAGMENTS).enumerate().fold(
                    0u64,
                    |bitmap, (index, part)| {
                        if part.is_none() {
                            bitmap | (1u64 << index)
                        } else {
                            bitmap
                        }
                    },
                )
            })
            .unwrap_or(0)
    }

    /// 指定帧的缺片位图，最多 64 片。
    pub fn missing_bitmap_for(&self, frame_id: u32) -> u64 {
        self.frames
            .get(&frame_id)
            .map(|frame| {
                frame.parts.iter().take(MAX_FRAGMENTS).enumerate().fold(
                    0u64,
                    |bitmap, (index, part)| {
                        if part.is_none() {
                            bitmap | (1u64 << index)
                        } else {
                            bitmap
                        }
                    },
                )
            })
            .unwrap_or(0)
    }

    /// 指定帧的缺片位图，按 64 位分组；用于 >64 片的 DATAGRAM/扩展 NACK。
    pub fn missing_words_for(&self, frame_id: u32) -> Vec<u64> {
        let Some(frame) = self.frames.get(&frame_id) else {
            return Vec::new();
        };
        let words = frame.parts.len().div_ceil(64);
        let mut out = vec![0u64; words];
        for (index, part) in frame.parts.iter().enumerate() {
            if part.is_none() {
                out[index / 64] |= 1u64 << (index % 64);
            }
        }
        out
    }

    pub fn push(
        &mut self,
        header: &MediaHeader,
        payload: &[u8],
    ) -> Result<Option<ReassembledFrame>, FragmentError> {
        if payload.is_empty() {
            return Err(FragmentError::EmptyFrame);
        }
        if header.frag_count == 0 || header.frag_count as usize > MAX_FRAGMENTS {
            return Err(FragmentError::BadFragCount {
                count: header.frag_count,
            });
        }
        if header.frag_idx >= header.frag_count {
            return Err(FragmentError::BadFragIndex {
                index: header.frag_idx,
                count: header.frag_count,
            });
        }
        let is_last = header.frag_idx + 1 == header.frag_count;
        if header.last_frag() != is_last {
            return Err(FragmentError::BadLastFlag {
                index: header.frag_idx,
                count: header.frag_count,
            });
        }
        if payload.len() > FRAGMENT_BYTES {
            return Err(FragmentError::BadFragmentSize {
                index: header.frag_idx,
                size: payload.len(),
                max: FRAGMENT_BYTES,
            });
        }
        if !is_last && payload.len() != FRAGMENT_BYTES {
            return Err(FragmentError::BadFragmentSize {
                index: header.frag_idx,
                size: payload.len(),
                max: FRAGMENT_BYTES,
            });
        }

        if !self.frames.contains_key(&header.frame_id) {
            while self.frames.len() >= self.max_frames {
                let Some(oldest) = self.order.pop_front() else {
                    break;
                };
                self.frames.remove(&oldest);
                if self.active == Some(oldest) {
                    self.active = self.order.back().copied();
                }
            }
            self.frames.insert(
                header.frame_id,
                PendingFrame {
                    header: header.clone(),
                    parts: vec![None; header.frag_count as usize],
                    total: 0,
                },
            );
            self.order.push_back(header.frame_id);
            self.active = Some(header.frame_id);
        }

        let frame = self.frames.get_mut(&header.frame_id).expect("inserted");
        if frame.header.frag_count != header.frag_count {
            return Err(FragmentError::InconsistentCount {
                expected: frame.header.frag_count,
                got: header.frag_count,
            });
        }
        if frame.header.codec() != header.codec()
            || frame.header.pts_us != header.pts_us
            || frame.header.key() != header.key()
        {
            return Err(FragmentError::FrameMismatch {
                frame_id: header.frame_id,
            });
        }
        let slot = &mut frame.parts[header.frag_idx as usize];
        if slot.is_some() {
            return Err(FragmentError::DuplicateFragment {
                index: header.frag_idx,
            });
        }
        frame.total = frame.total.saturating_add(payload.len());
        if frame.total > MAX_FRAGMENTED_FRAME_BYTES {
            let total = frame.total;
            self.frames.remove(&header.frame_id);
            self.order.retain(|id| *id != header.frame_id);
            if self.active == Some(header.frame_id) {
                self.active = self.order.back().copied();
            }
            return Err(FragmentError::TooLarge { bytes: total });
        }
        *slot = Some(payload.to_vec());

        if frame.parts.iter().any(|part| part.is_none()) {
            return Ok(None);
        }

        let pending = self
            .frames
            .remove(&header.frame_id)
            .expect("complete frame");
        self.order.retain(|id| *id != header.frame_id);
        if self.active == Some(header.frame_id) {
            self.active = self.order.back().copied();
        }
        let mut out = Vec::with_capacity(pending.total);
        for part in pending.parts {
            out.extend_from_slice(part.as_deref().expect("all parts present"));
        }
        let mut final_header = pending.header;
        final_header.frag_idx = 0;
        final_header.frag_count = 1;
        final_header.flags &= !frame_flags::LAST_FRAG;
        Ok(Some(ReassembledFrame {
            header: final_header,
            payload: out,
        }))
    }
}

/// 缺片恢复状态机：先发一次 `SliceNack`，20 ms 后仍缺则请求关键帧。
///
/// 它不持有连接，也不起后台线程；调用方在收到切片/定时轮询时驱动：
/// 1. 每次 `push` 后把 `missing_bitmap()`、当前时间交给 [`observe`]；
/// 2. 周期性调用 [`poll`]，返回 `RequestKeyframe` 时发送并清空该帧；
/// 3. 完整帧到达后调用 [`complete`]。
#[derive(Debug, Default)]
pub struct SliceRecovery {
    frame_id: Option<u32>,
    deadline_ms: u64,
    missing: u64,
    extra: Vec<u64>,
    nack_sent: bool,
    requested: bool,
}

impl SliceRecovery {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录当前缺片；首次发现某帧缺片时返回应发送的 `SliceNack`。
    pub fn observe(&mut self, frame_id: u32, missing: u64, now_ms: u64) -> Option<SliceNack> {
        self.observe_words(frame_id, &[missing], now_ms)
    }

    /// 记录多字缺片位图；>64 片的 DATAGRAM 使用这一入口。
    ///
    /// `words` 按小端位序排列：第 0 字覆盖 0..64，第 1 字覆盖 64..128。
    pub fn observe_words(
        &mut self,
        frame_id: u32,
        words: &[u64],
        now_ms: u64,
    ) -> Option<SliceNack> {
        let mut normalized: Vec<u64> = words.iter().copied().take(SliceNack::MAX_WORDS).collect();
        while normalized.last() == Some(&0) {
            normalized.pop();
        }
        if normalized.is_empty() {
            self.complete(frame_id);
            return None;
        }
        if self.frame_id != Some(frame_id) {
            self.frame_id = Some(frame_id);
            self.deadline_ms = now_ms.saturating_add(SLICE_RECOVERY_MS);
            self.missing = normalized[0];
            self.extra = normalized[1..].to_vec();
            self.nack_sent = false;
            self.requested = false;
        } else {
            self.missing |= normalized[0];
            if self.extra.len() < normalized.len() - 1 {
                self.extra.resize(normalized.len() - 1, 0);
            }
            for (index, word) in normalized.iter().enumerate().skip(1) {
                self.extra[index - 1] |= *word;
            }
        }
        if self.nack_sent {
            return None;
        }
        self.nack_sent = true;
        let mut all = Vec::with_capacity(1 + self.extra.len());
        all.push(self.missing);
        all.extend_from_slice(&self.extra);
        Some(SliceNack::with_words(frame_id, &all))
    }

    /// 当前缺片的完整多字位图。
    pub fn missing_words(&self) -> Vec<u64> {
        let mut out = Vec::with_capacity(1 + self.extra.len());
        out.push(self.missing);
        out.extend_from_slice(&self.extra);
        out
    }

    /// 到 20 ms 仍未补齐时返回一次关键帧请求；同一帧只返回一次。
    pub fn poll(&mut self, now_ms: u64) -> Option<RequestKeyframe> {
        if self.frame_id.is_none() || self.requested || now_ms < self.deadline_ms {
            return None;
        }
        self.requested = true;
        Some(RequestKeyframe {
            display: 0,
            reason: 1,
        })
    }

    pub fn complete(&mut self, frame_id: u32) {
        if self.frame_id == Some(frame_id) {
            self.reset();
        }
    }

    pub fn reset(&mut self) {
        self.frame_id = None;
        self.deadline_ms = 0;
        self.missing = 0;
        self.extra.clear();
        self.nack_sent = false;
        self.requested = false;
    }

    pub fn pending_frame(&self) -> Option<u32> {
        self.frame_id
    }

    pub fn missing(&self) -> u64 {
        self.missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Codec;

    fn header() -> MediaHeader {
        let mut h = MediaHeader::new(Codec::H264, 7, 1234);
        h.set_key(true);
        h
    }

    #[test]
    fn fragments_and_reassembles_large_frame() {
        let payload = vec![0xAB; FRAGMENT_BYTES * 2 + 77];
        let mut h = header();
        h.rects = vec![crate::media::Rect::new(1, 2, 3, 4)];
        let fragments = fragment_video(&h, &payload).unwrap();
        assert_eq!(fragments.len(), 3);
        assert_eq!(fragments[0].1.len(), FRAGMENT_BYTES);
        assert_eq!(fragments[1].1.len(), FRAGMENT_BYTES);
        assert_eq!(fragments[2].1.len(), 77);
        assert!(!fragments[0].0.last_frag());
        assert!(fragments[2].0.last_frag());
        assert_eq!(fragments[0].0.frag_count, 3);

        let mut r = VideoReassembler::new();
        assert!(r.push(&fragments[0].0, &fragments[0].1).unwrap().is_none());
        assert!(r.push(&fragments[2].0, &fragments[2].1).unwrap().is_none());
        let frame = r.push(&fragments[1].0, &fragments[1].1).unwrap().unwrap();
        assert_eq!(frame.payload, payload);
        assert_eq!(frame.header.frame_id, 7);
        assert_eq!(frame.header.frag_count, 1);
        assert_eq!(frame.header.rects, h.rects);
    }

    #[test]
    fn single_fragment_sets_last_and_reassembles() {
        let payload = vec![1u8; 17];
        let fragments = fragment_video(&header(), &payload).unwrap();
        assert_eq!(fragments.len(), 1);
        assert!(fragments[0].0.last_frag());
        let mut r = VideoReassembler::new();
        let got = r.push(&fragments[0].0, &fragments[0].1).unwrap().unwrap();
        assert_eq!(got.payload, payload);
    }

    #[test]
    fn rejects_bad_flags_sizes_and_duplicates() {
        let payload = vec![3u8; FRAGMENT_BYTES + 1];
        let fragments = fragment_video(&header(), &payload).unwrap();
        let mut r = VideoReassembler::new();
        let mut bad = fragments[0].0.clone();
        bad.set_last_frag(true);
        assert!(matches!(
            r.push(&bad, &fragments[0].1),
            Err(FragmentError::BadLastFlag { .. })
        ));

        let mut r = VideoReassembler::new();
        assert!(r.push(&fragments[0].0, &fragments[0].1).unwrap().is_none());
        assert!(matches!(
            r.push(&fragments[0].0, &fragments[0].1),
            Err(FragmentError::DuplicateFragment { .. })
        ));
    }

    #[test]
    fn tracks_multiple_frames_concurrently() {
        let payload = vec![0x7E; FRAGMENT_BYTES + 31];
        let mut h7 = MediaHeader::new(Codec::H264, 7, 100);
        h7.rects = vec![crate::media::Rect::new(1, 1, 2, 2)];
        let mut h8 = MediaHeader::new(Codec::H264, 8, 200);
        h8.rects = vec![crate::media::Rect::new(3, 3, 4, 4)];
        let f7 = fragment_video(&h7, &payload).unwrap();
        let f8 = fragment_video(&h8, &payload).unwrap();

        let mut r = VideoReassembler::new();
        assert!(r.push(&f7[0].0, &f7[0].1).unwrap().is_none());
        assert!(r.push(&f8[0].0, &f8[0].1).unwrap().is_none());
        assert_eq!(r.pending_frames(), 2);
        assert_eq!(r.pending(), 2);
        assert_eq!(r.frame_id(), Some(8));

        let out8 = r.push(&f8[1].0, &f8[1].1).unwrap().unwrap();
        assert_eq!(out8.header.frame_id, 8);
        assert_eq!(out8.payload, payload);
        assert_eq!(r.pending_frames(), 1);

        let out7 = r.push(&f7[1].0, &f7[1].1).unwrap().unwrap();
        assert_eq!(out7.header.frame_id, 7);
        assert_eq!(out7.payload, payload);
        assert_eq!(r.pending_frames(), 0);
    }

    #[test]
    fn missing_words_cover_large_fragment_sets() {
        let payload = vec![0x2A; FRAGMENT_BYTES * 2 + 1];
        let fragments = fragment_video(&header(), &payload).unwrap();
        let mut r = VideoReassembler::new();
        assert!(r.push(&fragments[0].0, &fragments[0].1).unwrap().is_none());
        let words = r.missing_words_for(fragments[0].0.frame_id);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].count_ones(), 2);
    }

    #[test]
    fn extended_slice_nack_covers_more_than_64_fragments() {
        let mut words = vec![0u64; SliceNack::MAX_WORDS];
        words[15] = 1u64 << 63;
        let mut recovery = SliceRecovery::new();
        let nack = recovery.observe_words(99, &words, 1_000).unwrap();
        assert_eq!(nack.frame_id, 99);
        assert_eq!(nack.extra.len(), SliceNack::MAX_WORDS - 1);
        assert_eq!(nack.missing_count(), 1);
        let encoded = nack.encode();
        assert_eq!(encoded.len(), SliceNack::MAX_LEN);
        assert_eq!(SliceNack::decode(&encoded).unwrap(), nack);
        assert_eq!(recovery.missing_words(), words);
    }

    #[test]
    fn recovery_nacks_then_requests_keyframe_after_20ms() {
        let payload = vec![0x5A; FRAGMENT_BYTES + 1];
        let fragments = fragment_video(&header(), &payload).unwrap();
        let mut reassembler = VideoReassembler::new();
        let mut recovery = SliceRecovery::new();

        assert!(reassembler
            .push(&fragments[0].0, &fragments[0].1)
            .unwrap()
            .is_none());
        let nack = recovery
            .observe(fragments[0].0.frame_id, reassembler.missing_bitmap(), 1_000)
            .unwrap();
        assert_eq!(nack.frame_id, fragments[0].0.frame_id);
        assert_eq!(nack.missing_bitmap.count_ones(), 1);
        assert!(recovery.observe(nack.frame_id, 0b10, 1_001).is_none());
        assert!(recovery.poll(1_019).is_none());
        let request = recovery.poll(1_020).unwrap();
        assert_eq!(request.display, 0);
        assert_eq!(request.reason, 1);
        assert!(recovery.poll(1_021).is_none());

        recovery.complete(nack.frame_id);
        assert_eq!(recovery.pending_frame(), None);
        assert_eq!(recovery.missing(), 0);
    }

    #[test]
    fn property_reassembles_eight_interleaved_frames() {
        let mut fragments = Vec::new();
        let mut expected = std::collections::BTreeMap::new();
        for id in 0..MAX_PENDING_FRAMES as u32 {
            let mut h = header();
            h.frame_id = id;
            h.pts_us = 1_000 + id as u64;
            let payload = (0..FRAGMENT_BYTES + 7)
                .map(|i| (id as usize + i) as u8)
                .collect::<Vec<_>>();
            expected.insert(id, payload.clone());
            fragments.extend(fragment_video(&h, &payload).unwrap());
        }

        let mut rng = 0x1234_5678_9ABC_DEF0u64;
        for i in (1..fragments.len()).rev() {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (rng as usize) % (i + 1);
            fragments.swap(i, j);
        }

        let mut reassembler = VideoReassembler::new();
        let mut done = std::collections::BTreeMap::new();
        for (header, payload) in fragments {
            if let Some(frame) = reassembler.push(&header, &payload).unwrap() {
                done.insert(frame.header.frame_id, frame.payload);
            }
        }
        assert_eq!(done.len(), MAX_PENDING_FRAMES);
        for (id, payload) in expected {
            assert_eq!(done.get(&id), Some(&payload), "frame {id}");
        }
        assert_eq!(reassembler.pending_frames(), 0);
    }

    #[test]
    fn rejects_oversize_and_empty() {
        assert!(matches!(
            fragment_video(&header(), &[]),
            Err(FragmentError::EmptyFrame)
        ));
        let too_big = vec![0u8; MAX_FRAGMENTED_FRAME_BYTES + 1];
        assert!(matches!(
            fragment_video(&header(), &too_big),
            Err(FragmentError::TooLarge { .. })
        ));
    }
}
