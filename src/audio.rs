//! 音频抖动缓冲与 RED 冗余包。
//!
//! 线格式仍由 `AudioHeader` + [`AudioFrame`](crate::payload::AudioFrame) 承载；
//! 本模块只处理 Opus 包的顺序、到达抖动和 RED 恢复，不依赖音频设备。

use std::collections::BTreeMap;

/// Opus 固定 48 kHz。
pub const AUDIO_RATE: u32 = 48_000;
/// 默认 Opus 帧长。
pub const AUDIO_FRAME_MS: u8 = 10;
/// jitter buffer 目标下限。
pub const MIN_TARGET_MS: u32 = 20;
/// jitter buffer 目标上限。
pub const MAX_TARGET_MS: u32 = 200;
/// 默认目标（无抖动时）。
pub const DEFAULT_TARGET_MS: u32 = 50;
/// RED 最多携带的包数（当前包 + 前 2 包）。
pub const MAX_RED_PACKETS: usize = 3;
/// RED 单包上限。
pub const MAX_RED_PAYLOAD: usize = 64 << 10;
/// 队列空闲超过该时长后把 playout 时钟重新锚定到下一包，避免长会话漂移累积。
pub const CLOCK_REANCHOR_IDLE_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioPacket {
    pub seq: u32,
    pub pts_us: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitterStats {
    pub received: u64,
    pub played: u64,
    pub dropped_late: u64,
    pub duplicates: u64,
    pub recovered_red: u64,
    pub target_ms: u32,
    pub max_jitter_ms: u32,
    pub playout_delay_ms: u32,
    pub clock_reanchors: u64,
    pub clock_skew_ppm: i32,
}

#[derive(Debug, Clone)]
struct Entry {
    packet: AudioPacket,
    arrival_us: u64,
}

#[derive(Debug, Clone)]
pub struct JitterBuffer {
    min_target_ms: u32,
    max_target_ms: u32,
    target_ms: u32,
    jitter_ms: f64,
    first_arrival_us: Option<u64>,
    first_pts_us: Option<u64>,
    last_arrival_us: Option<u64>,
    clock_skew: f64,
    skew_n: u64,
    skew_sx: f64,
    skew_sy: f64,
    skew_sxx: f64,
    skew_sxy: f64,
    next_seq: Option<u32>,
    packets: BTreeMap<u32, Entry>,
    stats: JitterStats,
}

impl Default for JitterBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_TARGET_MS)
    }
}

impl JitterBuffer {
    pub fn new(base_target_ms: u32) -> Self {
        let target = base_target_ms.clamp(MIN_TARGET_MS, MAX_TARGET_MS);
        Self {
            min_target_ms: MIN_TARGET_MS,
            max_target_ms: MAX_TARGET_MS,
            target_ms: target,
            jitter_ms: 0.0,
            first_arrival_us: None,
            first_pts_us: None,
            last_arrival_us: None,
            clock_skew: 0.0,
            skew_n: 0,
            skew_sx: 0.0,
            skew_sy: 0.0,
            skew_sxx: 0.0,
            skew_sxy: 0.0,
            next_seq: None,
            packets: BTreeMap::new(),
            stats: JitterStats {
                target_ms: target,
                ..JitterStats::default()
            },
        }
    }

    pub fn with_limits(base_target_ms: u32, min_ms: u32, max_ms: u32) -> Self {
        let min = min_ms.clamp(1, MAX_TARGET_MS);
        let max = max_ms.clamp(min, MAX_TARGET_MS);
        let target = base_target_ms.clamp(min, max);
        Self {
            min_target_ms: min,
            max_target_ms: max,
            target_ms: target,
            jitter_ms: 0.0,
            first_arrival_us: None,
            first_pts_us: None,
            last_arrival_us: None,
            clock_skew: 0.0,
            skew_n: 0,
            skew_sx: 0.0,
            skew_sy: 0.0,
            skew_sxx: 0.0,
            skew_sxy: 0.0,
            next_seq: None,
            packets: BTreeMap::new(),
            stats: JitterStats {
                target_ms: target,
                ..JitterStats::default()
            },
        }
    }

    pub fn target_ms(&self) -> u32 {
        self.target_ms
    }
    pub fn stats(&self) -> JitterStats {
        let mut s = self.stats;
        s.target_ms = self.target_ms;
        s
    }
    pub fn len(&self) -> usize {
        self.packets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    /// 把 playout 基准重新锚定到最早排队包（或下一包）。用于长会话/空闲后的时钟归零。
    pub fn reanchor(&mut self) {
        if let Some((&seq, entry)) = self.packets.iter().next() {
            self.first_arrival_us = Some(entry.arrival_us);
            self.first_pts_us = Some(entry.packet.pts_us);
            self.next_seq = Some(seq);
        } else {
            self.first_arrival_us = None;
            self.first_pts_us = None;
        }
        self.jitter_ms = 0.0;
        self.clock_skew = 0.0;
        self.skew_n = 0;
        self.skew_sx = 0.0;
        self.skew_sy = 0.0;
        self.skew_sxx = 0.0;
        self.skew_sxy = 0.0;
        self.stats.clock_skew_ppm = 0;
        self.target_ms = DEFAULT_TARGET_MS.clamp(self.min_target_ms, self.max_target_ms);
        self.stats.clock_reanchors += 1;
    }

    /// 插入一个包；重复包只计数，不覆盖已排队内容。
    pub fn push(&mut self, packet: AudioPacket, arrival_us: u64) {
        if self
            .last_arrival_us
            .is_some_and(|last| arrival_us.saturating_sub(last) > CLOCK_REANCHOR_IDLE_MS * 1_000)
        {
            self.reanchor();
        }
        self.last_arrival_us = Some(arrival_us);
        if self.first_arrival_us.is_none() {
            self.first_arrival_us = Some(arrival_us);
            self.first_pts_us = Some(packet.pts_us);
            self.next_seq = Some(packet.seq);
        }
        // RED 会重复携带前两包；陈旧/重复包不能参与抖动 EWMA，否则
        // 同一帧的旧时间戳会把目标窗口推到上限。
        if self.packets.contains_key(&packet.seq)
            || (self.stats.played > 0 && self.next_seq.is_some_and(|n| packet.seq < n))
        {
            self.stats.duplicates += 1;
            return;
        }

        let rel_pts = packet
            .pts_us
            .saturating_sub(self.first_pts_us.unwrap_or(packet.pts_us));
        let rel_arr = arrival_us.saturating_sub(self.first_arrival_us.unwrap_or(arrival_us));
        // 用到达时间对 RTP 时间戳做最小二乘拟合，估计持续时钟偏斜（ppm）。
        // 只对单调时间戳采样；抖动通过拟合平均掉，避免把单包抖动当成漂移。
        let x = rel_pts as f64;
        let raw_delay = rel_arr as i64 - rel_pts as i64;
        let y = raw_delay as f64;
        self.skew_n += 1;
        self.skew_sx += x;
        self.skew_sy += y;
        self.skew_sxx += x * x;
        self.skew_sxy += x * y;
        if self.skew_n >= 8 && self.skew_n % 8 == 0 {
            let n = self.skew_n as f64;
            let denom = n * self.skew_sxx - self.skew_sx * self.skew_sx;
            if denom.abs() > f64::EPSILON {
                let slope = (n * self.skew_sxy - self.skew_sx * self.skew_sy) / denom;
                self.clock_skew = slope.clamp(-0.01, 0.01);
                self.stats.clock_skew_ppm = (self.clock_skew * 1_000_000.0).round() as i32;
            }
        }
        let corrected_delay = raw_delay - (self.clock_skew * x) as i64;
        let abs = corrected_delay.max(0) as f64;
        // EWMA；前几个包快速收敛到实际抖动。
        self.jitter_ms = if self.stats.received < 4 {
            abs / 1000.0
        } else {
            self.jitter_ms * 0.875 + (abs / 1000.0) * 0.125
        };
        let target = (DEFAULT_TARGET_MS as f64 + 2.0 * self.jitter_ms)
            .round()
            .clamp(self.min_target_ms as f64, self.max_target_ms as f64)
            as u32;
        self.target_ms = target;
        self.stats.max_jitter_ms = self.stats.max_jitter_ms.max(self.jitter_ms.round() as u32);
        self.stats.received += 1;
        // 首包乱序到达时，队头应取最小 seq；已经播放过后不回退。
        if self.stats.played == 0 {
            if self.next_seq.map_or(true, |n| packet.seq < n) {
                self.next_seq = Some(packet.seq);
            }
        }
        self.packets
            .insert(packet.seq, Entry { packet, arrival_us });
    }

    /// 在 `now_us` 可播放的最小 seq 包；没有就返回 `None`。
    pub fn pop(&mut self, now_us: u64) -> Option<AudioPacket> {
        let first_arrival = self.first_arrival_us?;
        let first_pts = self.first_pts_us?;
        let next = *self.next_seq.as_ref()?;
        let due_base = first_arrival + (self.target_ms as u64 * 1000);

        if let Some(entry) = self.packets.get(&next) {
            let due = due_base + entry.packet.pts_us.saturating_sub(first_pts);
            if now_us >= due {
                let entry = self.packets.remove(&next).unwrap();
                self.next_seq = Some(next.wrapping_add(1));
                self.stats.played += 1;
                let delay_ms = (now_us.saturating_sub(entry.arrival_us) / 1000) as u32;
                self.stats.playout_delay_ms = self.stats.playout_delay_ms.max(delay_ms);
                return Some(entry.packet);
            }
            return None;
        }

        // 队头缺失：等目标时长；仍缺就跳到最小的已排队包并记一次丢包。
        let wait_until = due_base + (self.max_target_ms as u64 * 1000);
        if now_us < wait_until {
            return None;
        }
        let first = *self.packets.keys().next()?;
        self.stats.dropped_late += 1;
        self.next_seq = Some(first);
        let entry = self.packets.remove(&first)?;
        self.next_seq = Some(first.wrapping_add(1));
        self.stats.played += 1;
        Some(entry.packet)
    }

    /// 把 RED 包展开后插入；返回恢复的包数。
    pub fn push_red(&mut self, red: &AudioRed, arrival_us: u64) -> usize {
        let mut recovered = 0;
        for (i, p) in red.packets.iter().enumerate() {
            let before = self.packets.contains_key(&p.seq);
            self.push(p.clone(), arrival_us);
            if i > 0 && !before && self.packets.contains_key(&p.seq) {
                recovered += 1;
            }
        }
        self.stats.recovered_red += recovered as u64;
        recovered
    }
}

/// RED 载荷：`u8 count` + 每项 `u32 seq | u64 pts_us | u16 len | data`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioRed {
    pub packets: Vec<AudioPacket>,
}

impl AudioRed {
    pub fn new(packets: Vec<AudioPacket>) -> Result<Self, &'static str> {
        if packets.is_empty() || packets.len() > MAX_RED_PACKETS {
            return Err("red packet count");
        }
        let mut total = 1usize;
        for p in &packets {
            if p.data.is_empty() || p.data.len() > u16::MAX as usize {
                return Err("red payload length");
            }
            total += 4 + 8 + 2 + p.data.len();
            if total > MAX_RED_PAYLOAD {
                return Err("red too large");
            }
        }
        Ok(Self { packets })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(MAX_RED_PAYLOAD.min(1024));
        out.push(self.packets.len() as u8);
        for p in &self.packets {
            out.extend_from_slice(&p.seq.to_be_bytes());
            out.extend_from_slice(&p.pts_us.to_be_bytes());
            out.extend_from_slice(&(p.data.len() as u16).to_be_bytes());
            out.extend_from_slice(&p.data);
        }
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, &'static str> {
        let count = *buf.first().ok_or("red empty")? as usize;
        if count == 0 || count > MAX_RED_PACKETS {
            return Err("red packet count");
        }
        let mut off = 1usize;
        let mut packets = Vec::with_capacity(count);
        for _ in 0..count {
            if off + 14 > buf.len() {
                return Err("red truncated");
            }
            let seq = u32::from_be_bytes(buf[off..off + 4].try_into().unwrap());
            let pts_us = u64::from_be_bytes(buf[off + 4..off + 12].try_into().unwrap());
            let len = u16::from_be_bytes(buf[off + 12..off + 14].try_into().unwrap()) as usize;
            off += 14;
            if len == 0 || off + len > buf.len() {
                return Err("red payload length");
            }
            packets.push(AudioPacket {
                seq,
                pts_us,
                data: buf[off..off + len].to_vec(),
            });
            off += len;
        }
        if off != buf.len() {
            return Err("red trailing");
        }
        Self::new(packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(seq: u32, pts_ms: u64, byte: u8) -> AudioPacket {
        AudioPacket {
            seq,
            pts_us: pts_ms * 1000,
            data: vec![byte; 8],
        }
    }

    #[test]
    fn reorders_packets_within_target() {
        let mut jb = JitterBuffer::new(50);
        jb.push(p(2, 20, 2), 20_000);
        jb.push(p(1, 10, 1), 25_000);
        let first = jb.pop(90_000).unwrap();
        assert_eq!(first.seq, 1);
        let second = jb.pop(100_000).unwrap();
        assert_eq!(second.seq, 2);
    }

    #[test]
    fn target_adapts_to_200ms_jitter_and_stays_bounded() {
        let mut jb = JitterBuffer::new(50);
        for i in 0..40u32 {
            let pts = i as u64 * 10_000;
            let arrival = pts + if i % 2 == 0 { 0 } else { 200_000 };
            jb.push(
                AudioPacket {
                    seq: i,
                    pts_us: pts,
                    data: vec![1; 8],
                },
                arrival,
            );
        }
        assert!((MIN_TARGET_MS..=MAX_TARGET_MS).contains(&jb.target_ms()));
        assert!(jb.target_ms() >= 150);
        assert!(jb.stats().max_jitter_ms >= 190);
    }

    #[test]
    fn red_recovers_missing_packet() {
        let one = p(1, 10, 1);
        let two = p(2, 20, 2);
        let three = p(3, 30, 3);
        let red = AudioRed::new(vec![three.clone(), two.clone(), one.clone()]).unwrap();
        let bytes = red.encode();
        let decoded = AudioRed::decode(&bytes).unwrap();
        let mut jb = JitterBuffer::new(20);
        jb.push(one, 10_000);
        jb.push(three, 30_000);
        let recovered = jb.push_red(&decoded, 30_000);
        assert!(recovered >= 1);
        assert!(jb.packets.contains_key(&2));
        assert_eq!(decoded, red);
    }

    #[test]
    fn red_rejects_bad_shapes() {
        assert!(AudioRed::new(vec![]).is_err());
        assert!(AudioRed::new(vec![p(1, 0, 1); 4]).is_err());
        assert!(AudioRed::decode(&[0]).is_err());
        assert!(AudioRed::decode(&[1]).is_err());
    }

    #[test]
    fn clock_reanchors_after_idle() {
        let mut jb = JitterBuffer::new(50);
        jb.push(p(1, 10, 1), 10_000);
        assert_eq!(jb.pop(70_000).unwrap().seq, 1);
        jb.push(p(2, 20, 2), 6_000_000);
        let stats = jb.stats();
        assert_eq!(stats.clock_reanchors, 1);
        assert_eq!(stats.target_ms, DEFAULT_TARGET_MS);
    }

    #[test]
    fn clock_skew_is_estimated_and_removed_from_target() {
        let mut jb = JitterBuffer::new(50);
        let ppm = 1_000.0;
        for i in 0..6_000u32 {
            let pts = i as u64 * 10_000;
            let arrival = pts + 50_000 + (pts as f64 * ppm / 1_000_000.0) as u64;
            jb.push(
                AudioPacket {
                    seq: i,
                    pts_us: pts,
                    data: vec![1; 8],
                },
                arrival,
            );
        }
        let stats = jb.stats();
        assert!(
            (stats.clock_skew_ppm - 1_000).abs() <= 100,
            "skew={}",
            stats.clock_skew_ppm
        );
        assert!(jb.target_ms() < MAX_TARGET_MS, "target={}", jb.target_ms());
    }

    #[test]
    fn ten_minute_playout_delay_stays_bounded() {
        let mut jb = JitterBuffer::new(50);
        let mut delays = Vec::with_capacity(60_000);
        for i in 0..60_000u32 {
            let pts = i as u64 * 10_000;
            let jitter = ((i * 37) % 200) as u64 * 1_000;
            let arrival = pts + 50_000 + jitter;
            jb.push(
                AudioPacket {
                    seq: i,
                    pts_us: pts,
                    data: vec![1; 8],
                },
                arrival,
            );
            while let Some(packet) = jb.pop(arrival) {
                delays.push(arrival.saturating_sub(packet.pts_us));
            }
        }
        delays.sort_unstable();
        let p95 = delays[delays.len() * 95 / 100];
        assert!(p95 <= MAX_TARGET_MS as u64 * 1_000 + 100_000, "p95={p95}");
    }
}
