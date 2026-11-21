//! 画质设置，结构参考 Airhand `airhand-proto/src/session.rs` 的 `QualitySettings`：
//! fps/bitrate_kbps/max_short/adaptive/codec/priority，host 侧做范围校验。

use crate::codec::Codec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VideoPriority {
    Latency = 0,
    Clarity = 1,
}

impl VideoPriority {
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Latency),
            1 => Some(Self::Clarity),
            _ => None,
        }
    }

    pub fn id(self) -> u8 {
        self as u8
    }

    /// 不认识的优先项按低延迟（Airhand 的做法）。
    pub fn or_latency(self) -> Self {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualitySettings {
    /// 最大帧率 1..=120。
    pub fps: u8,
    /// 目标码率 kbps。
    pub bitrate_kbps: u32,
    /// 输出画面短边上限（720/1080…），0 = 不限制。
    pub max_short: u16,
    /// 根据网络自动下调/恢复码率。
    pub adaptive: bool,
    /// 期望编码；不支持时回退 H.264。
    pub codec: Codec,
    pub priority: VideoPriority,
}

impl Default for QualitySettings {
    fn default() -> Self {
        Self {
            fps: 30,
            bitrate_kbps: 4_000,
            max_short: 1080,
            adaptive: true,
            codec: Codec::H264,
            priority: VideoPriority::Latency,
        }
    }
}

impl QualitySettings {
    /// 范围校验；RAW 不作为质量档的首选，回退 H.264。
    pub fn sanitized(mut self) -> Self {
        self.fps = self.fps.clamp(1, 120);
        self.bitrate_kbps = self.bitrate_kbps.clamp(100, 100_000);
        if self.codec.is_raw() {
            self.codec = Codec::H264;
        }
        self
    }

    /// 多 viewer 合并（Airhand 的共享设置规则）：
    /// 最高 fps、最高码率、最严格短边、任一 adaptive、公共编码（不同则 H.264）。
    pub fn merge_viewers(settings: &[Self]) -> Self {
        let Some(first) = settings.first() else {
            return Self::default();
        };
        let fps = settings.iter().map(|s| s.fps).max().unwrap_or(first.fps);
        let bitrate = settings
            .iter()
            .map(|s| s.bitrate_kbps)
            .max()
            .unwrap_or(first.bitrate_kbps);
        let max_short = settings
            .iter()
            .map(|s| s.max_short)
            .filter(|&v| v != 0)
            .min()
            .unwrap_or(0);
        let adaptive = settings.iter().any(|s| s.adaptive);
        let codec = if settings.iter().all(|s| s.codec == first.codec) {
            first.codec
        } else {
            Codec::H264
        };
        let priority = if settings
            .iter()
            .any(|s| s.priority == VideoPriority::Clarity)
        {
            VideoPriority::Clarity
        } else {
            VideoPriority::Latency
        };
        Self {
            fps,
            bitrate_kbps: bitrate,
            max_short,
            adaptive,
            codec,
            priority,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_airhand_shape() {
        let q = QualitySettings::default();
        assert_eq!((q.fps, q.bitrate_kbps, q.max_short), (30, 4_000, 1080));
        assert!(q.adaptive);
        assert_eq!(q.codec, Codec::H264);
        assert_eq!(q.priority, VideoPriority::Latency);
    }

    #[test]
    fn sanitize_clamps_and_removes_raw() {
        let q = QualitySettings {
            fps: 200,
            bitrate_kbps: 1,
            max_short: 0,
            adaptive: false,
            codec: Codec::Raw,
            priority: VideoPriority::Clarity,
        }
        .sanitized();
        assert_eq!(q.fps, 120);
        assert_eq!(q.bitrate_kbps, 100);
        assert_eq!(q.codec, Codec::H264);
        assert_eq!(q.max_short, 0);
    }

    #[test]
    fn merge_takes_most_demanding_and_common_codec() {
        let a = QualitySettings {
            fps: 60,
            bitrate_kbps: 8_000,
            max_short: 1080,
            adaptive: true,
            codec: Codec::Av1,
            priority: VideoPriority::Clarity,
        };
        let b = QualitySettings {
            fps: 30,
            bitrate_kbps: 4_000,
            max_short: 720,
            adaptive: false,
            codec: Codec::H265,
            priority: VideoPriority::Latency,
        };
        let merged = QualitySettings::merge_viewers(&[a, b]);
        assert_eq!(merged.fps, 60);
        assert_eq!(merged.bitrate_kbps, 8_000);
        assert_eq!(merged.max_short, 720);
        assert!(merged.adaptive);
        assert_eq!(
            merged.codec,
            Codec::H264,
            "different codecs -> common H.264"
        );
        assert_eq!(merged.priority, VideoPriority::Clarity);
    }

    #[test]
    fn merge_same_codec_keeps_it() {
        let a = QualitySettings::default();
        let b = QualitySettings {
            fps: 15,
            ..QualitySettings::default()
        };
        let merged = QualitySettings::merge_viewers(&[a, b]);
        assert_eq!(merged.codec, Codec::H264);
        assert_eq!(merged.fps, 30);
    }
}
