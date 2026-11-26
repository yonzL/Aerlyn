//! V3/V4 control 面完整消息集（JSON，ch0）。
//!
//! 这里只定义线上形状与字段范围，不包含会话策略。未知 type 由
//! [`crate::message::ControlMessage`] 保留原始 JSON，不能因为解码失败断连。

use serde::{Deserialize, Serialize};

use crate::error::{ErrorClass, ErrorInfo};
use crate::ext::Extension;

fn yes() -> bool {
    true
}

/// 通用成功答复，`req_id` 原样带回。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OkReply {
    pub req_id: u32,
}

/// 通用错误答复；字段与 V3 `Error` 平铺一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorReply {
    pub req_id: u32,
    pub code: u16,
    pub class: u8,
    #[serde(default)]
    pub need: u64,
    #[serde(default)]
    pub retry_after_ms: u32,
}

impl ErrorReply {
    pub fn from_info(req_id: u32, info: &ErrorInfo) -> Self {
        Self {
            req_id,
            code: info.code,
            class: info.class,
            need: info.need,
            retry_after_ms: info.retry_after_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoStop {
    pub display: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestKeyframe {
    pub display: u8,
    #[serde(default)]
    pub reason: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub id: u8,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    #[serde(default = "default_scale")]
    pub scale: u16,
    #[serde(default)]
    pub primary: bool,
    #[serde(default)]
    pub name: String,
    /// Stable identity across index/geometry changes; 0 means unknown.
    #[serde(default)]
    pub display_key: u64,
}

fn default_scale() -> u16 {
    100
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayList {
    pub displays: Vec<DisplayInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayChanged {
    pub displays: Vec<DisplayInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputConfig {
    pub req_id: u32,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub keyboard: bool,
    #[serde(default = "yes")]
    pub mouse: bool,
    #[serde(default = "yes")]
    pub touch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputState {
    #[serde(default)]
    pub mods: u16,
    #[serde(default)]
    pub pressed_keys: Vec<u16>,
    #[serde(default)]
    pub release_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputStatus {
    pub seq: u32,
    pub code: u8,
    pub injected: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOffer {
    pub task_id: u32,
    pub dir: String,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub sha256: Vec<u8>,
    #[serde(default = "default_block_size")]
    pub block_size: u32,
    /// 发送方希望接收方广告的未确认块窗口；接收方必须采纳，否则自适应窗口
    /// 会在第一条 ack 后被固定窗口覆盖。
    #[serde(default = "default_file_credit")]
    pub credit: u16,
    #[serde(default)]
    pub block_sha_root: Vec<u8>,
    #[serde(default)]
    pub mtime_ms: u64,
    #[serde(default = "default_conflict")]
    pub conflict: u8,
    #[serde(default)]
    pub compress: bool,
    #[serde(default)]
    pub resume: bool,
    /// 扩展块，内容为 `Extensions::encode` 的 TLV 字节；未知 tag 由接收端跳过并计数。
    #[serde(default)]
    pub extensions: Vec<u8>,
}

fn default_block_size() -> u32 {
    1 << 20
}
fn default_file_credit() -> u16 {
    crate::transfer::DEFAULT_FILE_CREDIT
}
fn default_conflict() -> u8 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAccept {
    pub task_id: u32,
    #[serde(default)]
    pub resume_offset: u64,
    #[serde(default)]
    pub received_bitmap: Vec<u8>,
    #[serde(default)]
    pub block_sha256: Vec<Vec<u8>>,
    #[serde(default)]
    pub final_name: Option<String>,
    #[serde(default)]
    pub error: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<Extension>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeta {
    pub task_id: u32,
    pub size: u64,
    #[serde(default)]
    pub sha256: Vec<u8>,
    #[serde(default)]
    pub mtime_ms: u64,
    #[serde(default = "default_block_size")]
    pub block_size: u32,
    #[serde(default)]
    pub block_sha256: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime_ms: u64,
    #[serde(default)]
    pub attrs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileListing {
    pub task_id: u32,
    pub entries: Vec<FileEntry>,
    #[serde(default)]
    pub next_offset: u32,
    #[serde(default)]
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDone {
    pub task_id: u32,
    pub ok: bool,
    pub size: u64,
    #[serde(default)]
    pub error_code: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileErrorInfo {
    pub task_id: u32,
    pub code: u16,
    #[serde(default)]
    pub detail: String,
}

/// 客户端请求下载一个已存在文件；`offset` 为续传起点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileGet {
    pub task_id: u32,
    pub dir: String,
    pub name: String,
    #[serde(default)]
    pub offset: u64,
}

pub const DEFAULT_FILE_LIST_LIMIT: u32 = 500;
pub const MAX_FILE_LIST_LIMIT: u32 = 2_000;

/// 分页列出目录。`task_id` 由请求方生成，服务端在 `FileListing` 中原样回显。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileList {
    pub task_id: u32,
    pub dir: String,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_file_list_limit")]
    pub limit: u32,
    #[serde(default)]
    pub show_hidden: bool,
}

fn default_file_list_limit() -> u32 {
    DEFAULT_FILE_LIST_LIMIT
}

impl FileList {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.limit == 0 || self.limit > MAX_FILE_LIST_LIMIT {
            return Err("FileList.limit must be 1..=2000");
        }
        Ok(())
    }
}

/// 取消一个尚未完成的文件任务；服务端回 `Ok{req_id=task_id}`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileCancel {
    pub task_id: u32,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipPull {}

/// 剪贴板格式值域。值在线路上仍是 `u16`；未知值保留给未来扩展，
/// 但核心五种格式在此固定，避免各端各自猜。
pub mod clip_format {
    pub const TEXT: u16 = 0;
    pub const RTF: u16 = 1;
    pub const HTML: u16 = 2;
    pub const PNG: u16 = 3;
    pub const DIB: u16 = 4;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipFormat {
    Text,
    Rtf,
    Html,
    Png,
    Dib,
    Unknown(u16),
}

impl ClipFormat {
    pub fn from_u16(value: u16) -> Self {
        match value {
            clip_format::TEXT => Self::Text,
            clip_format::RTF => Self::Rtf,
            clip_format::HTML => Self::Html,
            clip_format::PNG => Self::Png,
            clip_format::DIB => Self::Dib,
            other => Self::Unknown(other),
        }
    }

    pub fn as_u16(self) -> u16 {
        match self {
            Self::Text => clip_format::TEXT,
            Self::Rtf => clip_format::RTF,
            Self::Html => clip_format::HTML,
            Self::Png => clip_format::PNG,
            Self::Dib => clip_format::DIB,
            Self::Unknown(v) => v,
        }
    }

    pub fn is_known(self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

/// 单个剪贴板条目的上限（协议块窗口）。
pub const MAX_CLIP_TOTAL_BYTES: u64 = 64 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipItem {
    pub format: u16,
    pub size: u32,
    #[serde(default)]
    pub sha256_prefix: Vec<u8>,
}

impl ClipItem {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !ClipFormat::from_u16(self.format).is_known() {
            return Err("unknown clipboard format");
        }
        if self.size == 0 || u64::from(self.size) > u64::from(crate::transfer::MAX_CLIP_BYTES) {
            return Err("clipboard item size must be 1..=16 MiB");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipOffer {
    pub seq: u32,
    /// 1 = client，2 = host。
    pub owner: u8,
    pub items: Vec<ClipItem>,
}

impl ClipOffer {
    pub fn total_bytes(&self) -> u64 {
        self.items.iter().map(|item| u64::from(item.size)).sum()
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.items.is_empty() {
            return Err("clipboard offer has no items");
        }
        if self.total_bytes() > MAX_CLIP_TOTAL_BYTES {
            return Err("clipboard aggregate exceeds 64 MiB");
        }
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipAccept {
    pub seq: u32,
    pub formats: Vec<u16>,
    pub credit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipDone {
    pub seq: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStart {
    pub codec: u8,
    pub rate: u32,
    pub ch: u8,
    pub frame_ms: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStop {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostStats {
    pub max_fps: u16,
    pub actual_fps: u16,
    pub accepted_max_kbps: u32,
    #[serde(default)]
    pub capture_ms: u16,
    #[serde(default)]
    pub encode_ms: u16,
    #[serde(default)]
    pub drop_frames: u32,
    pub codec: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ping {
    pub seq: u32,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pong {
    pub seq: u32,
    pub ts_ms: u64,
    #[serde(default)]
    pub rtt_ms: u32,
}

/// 跨端 8 平面发送优先级协商。
///
/// 发起方发 `ack=false`，接收方应用后回同一 `req_id`/`priorities` 的
/// `ack=true`；双方都以最终数组设置本地 QUIC 流权重。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanePriorities {
    pub req_id: u32,
    pub priorities: [i32; 8],
    #[serde(default)]
    pub ack: bool,
}

/// 默认平面权重：control/input/video/file/cursor/audio/feedback/clipboard。
pub const DEFAULT_PLANE_PRIORITIES: [i32; 8] = [100, 90, 50, 10, 60, 40, 30, 20];

/// 依据各平面 backlog 调整权重；只改调度，不改线格式。
#[derive(Debug, Clone)]
pub struct AdaptivePriorities {
    base: [i32; 8],
    ema: [f64; 8],
}

impl AdaptivePriorities {
    pub fn new(base: [i32; 8]) -> Self {
        Self {
            base,
            ema: [0.0; 8],
        }
    }

    pub fn observe(&mut self, backlog_bytes: [u64; 8]) -> [i32; 8] {
        for (i, bytes) in backlog_bytes.into_iter().enumerate() {
            let mb = bytes as f64 / (1024.0 * 1024.0);
            self.ema[i] = self.ema[i] * 0.7 + mb * 0.3;
        }
        let video = self.ema[2];
        let bulk = self.ema[3] + self.ema[5] + self.ema[7];
        let mut out = self.base;
        out[1] += (video * 6.0).min(30.0) as i32;
        out[2] += (video * 10.0).min(40.0) as i32;
        out[3] = (out[3] - (bulk * 5.0).min(10.0) as i32).max(1);
        out[4] += (video * 2.0).min(10.0) as i32;
        out[5] = (out[5] - (bulk * 8.0).min(20.0) as i32).max(1);
        out[7] = (out[7] - (bulk * 4.0).min(10.0) as i32).max(1);
        out
    }
}

/// 恢复票；TTL 300 s，单次使用，恢复成功即换新票（V3 §9.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeTicket {
    pub id: u64,
    pub nonce: [u8; 16],
    pub exp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeError {
    TicketMismatch,
    Expired,
}

impl ResumeTicket {
    pub const TTL_MS: u64 = 300_000;

    /// 先比对 id/nonce，再判断过期，避免把“票是否存在”当成时序旁路。
    pub fn validate(&self, resume: &Resume, now_ms: u64) -> Result<(), ResumeError> {
        if self.id != resume.ticket_id || self.nonce != resume.nonce {
            return Err(ResumeError::TicketMismatch);
        }
        if now_ms >= self.exp_ms {
            return Err(ResumeError::Expired);
        }
        Ok(())
    }
}

/// TLS 后首消息二选一中的 `Resume`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    pub ticket_id: u64,
    pub nonce: [u8; 16],
    #[serde(default)]
    pub last_frame_id: u32,
    #[serde(default)]
    pub last_input_seq: u32,
}

/// `Resume` 的答复；失败时客户端回完整 `Hello`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeResult {
    pub ok: bool,
    #[serde(default)]
    pub code: u16,
    #[serde(default)]
    pub need: u64,
    #[serde(default)]
    pub ticket: Option<ResumeTicket>,
    #[serde(default)]
    pub session_id: u64,
}

/// client 发起流的通道入学首消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelHello {
    pub channel: u8,
    pub session_id: u64,
    #[serde(default)]
    pub token: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelWelcome {
    pub channel: u8,
}

/// 通道入学或策略拒绝；`need` 用于 `NeedsUser` 类错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rejected {
    pub code: u16,
    #[serde(default)]
    pub need: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Screenshot {
    pub display: u8,
    /// 0 = PNG，1 = JPEG；保留其它值给后续格式。
    #[serde(default)]
    pub format: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub path: String,
}

/// Secure Attention Sequence（Ctrl+Alt+Del）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendCad {
    pub display: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMkdir {
    pub req_id: u32,
    pub dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDelete {
    pub req_id: u32,
    pub dir: String,
    pub name: String,
    #[serde(default)]
    pub recycle: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRename {
    pub req_id: u32,
    pub dir: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: u64,
    /// 0 = 独占，1 = 共享，2 = 抢占。
    #[serde(default)]
    pub share: u8,
    #[serde(default)]
    pub control_owner: Option<u64>,
    #[serde(default)]
    pub client_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEnded {
    pub code: u16,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlOwner {
    pub owner: u64,
    #[serde(default)]
    pub since_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareRequest {
    pub req_id: u32,
    pub share: u8,
}

impl ErrorReply {
    pub fn class(&self) -> Option<ErrorClass> {
        match self.class {
            0 => Some(ErrorClass::RetryableTransport),
            1 => Some(ErrorClass::Retryable),
            2 => Some(ErrorClass::NeedsUser),
            3 => Some(ErrorClass::Fatal),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_reply_is_flat_and_keeps_published_code() {
        let info = ErrorInfo::new(crate::error::ErrorCode::EncoderInitFailed);
        let reply = ErrorReply::from_info(9, &info);
        let json = serde_json::to_string(&reply).unwrap();
        assert!(json.contains("\"code\":53"));
        assert!(json.contains("\"req_id\":9"));
        let back: ErrorReply = serde_json::from_str(&json).unwrap();
        assert_eq!(back, reply);
        assert_eq!(back.class(), Some(ErrorClass::Retryable));
    }

    #[test]
    fn file_offer_defaults_match_v3() {
        let json = r#"{"task_id":1,"dir":"D:/x","name":"a.bin","size":10}"#;
        let offer: FileOffer = serde_json::from_str(json).unwrap();
        assert_eq!(offer.block_size, 1 << 20);
        assert_eq!(offer.conflict, 1);
        assert!(!offer.compress);
        assert!(!offer.resume);
    }

    #[test]
    fn display_key_defaults_to_zero_and_is_stable() {
        let without_key: DisplayInfo = serde_json::from_str(
            r#"{"id":0,"x":0,"y":0,"w":1920,"h":1080,"scale":100,"primary":true,"name":"main"}"#,
        )
        .unwrap();
        assert_eq!(without_key.display_key, 0);

        let mut moved = without_key;
        moved.id = 3;
        moved.x = 100;
        moved.w = 1280;
        assert_eq!(moved.display_key, 0);
    }

    #[test]
    fn display_roundtrip() {
        let list = DisplayList {
            displays: vec![DisplayInfo {
                id: 0,
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
                scale: 100,
                primary: true,
                name: "main".into(),
                display_key: 7,
            }],
        };
        let json = serde_json::to_string(&list).unwrap();
        assert_eq!(serde_json::from_str::<DisplayList>(&json).unwrap(), list);
    }

    #[test]
    fn resume_ticket_validation_rejects_mismatch_and_expiry() {
        let ticket = ResumeTicket {
            id: 7,
            nonce: [9; 16],
            exp_ms: 1_000,
        };
        let good = Resume {
            ticket_id: 7,
            nonce: [9; 16],
            last_frame_id: 10,
            last_input_seq: 3,
        };
        assert_eq!(ticket.validate(&good, 999), Ok(()));

        let wrong_nonce = Resume {
            nonce: [8; 16],
            ..good
        };
        assert_eq!(
            ticket.validate(&wrong_nonce, 999),
            Err(ResumeError::TicketMismatch)
        );

        let mut wrong_id = good;
        wrong_id.ticket_id = 8;
        assert_eq!(
            ticket.validate(&wrong_id, 999),
            Err(ResumeError::TicketMismatch)
        );
        assert_eq!(ticket.validate(&good, 1_000), Err(ResumeError::Expired));
    }

    #[test]
    fn adaptive_priorities_raise_video_and_lower_bulk_under_backlog() {
        let mut p = AdaptivePriorities::new(DEFAULT_PLANE_PRIORITIES);
        assert_eq!(p.observe([0; 8]), DEFAULT_PLANE_PRIORITIES);
        let video = p.observe([0, 0, 4 << 20, 8 << 20, 0, 2 << 20, 0, 2 << 20]);
        assert!(video[2] > DEFAULT_PLANE_PRIORITIES[2]);
        assert!(video[1] > DEFAULT_PLANE_PRIORITIES[1]);
        assert!(video[3] < DEFAULT_PLANE_PRIORITIES[3]);
        assert!(video[5] < DEFAULT_PLANE_PRIORITIES[5]);
        assert!(video[7] < DEFAULT_PLANE_PRIORITIES[7]);
    }

    #[test]
    fn file_list_limit_is_bounded() {
        let good = FileList {
            task_id: 1,
            dir: "D:/share".into(),
            offset: 0,
            limit: DEFAULT_FILE_LIST_LIMIT,
            show_hidden: false,
        };
        assert_eq!(good.validate(), Ok(()));
        assert_eq!(
            FileList {
                limit: 0,
                ..good.clone()
            }
            .validate(),
            Err("FileList.limit must be 1..=2000")
        );
        assert_eq!(
            FileList {
                limit: MAX_FILE_LIST_LIMIT + 1,
                ..good
            }
            .validate(),
            Err("FileList.limit must be 1..=2000")
        );
    }

    #[test]
    fn clipboard_format_domain_is_explicit() {
        assert_eq!(ClipFormat::from_u16(clip_format::TEXT), ClipFormat::Text);
        assert_eq!(ClipFormat::from_u16(clip_format::DIB), ClipFormat::Dib);
        assert!(!ClipFormat::from_u16(99).is_known());
        assert_eq!(ClipFormat::from_u16(99).as_u16(), 99);
    }

    #[test]
    fn clipboard_offer_enforces_aggregate_budget() {
        let item = |format| ClipItem {
            format,
            size: 16 << 20,
            sha256_prefix: Vec::new(),
        };
        let at_limit = ClipOffer {
            seq: 1,
            owner: 1,
            items: vec![
                item(clip_format::TEXT),
                item(clip_format::RTF),
                item(clip_format::HTML),
                item(clip_format::PNG),
            ],
        };
        assert_eq!(at_limit.total_bytes(), MAX_CLIP_TOTAL_BYTES);
        assert_eq!(at_limit.validate(), Ok(()));

        let over = ClipOffer {
            items: vec![
                item(clip_format::TEXT),
                item(clip_format::RTF),
                item(clip_format::HTML),
                item(clip_format::PNG),
                item(clip_format::DIB),
            ],
            ..at_limit.clone()
        };
        assert_eq!(over.validate(), Err("clipboard aggregate exceeds 64 MiB"));

        let bad_format = ClipOffer {
            items: vec![item(99)],
            ..at_limit
        };
        assert_eq!(bad_format.validate(), Err("unknown clipboard format"));
    }

    #[test]
    fn input_status_and_host_stats_roundtrip() {
        let s = InputStatus {
            seq: 7,
            code: 1,
            injected: 15,
            failed: 1,
        };
        assert_eq!(
            serde_json::from_str::<InputStatus>(&serde_json::to_string(&s).unwrap()).unwrap(),
            s
        );
        let h = HostStats {
            max_fps: 60,
            actual_fps: 58,
            accepted_max_kbps: 8_000,
            capture_ms: 2,
            encode_ms: 5,
            drop_frames: 0,
            codec: 4,
        };
        assert_eq!(
            serde_json::from_str::<HostStats>(&serde_json::to_string(&h).unwrap()).unwrap(),
            h
        );
    }
}
