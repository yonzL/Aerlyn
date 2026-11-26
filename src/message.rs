//! V4 control 消息（JSON，内部 `type` 标签）。
//!
//! 参考 V3/V4 的 control 面：`VideoConfig` / `VideoReady` / `CodecConfig` /
//! `VideoAdjusted` / `DecodeError`。ch6 的 `FrameAck` 是固定 20 B 二进制，
//! 不走本模块的 JSON 注册表。
//! 未知 `type` 必须被保留并跳过，不能断连（V3 §8 的容错规则）。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::codec::{Codec, CodecCap};
use crate::control::{
    AudioStart, AudioStop, ChannelHello, ChannelWelcome, ClipAccept, ClipDone, ClipOffer, ClipPull,
    ControlOwner, DisplayChanged, DisplayList, ErrorReply, FileAccept, FileCancel, FileDelete,
    FileDone, FileErrorInfo, FileGet, FileList, FileListing, FileMeta, FileMkdir, FileOffer,
    FileRename, HostStats, InputConfig, InputState, InputStatus, OkReply, Ping, PlanePriorities,
    Pong, Rejected, RequestKeyframe, Resume, ResumeResult, ResumeTicket, Saved, Screenshot,
    SendCad, SessionEnded, SessionInfo, ShareRequest, VideoStop,
};
use crate::data::InputError;
use crate::ext::Extension;

/// 握手 Hello（control JSON，ch0）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// 单一版本号；冻结前不做版本协商。
    pub version: u16,
    pub public_key: Vec<u8>,
    pub caps: u64,
    #[serde(default)]
    pub caps_required: u64,
    #[serde(default)]
    pub codec_caps: Vec<CodecCap>,
    #[serde(default)]
    pub caps_ext: Vec<u8>,
    #[serde(default)]
    pub actor_host: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<Extension>,
}

impl Hello {
    pub fn video_caps(&self) -> crate::caps::VideoCaps {
        crate::caps::VideoCaps::from_bits(self.caps)
    }

    /// 功能能力位与 `version` 独立；未知位由 `FeatureCaps` 原样保留。
    pub fn feature_caps(&self) -> crate::caps::FeatureCaps {
        crate::caps::FeatureCaps::from_bits(self.caps)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Challenge {
    pub nonce: Vec<u8>,
    pub host_fp: Vec<u8>,
    pub version: u16,
    pub caps: u64,
    #[serde(default)]
    pub caps_ext: Vec<u8>,
}

impl Challenge {
    pub fn video_caps(&self) -> crate::caps::VideoCaps {
        crate::caps::VideoCaps::from_bits(self.caps)
    }

    pub fn feature_caps(&self) -> crate::caps::FeatureCaps {
        crate::caps::FeatureCaps::from_bits(self.caps)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Auth {
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Welcome {
    pub version: u16,
    pub caps: u64,
    pub session_id: u64,
    pub token: Vec<u8>,
    #[serde(default)]
    pub host_fp: Vec<u8>,
    /// 恢复票；新会话也要发，TTL 300 s。
    #[serde(default)]
    pub resume_ticket: Option<ResumeTicket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<Extension>,
}

impl Welcome {
    pub fn video_caps(&self) -> crate::caps::VideoCaps {
        crate::caps::VideoCaps::from_bits(self.caps)
    }

    pub fn feature_caps(&self) -> crate::caps::FeatureCaps {
        crate::caps::FeatureCaps::from_bits(self.caps)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bye {
    pub code: u16,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoConfig {
    pub req_id: u32,
    pub display: u8,
    /// 有序 codec id；空 = host 默认排序。
    #[serde(default)]
    pub codec_pref: Vec<u8>,
    pub max_fps: u16,
    pub max_kbps: u32,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub priority: u8,
    #[serde(default = "default_scale")]
    pub scale: u16,
    #[serde(default = "default_quality")]
    pub quality: u8,
    #[serde(default = "default_quality_mode")]
    pub quality_mode: u8,
    #[serde(default)]
    pub chroma_pref: u8,
    #[serde(default)]
    pub bit_depth_pref: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<Extension>,
}

fn default_scale() -> u16 {
    100
}
fn default_quality() -> u8 {
    70
}
fn default_quality_mode() -> u8 {
    2
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            req_id: 0,
            display: 0,
            codec_pref: Vec::new(),
            max_fps: 60,
            max_kbps: 8_000,
            active: false,
            priority: 0,
            scale: 100,
            quality: 70,
            quality_mode: 2,
            chroma_pref: 0,
            bit_depth_pref: 0,
            extensions: Vec::new(),
        }
    }
}

impl VideoConfig {
    /// 过滤未知 codec id，得到可交给 `select_codec` 的顺序。
    pub fn preference(&self) -> Vec<Codec> {
        self.codec_pref
            .iter()
            .filter_map(|id| Codec::from_id(*id))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoReady {
    pub req_id: u32,
    pub display: u8,
    pub codec: u8,
    pub profile: u8,
    pub level: u16,
    pub chroma: u8,
    pub bit_depth: u8,
    pub width: u16,
    pub height: u16,
    pub max_fps: u16,
    pub max_kbps: u32,
    #[serde(default)]
    pub hw: bool,
}

impl VideoReady {
    pub fn codec_kind(&self) -> Option<Codec> {
        Codec::from_id(self.codec)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodecConfig {
    pub display: u8,
    pub codec: u8,
    pub profile: u8,
    pub level: u16,
    pub chroma: u8,
    pub bit_depth: u8,
    /// H.264 SPS/PPS、H.265 VPS/SPS/PPS、AV1 seq header；VP8/VP9/RAW 为空。
    #[serde(default)]
    pub extradata: Vec<u8>,
}

impl CodecConfig {
    pub fn codec_kind(&self) -> Option<Codec> {
        Codec::from_id(self.codec)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdjustReason {
    Capability,
    Congestion,
    EncoderInit,
    DecoderError,
    CodecFallback,
    QualityMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoAdjusted {
    pub display: u8,
    pub max_fps: u16,
    pub actual_fps: u16,
    pub reason: AdjustReason,
    #[serde(default)]
    pub retry_after_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecodeError {
    pub display: u8,
    pub codec: u8,
    pub frame_id: u32,
    pub reason: String,
}

/// ch6 `type=1` 的累计确认；线上是固定 20 B，不是 JSON。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameAck {
    pub ack_frame_id: u32,
    pub recv_ts_us: u64,
    pub decode_done_us: u32,
    pub queue_depth: u16,
    pub dropped: u16,
}

impl FrameAck {
    /// ch6 `type=1`，固定 20 B，与 V3 §11.3 的累计确认一致。
    pub const LEN: usize = 20;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..4].copy_from_slice(&self.ack_frame_id.to_be_bytes());
        out[4..12].copy_from_slice(&self.recv_ts_us.to_be_bytes());
        out[12..16].copy_from_slice(&self.decode_done_us.to_be_bytes());
        out[16..18].copy_from_slice(&self.queue_depth.to_be_bytes());
        out[18..20].copy_from_slice(&self.dropped.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() < Self::LEN {
            return Err(InputError::TooShort);
        }
        if buf.len() != Self::LEN {
            return Err(InputError::Trailing);
        }
        Ok(Self {
            ack_frame_id: u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]),
            recv_ts_us: u64::from_be_bytes([
                buf[4], buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11],
            ]),
            decode_done_us: u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]),
            queue_depth: u16::from_be_bytes([buf[16], buf[17]]),
            dropped: u16::from_be_bytes([buf[18], buf[19]]),
        })
    }
}

/// 由同一张表生成 control 类型注册表、`type_name()` 和 JSON 分派，避免三处漂移。
macro_rules! control_dispatch {
    ($($variant:ident => $ty:ty),* $(,)?) => {
        pub const CONTROL_MESSAGE_TYPES: &[&str] = &[$(stringify!($variant)),*];

        impl ControlMessage {
            pub fn type_name(&self) -> &str {
                match self {
                    $(Self::$variant(_) => stringify!($variant),)*
                    Self::Unknown { type_name, .. } => type_name,
                }
            }

            pub fn from_json(input: &str) -> Result<Self, MessageError> {
                let value: Value = serde_json::from_str(input)?;
                let type_name = value
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or(MessageError::MissingType)?
                    .to_owned();
                let parsed = match type_name.as_str() {
                    $(stringify!($variant) => {
                        serde_json::from_value::<$ty>(value).map(Self::$variant)
                    })*
                    _ => {
                        return Ok(Self::Unknown {
                            type_name,
                            raw: value,
                        })
                    }
                };
                parsed.map_err(MessageError::Json)
            }

            pub fn to_json(&self) -> Result<String, MessageError> {
                match self {
                    Self::Unknown { raw, .. } => Ok(serde_json::to_string(raw)?),
                    $(Self::$variant(m) => tagged(stringify!($variant), m),)*
                }
            }
        }
    };
}

/// control 消息。未知类型保留原始 JSON，交给上层计数/忽略。
#[derive(Debug, Clone, PartialEq)]
pub enum ControlMessage {
    Hello(Hello),
    Challenge(Challenge),
    Auth(Auth),
    Welcome(Welcome),
    Resume(Resume),
    ResumeResult(ResumeResult),
    ChannelHello(ChannelHello),
    ChannelWelcome(ChannelWelcome),
    Rejected(Rejected),
    Bye(Bye),
    VideoConfig(VideoConfig),
    VideoReady(VideoReady),
    CodecConfig(CodecConfig),
    VideoAdjusted(VideoAdjusted),
    DecodeError(DecodeError),
    Ok(OkReply),
    Error(ErrorReply),
    VideoStop(VideoStop),
    RequestKeyframe(RequestKeyframe),
    DisplayList(DisplayList),
    DisplayChanged(DisplayChanged),
    InputConfig(InputConfig),
    InputState(InputState),
    InputStatus(InputStatus),
    FileOffer(FileOffer),
    FileGet(FileGet),
    FileList(FileList),
    FileCancel(FileCancel),
    FileAccept(FileAccept),
    FileMeta(FileMeta),
    FileListing(FileListing),
    FileDone(FileDone),
    FileError(FileErrorInfo),
    FileMkdir(FileMkdir),
    FileDelete(FileDelete),
    FileRename(FileRename),
    ClipPull(ClipPull),
    ClipOffer(ClipOffer),
    ClipAccept(ClipAccept),
    ClipDone(ClipDone),
    AudioStart(AudioStart),
    AudioStop(AudioStop),
    Screenshot(Screenshot),
    Saved(Saved),
    SendCad(SendCad),
    SessionInfo(SessionInfo),
    SessionEnded(SessionEnded),
    ControlOwner(ControlOwner),
    ShareRequest(ShareRequest),
    HostStats(HostStats),
    Ping(Ping),
    Pong(Pong),
    PlanePriorities(PlanePriorities),
    Unknown { type_name: String, raw: Value },
}

// `serde_json::Value` has a total equality relation for the finite JSON values
// this protocol accepts, but it does not implement `Eq` itself. This marker
// keeps `SessionMachine` (which retains unknown messages) `Eq`.
impl Eq for ControlMessage {}

control_dispatch! {
    AudioStart => AudioStart,
    AudioStop => AudioStop,
    Auth => Auth,
    Bye => Bye,
    Challenge => Challenge,
    ChannelHello => ChannelHello,
    ChannelWelcome => ChannelWelcome,
    ClipAccept => ClipAccept,
    ClipDone => ClipDone,
    ClipOffer => ClipOffer,
    ClipPull => ClipPull,
    CodecConfig => CodecConfig,
    ControlOwner => ControlOwner,
    DecodeError => DecodeError,
    DisplayChanged => DisplayChanged,
    DisplayList => DisplayList,
    Error => ErrorReply,
    FileAccept => FileAccept,
    FileDelete => FileDelete,
    FileDone => FileDone,
    FileCancel => FileCancel,
    FileError => FileErrorInfo,
    FileGet => FileGet,
    FileList => FileList,
    FileListing => FileListing,
    FileMeta => FileMeta,
    FileMkdir => FileMkdir,
    FileOffer => FileOffer,
    FileRename => FileRename,
    Hello => Hello,
    HostStats => HostStats,
    InputConfig => InputConfig,
    InputState => InputState,
    InputStatus => InputStatus,
    Ok => OkReply,
    Ping => Ping,
    PlanePriorities => PlanePriorities,
    Pong => Pong,
    Rejected => Rejected,
    RequestKeyframe => RequestKeyframe,
    Resume => Resume,
    ResumeResult => ResumeResult,
    Saved => Saved,
    Screenshot => Screenshot,
    SendCad => SendCad,
    SessionEnded => SessionEnded,
    SessionInfo => SessionInfo,
    ShareRequest => ShareRequest,
    VideoAdjusted => VideoAdjusted,
    VideoConfig => VideoConfig,
    VideoReady => VideoReady,
    VideoStop => VideoStop,
    Welcome => Welcome,
}

#[derive(Debug)]
pub enum MessageError {
    Json(serde_json::Error),
    MissingType,
}

impl From<serde_json::Error> for MessageError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

fn tagged<T: Serialize>(name: &str, payload: &T) -> Result<String, MessageError> {
    let mut value = serde_json::to_value(payload)?;
    match value.as_object_mut() {
        Some(obj) => {
            obj.insert("type".to_owned(), Value::String(name.to_owned()));
        }
        None => {
            value = json!({ "type": name });
        }
    }
    Ok(serde_json::to_string(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_config_roundtrip() {
        let m = ControlMessage::VideoConfig(VideoConfig {
            req_id: 7,
            display: 1,
            codec_pref: vec![4, 1, 3, 0, 2, 5],
            max_fps: 60,
            max_kbps: 8_000,
            active: true,
            priority: 1,
            quality_mode: 2,
            ..VideoConfig::default()
        });
        let json = m.to_json().unwrap();
        assert!(json.contains("\"type\":\"VideoConfig\""));
        assert_eq!(ControlMessage::from_json(&json).unwrap(), m);
        let ControlMessage::VideoConfig(c) = m else {
            panic!("expected VideoConfig");
        };
        assert_eq!(
            c.preference(),
            vec![
                Codec::Av1,
                Codec::H265,
                Codec::Vp9,
                Codec::H264,
                Codec::Vp8,
                Codec::Raw
            ]
        );
    }

    #[test]
    fn unknown_type_is_preserved() {
        let raw = r#"{"type":"FutureCodec","frame":42,"x":1}"#;
        let m = ControlMessage::from_json(raw).unwrap();
        match &m {
            ControlMessage::Unknown { type_name, raw } => {
                assert_eq!(type_name, "FutureCodec");
                assert_eq!(raw["frame"], 42);
            }
            _ => panic!("expected Unknown"),
        }
        assert_eq!(m.type_name(), "FutureCodec");
        assert!(m.to_json().unwrap().contains("FutureCodec"));
    }

    #[test]
    fn missing_type_rejected() {
        assert!(matches!(
            ControlMessage::from_json(r#"{"max_fps":60}"#),
            Err(MessageError::MissingType)
        ));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{"type":"VideoConfig","req_id":1,"display":0,"max_fps":30,"max_kbps":4000,"future_field":true}"#;
        let m = ControlMessage::from_json(json).unwrap();
        match m {
            ControlMessage::VideoConfig(c) => {
                assert_eq!(c.req_id, 1);
                assert_eq!(c.max_fps, 30);
            }
            _ => panic!("expected VideoConfig"),
        }
    }

    #[test]
    fn other_messages_roundtrip() {
        let messages = vec![
            ControlMessage::VideoReady(VideoReady {
                req_id: 1,
                display: 0,
                codec: Codec::Av1.id(),
                profile: 1,
                level: 13,
                chroma: crate::codec::chroma::I420,
                bit_depth: crate::codec::bit_depth::EIGHT,
                width: 1920,
                height: 1080,
                max_fps: 60,
                max_kbps: 8_000,
                hw: true,
            }),
            ControlMessage::CodecConfig(CodecConfig {
                display: 0,
                codec: Codec::H264.id(),
                profile: 2,
                level: 51,
                chroma: crate::codec::chroma::I420,
                bit_depth: crate::codec::bit_depth::EIGHT,
                extradata: vec![0, 0, 0, 1, 0x67, 0x64],
            }),
            ControlMessage::VideoAdjusted(VideoAdjusted {
                display: 0,
                max_fps: 60,
                actual_fps: 37,
                reason: AdjustReason::Congestion,
                retry_after_ms: 500,
            }),
            ControlMessage::DecodeError(DecodeError {
                display: 0,
                codec: Codec::H265.id(),
                frame_id: 9,
                reason: "no hw decoder".into(),
            }),
        ];
        for m in messages {
            let json = m.to_json().unwrap();
            assert_eq!(ControlMessage::from_json(&json).unwrap(), m);
        }
    }

    #[test]
    fn full_control_registry_roundtrip() {
        let messages = vec![
            ControlMessage::Ok(OkReply { req_id: 1 }),
            ControlMessage::Error(ErrorReply {
                req_id: 2,
                code: 61,
                class: 1,
                need: 0,
                retry_after_ms: 250,
            }),
            ControlMessage::VideoStop(VideoStop { display: 1 }),
            ControlMessage::RequestKeyframe(RequestKeyframe {
                display: 0,
                reason: 2,
            }),
            ControlMessage::DisplayList(DisplayList {
                displays: vec![crate::control::DisplayInfo {
                    id: 0,
                    x: -1920,
                    y: 0,
                    w: 1920,
                    h: 1080,
                    scale: 100,
                    primary: false,
                    name: "left".into(),
                    display_key: 2,
                }],
            }),
            ControlMessage::DisplayChanged(DisplayChanged {
                displays: Vec::new(),
            }),
            ControlMessage::InputConfig(InputConfig {
                req_id: 3,
                enabled: true,
                keyboard: true,
                mouse: false,
                touch: false,
            }),
            ControlMessage::InputState(InputState {
                mods: 0x02,
                pressed_keys: vec![65, 66],
                release_all: false,
            }),
            ControlMessage::InputStatus(InputStatus {
                seq: 9,
                code: 0,
                injected: 16,
                failed: 0,
            }),
            ControlMessage::FileOffer(FileOffer {
                task_id: 4,
                dir: "D:/recv".into(),
                name: "a.bin".into(),
                size: 1 << 30,
                sha256: vec![1; 32],
                block_size: 1 << 20,
                credit: crate::transfer::DEFAULT_FILE_CREDIT,
                block_sha_root: vec![2; 32],
                mtime_ms: 1_700_000_000_000,
                conflict: 2,
                compress: true,
                resume: true,
                extensions: Vec::new(),
            }),
            ControlMessage::FileAccept(FileAccept {
                task_id: 4,
                resume_offset: 1 << 20,
                received_bitmap: vec![0xff],
                block_sha256: vec![vec![3; 32]],
                final_name: Some("a (1).bin".into()),
                error: None,
                extensions: Vec::new(),
            }),
            ControlMessage::FileMeta(FileMeta {
                task_id: 5,
                size: 42,
                sha256: vec![4; 32],
                mtime_ms: 0,
                block_size: 1 << 20,
                block_sha256: Vec::new(),
            }),
            ControlMessage::FileListing(FileListing {
                task_id: 6,
                entries: vec![crate::control::FileEntry {
                    name: "x".into(),
                    is_dir: true,
                    size: 0,
                    mtime_ms: 0,
                    attrs: 0,
                }],
                next_offset: 0,
                total: 1,
            }),
            ControlMessage::FileDone(FileDone {
                task_id: 4,
                ok: true,
                size: 42,
                error_code: None,
            }),
            ControlMessage::FileError(FileErrorInfo {
                task_id: 7,
                code: 31,
                detail: "disk full".into(),
            }),
            ControlMessage::ClipPull(ClipPull {}),
            ControlMessage::ClipOffer(ClipOffer {
                seq: 8,
                owner: 2,
                items: vec![crate::control::ClipItem {
                    format: 0,
                    size: 5,
                    sha256_prefix: vec![5; 8],
                }],
            }),
            ControlMessage::ClipAccept(ClipAccept {
                seq: 8,
                formats: vec![0, 1],
                credit: 4,
            }),
            ControlMessage::ClipDone(ClipDone { seq: 8 }),
            ControlMessage::AudioStart(AudioStart {
                codec: 0,
                rate: 48_000,
                ch: 2,
                frame_ms: 10,
            }),
            ControlMessage::AudioStop(AudioStop {}),
            ControlMessage::HostStats(HostStats {
                max_fps: 60,
                actual_fps: 59,
                accepted_max_kbps: 8_000,
                capture_ms: 2,
                encode_ms: 4,
                drop_frames: 0,
                codec: Codec::Av1.id(),
            }),
            ControlMessage::Ping(Ping {
                seq: 10,
                ts_ms: 1_700_000_000_000,
            }),
            ControlMessage::Pong(Pong {
                seq: 10,
                ts_ms: 1_700_000_000_001,
                rtt_ms: 1,
            }),
            ControlMessage::Resume(Resume {
                ticket_id: 1,
                nonce: [7; 16],
                last_frame_id: 42,
                last_input_seq: 9,
            }),
            ControlMessage::ResumeResult(ResumeResult {
                ok: true,
                code: 0,
                need: 0,
                ticket: Some(ResumeTicket {
                    id: 2,
                    nonce: [8; 16],
                    exp_ms: 1_700_000_300_000,
                }),
                session_id: 42,
            }),
            ControlMessage::ChannelHello(ChannelHello {
                channel: 3,
                session_id: 42,
                token: vec![9; 32],
            }),
            ControlMessage::ChannelWelcome(ChannelWelcome { channel: 3 }),
            ControlMessage::Rejected(Rejected { code: 8, need: 0 }),
            ControlMessage::Screenshot(Screenshot {
                display: 0,
                format: 0,
            }),
            ControlMessage::Saved(Saved {
                path: "D:/shot.png".into(),
            }),
            ControlMessage::SendCad(SendCad { display: 0 }),
            ControlMessage::FileMkdir(FileMkdir {
                req_id: 11,
                dir: "D:/recv".into(),
            }),
            ControlMessage::FileDelete(FileDelete {
                req_id: 12,
                dir: "D:/recv".into(),
                name: "a.bin".into(),
                recycle: true,
            }),
            ControlMessage::FileRename(FileRename {
                req_id: 13,
                dir: "D:/recv".into(),
                from: "a.bin".into(),
                to: "b.bin".into(),
            }),
            ControlMessage::SessionInfo(SessionInfo {
                session_id: 42,
                share: 1,
                control_owner: Some(7),
                client_name: "client".into(),
            }),
            ControlMessage::SessionEnded(SessionEnded {
                code: 0,
                reason: "done".into(),
            }),
            ControlMessage::ControlOwner(ControlOwner {
                owner: 7,
                since_ms: 1_700_000_000_000,
            }),
            ControlMessage::ShareRequest(ShareRequest {
                req_id: 14,
                share: 2,
            }),
        ];
        for m in messages {
            let json = m.to_json().unwrap();
            assert_eq!(
                ControlMessage::from_json(&json).unwrap(),
                m,
                "type {}",
                m.type_name()
            );
        }
    }

    #[test]
    fn reason_serializes_screaming_snake() {
        let m = ControlMessage::VideoAdjusted(VideoAdjusted {
            display: 0,
            max_fps: 60,
            actual_fps: 0,
            reason: AdjustReason::EncoderInit,
            retry_after_ms: 0,
        });
        assert!(m.to_json().unwrap().contains("\"ENCODER_INIT\""));
    }

    #[test]
    fn frame_ack_binary_roundtrip() {
        let ack = FrameAck {
            ack_frame_id: 42,
            recv_ts_us: 1_700_000_000_000,
            decode_done_us: 7_000,
            queue_depth: 2,
            dropped: 1,
        };
        let bytes = ack.encode();
        assert_eq!(bytes.len(), FrameAck::LEN);
        assert_eq!(FrameAck::decode(&bytes).unwrap(), ack);
        assert_eq!(
            FrameAck::decode(&bytes[..FrameAck::LEN - 1]),
            Err(InputError::TooShort)
        );
        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert_eq!(FrameAck::decode(&trailing), Err(InputError::Trailing));
    }

    #[test]
    fn file_offer_carries_tlv_extensions_end_to_end() {
        let mut extensions = crate::ext::Extensions::new();
        extensions.push(7, vec![1, 2, 3]).unwrap();
        extensions.push(9, vec![4, 5]).unwrap();
        let encoded = extensions.encode().unwrap();

        let offer = FileOffer {
            task_id: 1,
            dir: "D:/recv".into(),
            name: "a.bin".into(),
            size: 3,
            sha256: Vec::new(),
            block_size: 1 << 20,
            credit: crate::transfer::DEFAULT_FILE_CREDIT,
            block_sha_root: Vec::new(),
            mtime_ms: 0,
            conflict: 1,
            compress: false,
            resume: false,
            extensions: encoded.clone(),
        };
        let message = ControlMessage::FileOffer(offer);
        let json = message.to_json().unwrap();
        let decoded_message = ControlMessage::from_json(&json).unwrap();
        let decoded_offer = match decoded_message {
            ControlMessage::FileOffer(offer) => offer,
            _ => panic!("expected FileOffer"),
        };
        assert_eq!(decoded_offer.extensions, encoded);

        let (known, skipped) =
            crate::ext::Extensions::decode(&decoded_offer.extensions, &[7]).unwrap();
        assert_eq!(known.get(7), Some(&[1, 2, 3][..]));
        assert_eq!(skipped, 1);
    }

    #[test]
    fn extensions_survive_four_evolvable_messages() {
        use crate::ext::{Extension, Extensions};

        let extensions = vec![
            Extension {
                tag: 7,
                value: vec![1, 2],
            },
            Extension {
                tag: 9,
                value: vec![3],
            },
        ];

        let hello = ControlMessage::Hello(Hello {
            version: 400,
            public_key: vec![1; 32],
            caps: 1,
            caps_required: 0,
            codec_caps: Vec::new(),
            caps_ext: Vec::new(),
            actor_host: "host".into(),
            extensions: extensions.clone(),
        });
        match ControlMessage::from_json(&hello.to_json().unwrap()).unwrap() {
            ControlMessage::Hello(m) => assert_eq!(m.extensions, extensions),
            _ => panic!("expected Hello"),
        }

        let welcome = ControlMessage::Welcome(Welcome {
            version: 400,
            caps: 1,
            session_id: 2,
            token: vec![3; 32],
            host_fp: vec![4; 32],
            resume_ticket: None,
            extensions: extensions.clone(),
        });
        match ControlMessage::from_json(&welcome.to_json().unwrap()).unwrap() {
            ControlMessage::Welcome(m) => assert_eq!(m.extensions, extensions),
            _ => panic!("expected Welcome"),
        }

        let video = ControlMessage::VideoConfig(VideoConfig {
            extensions: extensions.clone(),
            ..VideoConfig::default()
        });
        match ControlMessage::from_json(&video.to_json().unwrap()).unwrap() {
            ControlMessage::VideoConfig(m) => assert_eq!(m.extensions, extensions),
            _ => panic!("expected VideoConfig"),
        }

        let accept = ControlMessage::FileAccept(FileAccept {
            task_id: 5,
            resume_offset: 0,
            received_bitmap: Vec::new(),
            block_sha256: Vec::new(),
            final_name: None,
            error: None,
            extensions: extensions.clone(),
        });
        match ControlMessage::from_json(&accept.to_json().unwrap()).unwrap() {
            ControlMessage::FileAccept(m) => assert_eq!(m.extensions, extensions),
            _ => panic!("expected FileAccept"),
        }

        let mut block = Extensions::new();
        block.push(7, vec![1, 2]).unwrap();
        block.push(9, vec![3]).unwrap();
        let (known, skipped) = Extensions::decode(&block.encode().unwrap(), &[7]).unwrap();
        assert_eq!(known.get(7), Some(&[1, 2][..]));
        assert_eq!(skipped, 1);
    }
}
