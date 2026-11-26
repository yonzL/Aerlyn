//! 认证后的协议状态机：校验方向、维护视频/文件/剪贴板/音频状态。
//!
//! 握手仍由 [`crate::handshake`] 负责；这里从 `Welcome` 之后开始，接收
//! [`ControlMessage`]，返回需要立即回发的控制消息（目前是 Ping → Pong）。
//! 未知 type 按 V3 §8 忽略，不改变状态。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::control::{FileOffer, HostStats, PlanePriorities, Pong, PreviewSubscription, Resume};
use crate::message::ControlMessage;
use crate::privacy::PrivacyTracker;
use crate::session::{Role, SessionState};
use crate::timing::RttEstimator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Client,
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSession {
    pub display: u8,
    pub codec: u8,
    pub max_fps: u16,
    pub actual_fps: u16,
    pub max_kbps: u32,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTask {
    pub task_id: u32,
    pub size: u64,
    pub received_upto: u64,
    pub done: bool,
}

/// 恢复会话需要续接的最小状态。视频在途窗口由媒体层按 `frame_id` 重建；
/// 文件只记录已落盘偏移，未确认的发送块由对端 `FileAccept.resume_offset` 重新协商。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeSnapshot {
    pub last_frame_id: u32,
    pub last_input_seq: Option<u32>,
    pub files: BTreeMap<u32, FileTask>,
    pub clip_seq: Option<u32>,
    pub audio: bool,
    pub video: Option<VideoSession>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineError {
    WrongDirection {
        message: String,
        side: Side,
    },
    WrongState {
        message: String,
        state: SessionState,
    },
    DuplicateTask(u32),
    UnknownTask(u32),
    ClipSequenceMismatch {
        expected: Option<u32>,
        got: u32,
    },
    Protocol(&'static str),
    ResumeAhead {
        field: &'static str,
        requested: u32,
        current: u32,
    },
}

impl std::fmt::Display for MachineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for MachineError {}

/// 单条连接的协议状态。`Ready` 表示已认证、尚无视频流。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMachine {
    pub side: Side,
    pub role: Role,
    pub state: SessionState,
    pub video: Option<VideoSession>,
    pub files: BTreeMap<u32, FileTask>,
    pub audio: bool,
    pub clip_seq: Option<u32>,
    pub last_frame_id: u32,
    pub last_input_seq: Option<u32>,
    pub last_host_stats: Option<HostStats>,
    pub last_ping: Option<u32>,
    /// RTT measured by this side's control Ping/Pong exchanges.
    pub rtt: RttEstimator,
    pub last_session_info: Option<crate::control::SessionInfo>,
    pub control_owner: Option<u64>,
    /// Host-authoritative privacy state, monotonically applied on the client.
    pub privacy: PrivacyTracker,
    /// Latest preview request per viewer, for the host application to accept/reject.
    pub preview_subscriptions: BTreeMap<u64, PreviewSubscription>,
    /// 主机侧要求 ChannelHello 携带的会话 token；`None` 表示不校验。
    pub channel_token: Option<Vec<u8>>,
    /// Wall-clock issue time for `channel_token`.
    pub channel_token_issued_ms: u64,
    /// Channels already admitted with this token; repeats are idempotent.
    pub channel_token_used: BTreeSet<u8>,
    /// Caller-supplied wall clock used for token TTL checks.
    pub now_ms: u64,
    /// Unknown typed controls retained for the embedding application.
    unknown_messages: VecDeque<ControlMessage>,
}

/// Upper bound for retained unknown controls; oldest entries are discarded.
pub const MAX_UNKNOWN_MESSAGES: usize = 64;

impl SessionMachine {
    pub fn new(side: Side, role: Role) -> Self {
        Self {
            side,
            role,
            state: SessionState::Ready,
            video: None,
            files: BTreeMap::new(),
            audio: false,
            clip_seq: None,
            last_frame_id: 0,
            last_input_seq: None,
            last_host_stats: None,
            last_ping: None,
            rtt: RttEstimator::new(),
            last_session_info: None,
            control_owner: None,
            privacy: PrivacyTracker::new(),
            preview_subscriptions: BTreeMap::new(),
            channel_token: None,
            channel_token_issued_ms: 0,
            channel_token_used: BTreeSet::new(),
            now_ms: 0,
            unknown_messages: VecDeque::new(),
        }
    }

    /// Unknown typed controls are forwarded instead of silently dropped.
    pub fn take_unknown(&mut self) -> Vec<ControlMessage> {
        self.unknown_messages.drain(..).collect()
    }

    pub fn unknown_count(&self) -> usize {
        self.unknown_messages.len()
    }

    /// 主机在握手完成后绑定会话 token；之后 ChannelHello 必须携带相同 token。
    pub fn set_channel_token(&mut self, token: Vec<u8>) {
        self.set_channel_token_at(token, crate::timeouts::unix_ms());
    }

    /// Bind a token with its original issue time (used by handshake/resume).
    pub fn set_channel_token_at(&mut self, token: Vec<u8>, issued_ms: u64) {
        self.channel_token = Some(token);
        self.channel_token_issued_ms = issued_ms;
        self.channel_token_used.clear();
    }

    /// Update the wall clock used for token TTL checks.
    pub fn set_now_ms(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    pub fn channel_token_valid(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.channel_token_issued_ms) <= crate::timeouts::TOKEN_TTL_MS
    }

    /// Record a locally measured control RTT sample.
    pub fn sample_rtt(&mut self, rtt_ms: u32) {
        self.rtt.sample(rtt_ms);
    }

    pub fn base_rtt_ms(&self) -> Option<u32> {
        self.rtt.base_ms()
    }

    pub fn avg_rtt_ms(&self) -> Option<u32> {
        self.rtt.avg_ms()
    }

    /// 客户端发出 `VideoConfig` 后记录本地期望；服务端用 `handle` 接收同一条消息。
    pub fn begin_local_video(&mut self, config: &crate::message::VideoConfig) {
        self.video = Some(VideoSession {
            display: config.display,
            codec: 0,
            max_fps: config.max_fps,
            actual_fps: 0,
            max_kbps: config.max_kbps,
            active: config.active,
        });
        self.state = SessionState::Streaming;
    }

    /// 媒体层每发/收一帧调用一次；恢复时用它判断客户端是否在伪造未来帧号。
    pub fn note_frame(&mut self, frame_id: u32) {
        self.last_frame_id = frame_id;
    }

    /// 输入层每发/应用一批输入调用一次；服务端已由 `InputStatus` 自动更新。
    pub fn note_input(&mut self, seq: u32) {
        self.last_input_seq = Some(seq);
    }

    pub fn snapshot(&self) -> ResumeSnapshot {
        ResumeSnapshot {
            last_frame_id: self.last_frame_id,
            last_input_seq: self.last_input_seq,
            files: self.files.clone(),
            clip_seq: self.clip_seq,
            audio: self.audio,
            video: self.video.clone(),
        }
    }

    /// 在 TLS 恢复协商成功后调用：只接受不超前于本地状态的 `Resume`，
    /// 返回需要由媒体/文件层续接的快照。
    pub fn accept_resume(&mut self, resume: &Resume) -> Result<ResumeSnapshot, MachineError> {
        if self.state == SessionState::Closed {
            return Err(MachineError::WrongState {
                message: "Resume".to_string(),
                state: self.state,
            });
        }
        if resume.last_frame_id > self.last_frame_id {
            return Err(MachineError::ResumeAhead {
                field: "frame_id",
                requested: resume.last_frame_id,
                current: self.last_frame_id,
            });
        }
        let current_input = self.last_input_seq.unwrap_or(0);
        if resume.last_input_seq > current_input {
            return Err(MachineError::ResumeAhead {
                field: "input_seq",
                requested: resume.last_input_seq,
                current: current_input,
            });
        }
        self.state = if self.video.is_some() {
            SessionState::Streaming
        } else {
            SessionState::Ready
        };
        Ok(self.snapshot())
    }

    /// 从快照恢复；调用方负责重新建立 TLS 连接与媒体解码器。
    pub fn restore_resume(&mut self, snapshot: ResumeSnapshot) {
        self.last_frame_id = snapshot.last_frame_id;
        self.last_input_seq = snapshot.last_input_seq;
        self.files = snapshot.files;
        self.clip_seq = snapshot.clip_seq;
        self.audio = snapshot.audio;
        self.video = snapshot.video;
        self.state = if self.video.is_some() {
            SessionState::Streaming
        } else {
            SessionState::Ready
        };
    }

    /// 应用一条 control 消息，返回需要立刻回发的消息。
    pub fn handle(&mut self, msg: &ControlMessage) -> Result<Vec<ControlMessage>, MachineError> {
        let name = msg.type_name();
        if let ControlMessage::Unknown { .. } = msg {
            if self.unknown_messages.len() == MAX_UNKNOWN_MESSAGES {
                self.unknown_messages.pop_front();
            }
            self.unknown_messages.push_back(msg.clone());
            return Ok(Vec::new());
        }
        if !direction_allowed(name, self.side) {
            return Err(MachineError::WrongDirection {
                message: name.to_string(),
                side: self.side,
            });
        }
        if !self.state.can_accept_control() {
            return Err(MachineError::WrongState {
                message: name.to_string(),
                state: self.state,
            });
        }

        let mut replies = Vec::new();
        match msg {
            ControlMessage::VideoConfig(v) => {
                self.require_stream_capable(name)?;
                if !self.role.can_read() {
                    return Err(MachineError::Protocol("role cannot view"));
                }
                self.video = Some(VideoSession {
                    display: v.display,
                    codec: 0,
                    max_fps: v.max_fps,
                    actual_fps: 0,
                    max_kbps: v.max_kbps,
                    active: v.active,
                });
                self.state = SessionState::Streaming;
            }
            ControlMessage::VideoReady(v) => {
                self.require_streaming(name)?;
                let video = self
                    .video
                    .as_mut()
                    .ok_or(MachineError::Protocol("VideoReady before VideoConfig"))?;
                video.display = v.display;
                video.codec = v.codec;
                video.max_fps = v.max_fps;
                video.max_kbps = v.max_kbps;
            }
            ControlMessage::VideoAdjusted(v) => {
                self.require_streaming(name)?;
                if let Some(video) = self.video.as_mut() {
                    video.max_fps = v.max_fps;
                    video.actual_fps = v.actual_fps;
                }
            }
            ControlMessage::VideoStop(v) => {
                self.require_streaming(name)?;
                if let Some(video) = self.video.as_ref() {
                    if video.display != v.display {
                        return Err(MachineError::Protocol("VideoStop for other display"));
                    }
                }
                self.video = None;
                self.state = SessionState::Ready;
            }
            ControlMessage::RequestKeyframe(v) => {
                self.require_streaming(name)?;
                if !self.video.as_ref().is_some_and(|x| x.display == v.display) {
                    return Err(MachineError::Protocol(
                        "RequestKeyframe for inactive display",
                    ));
                }
            }
            ControlMessage::DisplayList(_) | ControlMessage::DisplayChanged(_) => {
                self.require_streaming(name)?;
            }
            ControlMessage::InputConfig(_) => {
                self.require_stream_capable(name)?;
            }
            ControlMessage::InputState(_) => {}
            ControlMessage::InputStatus(s) => {
                self.last_input_seq = Some(s.seq);
            }
            ControlMessage::FileOffer(o) => {
                self.insert_file(o)?;
            }
            ControlMessage::FileGet(g) => {
                self.require_stream_capable(name)?;
                self.insert_task(g.task_id, 0)?;
            }
            ControlMessage::FileList(l) => {
                self.require_stream_capable(name)?;
                l.validate().map_err(MachineError::Protocol)?;
                self.insert_task(l.task_id, 0)?;
            }
            ControlMessage::FileCancel(c) => {
                self.require_stream_capable(name)?;
                if self.files.remove(&c.task_id).is_none() {
                    return Err(MachineError::UnknownTask(c.task_id));
                }
                replies.push(ControlMessage::Ok(crate::control::OkReply {
                    req_id: c.task_id,
                }));
            }
            ControlMessage::FileAccept(a) => {
                self.file_mut(a.task_id)?.resume(a.resume_offset);
            }
            ControlMessage::FileMeta(m) => {
                let task = self.file_mut(m.task_id)?;
                task.size = m.size;
            }
            ControlMessage::FileListing(l) => {
                self.file_mut(l.task_id)?;
            }
            ControlMessage::FileDone(d) => {
                let task = self.file_mut(d.task_id)?;
                task.done = d.ok;
                task.size = d.size;
            }
            ControlMessage::FileError(e) => {
                self.file_mut(e.task_id)?;
            }
            ControlMessage::ClipPull(_) => {}
            ControlMessage::ClipOffer(o) => {
                o.validate().map_err(MachineError::Protocol)?;
                if let Some(expected) = self.clip_seq {
                    if o.seq < expected {
                        return Err(MachineError::ClipSequenceMismatch {
                            expected: self.clip_seq,
                            got: o.seq,
                        });
                    }
                }
                self.clip_seq = Some(o.seq);
            }
            ControlMessage::ClipAccept(a) => {
                if self.clip_seq != Some(a.seq) {
                    return Err(MachineError::ClipSequenceMismatch {
                        expected: self.clip_seq,
                        got: a.seq,
                    });
                }
            }
            ControlMessage::ClipDone(d) => {
                if self.clip_seq != Some(d.seq) {
                    return Err(MachineError::ClipSequenceMismatch {
                        expected: self.clip_seq,
                        got: d.seq,
                    });
                }
                self.clip_seq = None;
            }
            ControlMessage::AudioStart(_) => self.audio = true,
            ControlMessage::AudioStop(_) => self.audio = false,
            ControlMessage::Screenshot(_) | ControlMessage::SendCad(_) => {
                self.require_stream_capable(name)?;
                if !self.role.can_read() {
                    return Err(MachineError::Protocol("role cannot capture"));
                }
            }
            ControlMessage::Saved(_) => {
                self.require_stream_capable(name)?;
            }
            ControlMessage::FileMkdir(_)
            | ControlMessage::FileDelete(_)
            | ControlMessage::FileRename(_) => {
                self.require_stream_capable(name)?;
                if !self.role.can_write() {
                    return Err(MachineError::Protocol("role cannot mutate files"));
                }
            }
            ControlMessage::SessionInfo(info) => {
                self.last_session_info = Some(info.clone());
                self.control_owner = info.control_owner;
            }
            ControlMessage::SessionEnded(_) => {
                self.state = SessionState::Closed;
                self.video = None;
                self.audio = false;
                self.files.clear();
                self.clip_seq = None;
            }
            ControlMessage::ControlOwner(owner) => {
                self.control_owner = Some(owner.owner);
            }
            ControlMessage::PrivacyState(state) => {
                self.privacy.apply(state.clone());
            }
            ControlMessage::PreviewSubscription(sub) => {
                self.require_stream_capable(name)?;
                sub.validate().map_err(MachineError::Protocol)?;
                if self.preview_subscriptions.len() >= 64
                    && !self.preview_subscriptions.contains_key(&sub.viewer_id)
                {
                    return Err(MachineError::Protocol("too many preview subscriptions"));
                }
                self.preview_subscriptions.insert(sub.viewer_id, *sub);
            }
            ControlMessage::ShareRequest(_) => {
                self.require_stream_capable(name)?;
                if !self.role.can_write() {
                    return Err(MachineError::Protocol("role cannot change share mode"));
                }
            }
            ControlMessage::HostStats(s) => {
                self.last_host_stats = Some(*s);
                if let Some(video) = self.video.as_mut() {
                    video.actual_fps = s.actual_fps;
                }
            }
            ControlMessage::ChannelHello(h) => {
                let token_ok = self
                    .channel_token
                    .as_ref()
                    .is_none_or(|expected| expected == &h.token);
                let token_live = self.channel_token_valid(self.now_ms);
                if !token_ok || !token_live {
                    replies.push(ControlMessage::Rejected(crate::control::Rejected {
                        code: crate::error::ErrorCode::TokenInvalid.code(),
                        need: 0,
                    }));
                } else if (1..=7).contains(&h.channel) {
                    // Re-sending ChannelHello on the same channel is idempotent:
                    // the prior ChannelWelcome may have been lost.
                    self.channel_token_used.insert(h.channel);
                    replies.push(ControlMessage::ChannelWelcome(
                        crate::control::ChannelWelcome { channel: h.channel },
                    ));
                } else {
                    replies.push(ControlMessage::Rejected(crate::control::Rejected {
                        code: crate::error::ErrorCode::ProtocolError.code(),
                        need: 0,
                    }));
                }
            }
            ControlMessage::ChannelWelcome(_) | ControlMessage::Rejected(_) => {}
            ControlMessage::Resume(_) => {
                return Err(MachineError::Protocol("Resume after Welcome"));
            }
            ControlMessage::ResumeResult(_) => {}
            ControlMessage::Ping(p) => {
                self.last_ping = Some(p.seq);
                replies.push(ControlMessage::Pong(Pong {
                    seq: p.seq,
                    ts_ms: p.ts_ms,
                    // The responder reports its own latest RTT estimate. The
                    // initiator must still measure the round trip locally.
                    rtt_ms: self.rtt.avg_ms().unwrap_or(0),
                }));
            }
            ControlMessage::Pong(p) => {
                self.last_ping = Some(p.seq);
            }
            ControlMessage::PlanePriorities(p) => {
                if !p.ack {
                    replies.push(ControlMessage::PlanePriorities(PlanePriorities {
                        req_id: p.req_id,
                        priorities: p.priorities,
                        ack: true,
                    }));
                }
            }
            ControlMessage::Bye(_) => {
                self.state = SessionState::Closed;
                self.video = None;
                self.audio = false;
                self.files.clear();
                self.clip_seq = None;
            }
            ControlMessage::Error(_) => {
                self.state = SessionState::Closing;
            }
            ControlMessage::Ok(_) => {}
            ControlMessage::Hello(_)
            | ControlMessage::Challenge(_)
            | ControlMessage::Auth(_)
            | ControlMessage::Welcome(_) => {
                return Err(MachineError::Protocol("handshake message after Welcome"));
            }
            ControlMessage::CodecConfig(_) | ControlMessage::DecodeError(_) => {
                self.require_streaming(name)?;
            }
            ControlMessage::Unknown { .. } => return Ok(Vec::new()),
        }
        Ok(replies)
    }

    fn require_stream_capable(&self, message: &str) -> Result<(), MachineError> {
        if self.state.is_open() {
            Ok(())
        } else {
            Err(MachineError::WrongState {
                message: message.to_string(),
                state: self.state,
            })
        }
    }

    fn require_streaming(&self, message: &str) -> Result<(), MachineError> {
        if self.state == SessionState::Streaming {
            Ok(())
        } else {
            Err(MachineError::WrongState {
                message: message.to_string(),
                state: self.state,
            })
        }
    }

    fn insert_file(&mut self, offer: &FileOffer) -> Result<(), MachineError> {
        self.insert_task(offer.task_id, offer.size)
    }

    fn insert_task(&mut self, task_id: u32, size: u64) -> Result<(), MachineError> {
        if self.files.contains_key(&task_id) {
            return Err(MachineError::DuplicateTask(task_id));
        }
        self.files.insert(
            task_id,
            FileTask {
                task_id,
                size,
                received_upto: 0,
                done: false,
            },
        );
        Ok(())
    }

    fn file_mut(&mut self, task_id: u32) -> Result<&mut FileTask, MachineError> {
        self.files
            .get_mut(&task_id)
            .ok_or(MachineError::UnknownTask(task_id))
    }
}

impl FileTask {
    fn resume(&mut self, offset: u64) {
        self.received_upto = offset;
        self.done = false;
    }
}

/// control 消息方向表；与 V3 附录 A 对齐。这里的 `side` 是**接收方**。
pub fn direction_allowed(name: &str, side: Side) -> bool {
    use Side::{Client, Host};
    match name {
        "Hello" | "Auth" | "Resume" | "ChannelHello" => matches!(side, Host),
        "Challenge" | "Welcome" | "ResumeResult" | "ChannelWelcome" | "Rejected" => {
            matches!(side, Client)
        }
        "Bye" | "Error" | "Ok" | "Ping" | "Pong" | "PlanePriorities" => true,
        "VideoConfig" | "VideoStop" | "RequestKeyframe" | "InputConfig" | "DecodeError" => {
            matches!(side, Host)
        }
        "VideoReady" | "CodecConfig" | "VideoAdjusted" | "DisplayList" | "DisplayChanged"
        | "InputState" | "InputStatus" | "HostStats" | "Saved" | "SessionInfo" | "SessionEnded"
        | "ControlOwner" | "PrivacyState" => matches!(side, Client),
        "FileOffer" | "FileGet" | "FileList" | "FileCancel" | "PreviewSubscription" => {
            matches!(side, Host)
        }
        "FileAccept" | "FileMeta" | "FileListing" => matches!(side, Client),
        "FileDone" | "FileError" => true,
        "ClipPull" | "AudioStart" | "AudioStop" => matches!(side, Host),
        "ClipOffer" | "ClipAccept" | "ClipDone" => true,
        "Screenshot" | "SendCad" | "FileMkdir" | "FileDelete" | "FileRename" | "ShareRequest" => {
            matches!(side, Host)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{
        AudioStart, FileCancel, FileDone, FileGet, FileList, FileOffer, InputStatus, Ping,
        PlanePriorities, PreviewSubscription, PrivacyState, Resume, VideoStop,
    };
    use crate::message::{VideoAdjusted, VideoConfig, VideoReady, CONTROL_MESSAGE_TYPES};
    use serde_json::json;

    fn client() -> SessionMachine {
        SessionMachine::new(Side::Client, Role::Control)
    }
    fn host() -> SessionMachine {
        SessionMachine::new(Side::Host, Role::Control)
    }

    fn start_stream(m: &mut SessionMachine) {
        m.handle(&ControlMessage::VideoConfig(VideoConfig {
            max_fps: 60,
            ..VideoConfig::default()
        }))
        .unwrap();
    }

    #[test]
    fn unknown_controls_are_forwarded_and_bounded() {
        let mut m = client();
        for i in 0..(MAX_UNKNOWN_MESSAGES + 2) {
            let msg = ControlMessage::Unknown {
                type_name: "FutureControl".to_string(),
                raw: json!({ "i": i }),
            };
            m.handle(&msg).unwrap();
        }
        assert_eq!(m.unknown_count(), MAX_UNKNOWN_MESSAGES);
        let forwarded = m.take_unknown();
        assert_eq!(forwarded.len(), MAX_UNKNOWN_MESSAGES);
        assert_eq!(forwarded.first().unwrap().type_name(), "FutureControl");
        assert_eq!(m.unknown_count(), 0);
    }

    #[test]
    fn every_registered_control_has_a_receiving_side() {
        for name in CONTROL_MESSAGE_TYPES {
            assert!(
                direction_allowed(name, Side::Host) || direction_allowed(name, Side::Client),
                "control {name} has no receiving side"
            );
        }
    }

    #[test]
    fn video_lifecycle() {
        let mut h = host();
        start_stream(&mut h);
        assert_eq!(h.state, SessionState::Streaming);
        h.handle(&ControlMessage::VideoStop(VideoStop { display: 0 }))
            .unwrap();
        assert!(h.video.is_none());
        assert_eq!(h.state, SessionState::Ready);

        let mut c = client();
        c.state = SessionState::Streaming;
        c.video = Some(VideoSession {
            display: 0,
            codec: 0,
            max_fps: 60,
            actual_fps: 0,
            max_kbps: 8_000,
            active: true,
        });
        c.handle(&ControlMessage::VideoReady(VideoReady {
            req_id: 0,
            display: 0,
            codec: 4,
            profile: 1,
            level: 13,
            chroma: 1,
            bit_depth: 1,
            width: 1920,
            height: 1080,
            max_fps: 60,
            max_kbps: 8_000,
            hw: true,
        }))
        .unwrap();
        assert_eq!(c.video.as_ref().unwrap().codec, 4);
        c.handle(&ControlMessage::VideoAdjusted(VideoAdjusted {
            display: 0,
            max_fps: 45,
            actual_fps: 44,
            reason: crate::message::AdjustReason::Congestion,
            retry_after_ms: 0,
        }))
        .unwrap();
        assert_eq!(c.video.as_ref().unwrap().max_fps, 45);
    }

    #[test]
    fn resume_snapshot_preserves_offsets_and_rejects_ahead() {
        let mut h = host();
        start_stream(&mut h);
        h.note_frame(120);
        h.note_input(7);
        h.files.insert(
            9,
            FileTask {
                task_id: 9,
                size: 8_192,
                received_upto: 4_096,
                done: false,
            },
        );

        let resume = Resume {
            ticket_id: 1,
            nonce: [0; 16],
            last_frame_id: 120,
            last_input_seq: 7,
        };
        let snapshot = h.accept_resume(&resume).unwrap();
        assert_eq!(snapshot.last_frame_id, 120);
        assert_eq!(snapshot.last_input_seq, Some(7));
        assert_eq!(snapshot.files.get(&9).unwrap().received_upto, 4_096);
        assert_eq!(h.state, SessionState::Streaming);

        let ahead = Resume {
            last_frame_id: 121,
            ..resume
        };
        assert_eq!(
            h.accept_resume(&ahead),
            Err(MachineError::ResumeAhead {
                field: "frame_id",
                requested: 121,
                current: 120,
            })
        );

        let mut c = client();
        c.restore_resume(snapshot);
        assert_eq!(c.last_frame_id, 120);
        assert_eq!(c.last_input_seq, Some(7));
        assert_eq!(c.files.get(&9).unwrap().received_upto, 4_096);
    }

    #[test]
    fn host_direction_rejects_client_video_ready() {
        let mut h = host();
        let err = h
            .handle(&ControlMessage::VideoReady(VideoReady {
                req_id: 0,
                display: 0,
                codec: 4,
                profile: 1,
                level: 13,
                chroma: 1,
                bit_depth: 1,
                width: 1920,
                height: 1080,
                max_fps: 60,
                max_kbps: 8_000,
                hw: true,
            }))
            .unwrap_err();
        assert!(matches!(
            err,
            MachineError::WrongDirection {
                side: Side::Host,
                ..
            }
        ));
    }

    #[test]
    fn client_receives_host_stats() {
        let mut c = client();
        c.handle(&ControlMessage::HostStats(HostStats {
            max_fps: 60,
            actual_fps: 60,
            accepted_max_kbps: 8_000,
            capture_ms: 0,
            encode_ms: 0,
            drop_frames: 0,
            codec: 4,
        }))
        .unwrap();
        assert_eq!(c.last_host_stats.unwrap().actual_fps, 60);
    }

    #[test]
    fn privacy_state_applies_monotonically_on_client() {
        let mut c = client();
        let state = |revision| PrivacyState {
            revision,
            state: crate::control::privacy_mode::ON,
            overlay_active: true,
            reason: "remote session".into(),
            local_unlock_revision: 0,
        };
        c.handle(&ControlMessage::PrivacyState(state(1))).unwrap();
        c.handle(&ControlMessage::PrivacyState(state(1))).unwrap();
        assert_eq!(c.privacy.current().unwrap().revision, 1);
        assert!(c.privacy.overlay_visible());
        assert!(c.privacy.local_unlock());
        assert!(!c.privacy.overlay_visible());
        c.handle(&ControlMessage::PrivacyState(state(2))).unwrap();
        assert_eq!(c.privacy.current().unwrap().revision, 2);
        assert!(c.privacy.overlay_visible());
    }

    #[test]
    fn host_keeps_latest_preview_subscription_per_viewer() {
        let mut h = host();
        let sub = |fps| PreviewSubscription {
            req_id: 1,
            viewer_id: 9,
            max_fps: fps,
            max_kbps: 4_000,
            width: 1280,
            height: 720,
            codec: 0,
            quality: 1,
        };
        h.handle(&ControlMessage::PreviewSubscription(sub(30)))
            .unwrap();
        h.handle(&ControlMessage::PreviewSubscription(sub(60)))
            .unwrap();
        assert_eq!(h.preview_subscriptions.len(), 1);
        assert_eq!(h.preview_subscriptions[&9].max_fps, 60);
    }

    #[test]
    fn client_direction_rejects_video_config() {
        let mut c = client();
        let err = c
            .handle(&ControlMessage::VideoConfig(VideoConfig {
                max_fps: 60,
                ..VideoConfig::default()
            }))
            .unwrap_err();
        assert!(matches!(
            err,
            MachineError::WrongDirection {
                side: Side::Client,
                ..
            }
        ));
    }

    #[test]
    fn ping_returns_pong() {
        let mut c = client();
        let replies = c
            .handle(&ControlMessage::Ping(Ping { seq: 7, ts_ms: 100 }))
            .unwrap();
        assert_eq!(replies.len(), 1);
        match &replies[0] {
            ControlMessage::Pong(p) => {
                assert_eq!(p.seq, 7);
                assert_eq!(p.ts_ms, 100);
            }
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn pong_reports_measured_rtt() {
        let mut c = client();
        c.sample_rtt(40);
        c.sample_rtt(80);
        let replies = c
            .handle(&ControlMessage::Ping(Ping { seq: 9, ts_ms: 5 }))
            .unwrap();
        match &replies[0] {
            ControlMessage::Pong(p) => {
                assert_eq!(p.seq, 9);
                assert_eq!(p.rtt_ms, c.avg_rtt_ms().unwrap());
                assert_eq!(c.base_rtt_ms(), Some(40));
            }
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn file_task_lifecycle_and_duplicate() {
        let mut h = host();
        let offer = FileOffer {
            task_id: 5,
            dir: "D:/recv".into(),
            name: "a".into(),
            size: 10,
            sha256: Vec::new(),
            block_size: 1 << 20,
            credit: crate::transfer::DEFAULT_FILE_CREDIT,
            block_sha_root: Vec::new(),
            mtime_ms: 0,
            conflict: 1,
            compress: false,
            resume: false,
            extensions: Vec::new(),
        };
        h.handle(&ControlMessage::FileOffer(offer.clone())).unwrap();
        assert_eq!(h.files.len(), 1);
        assert_eq!(
            h.handle(&ControlMessage::FileOffer(offer)),
            Err(MachineError::DuplicateTask(5))
        );

        let mut c = client();
        c.files.insert(
            5,
            FileTask {
                task_id: 5,
                size: 10,
                received_upto: 0,
                done: false,
            },
        );
        c.handle(&ControlMessage::FileDone(FileDone {
            task_id: 5,
            ok: true,
            size: 10,
            error_code: None,
        }))
        .unwrap();
        assert!(c.files.get(&5).unwrap().done);
    }

    #[test]
    fn file_browse_requests_create_and_cancel_tasks() {
        let mut h = host();
        let get = FileGet {
            task_id: 11,
            dir: "D:/share".into(),
            name: "a.bin".into(),
            offset: 0,
        };
        assert!(h
            .handle(&ControlMessage::FileGet(get.clone()))
            .unwrap()
            .is_empty());
        assert_eq!(h.files.get(&11).unwrap().size, 0);
        assert_eq!(
            h.handle(&ControlMessage::FileGet(get)),
            Err(MachineError::DuplicateTask(11))
        );

        let list = FileList {
            task_id: 12,
            dir: "D:/share".into(),
            offset: 0,
            limit: 500,
            show_hidden: false,
        };
        assert!(h
            .handle(&ControlMessage::FileList(list.clone()))
            .unwrap()
            .is_empty());
        assert_eq!(h.files.len(), 2);
        let bad = FileList { limit: 0, ..list };
        assert!(matches!(
            h.handle(&ControlMessage::FileList(bad)),
            Err(MachineError::Protocol(_))
        ));

        let replies = h
            .handle(&ControlMessage::FileCancel(FileCancel {
                task_id: 11,
                reason: "user".into(),
            }))
            .unwrap();
        assert!(matches!(
            replies.as_slice(),
            [ControlMessage::Ok(o)] if o.req_id == 11
        ));
        assert!(!h.files.contains_key(&11));
        assert_eq!(
            h.handle(&ControlMessage::FileCancel(FileCancel {
                task_id: 99,
                reason: String::new(),
            })),
            Err(MachineError::UnknownTask(99))
        );
    }

    #[test]
    fn audio_and_input_state() {
        let mut h = host();
        h.handle(&ControlMessage::AudioStart(AudioStart {
            codec: 0,
            rate: 48_000,
            ch: 2,
            frame_ms: 10,
        }))
        .unwrap();
        assert!(h.audio);
        h.handle(&ControlMessage::AudioStop(crate::control::AudioStop {}))
            .unwrap();
        assert!(!h.audio);

        let mut c = client();
        c.handle(&ControlMessage::InputStatus(InputStatus {
            seq: 3,
            code: 0,
            injected: 4,
            failed: 0,
        }))
        .unwrap();
        assert_eq!(c.last_input_seq, Some(3));
    }

    #[test]
    fn channel_admission_and_session_events() {
        let mut h = host();
        let replies = h
            .handle(&ControlMessage::ChannelHello(
                crate::control::ChannelHello {
                    channel: 3,
                    session_id: 42,
                    token: vec![1; 32],
                },
            ))
            .unwrap();
        assert_eq!(
            replies,
            vec![ControlMessage::ChannelWelcome(
                crate::control::ChannelWelcome { channel: 3 }
            )]
        );

        let replies = h
            .handle(&ControlMessage::ChannelHello(
                crate::control::ChannelHello {
                    channel: 0,
                    session_id: 42,
                    token: vec![1; 32],
                },
            ))
            .unwrap();
        assert!(matches!(
            replies.as_slice(),
            [ControlMessage::Rejected(r)] if r.code == crate::error::ErrorCode::ProtocolError.code()
        ));

        let mut c = client();
        c.handle(&ControlMessage::SessionInfo(crate::control::SessionInfo {
            session_id: 42,
            share: 1,
            control_owner: Some(7),
            client_name: "client".into(),
        }))
        .unwrap();
        assert_eq!(c.control_owner, Some(7));
        c.handle(&ControlMessage::ControlOwner(
            crate::control::ControlOwner {
                owner: 8,
                since_ms: 1,
            },
        ))
        .unwrap();
        assert_eq!(c.control_owner, Some(8));
        c.handle(&ControlMessage::SessionEnded(
            crate::control::SessionEnded {
                code: 0,
                reason: "done".into(),
            },
        ))
        .unwrap();
        assert_eq!(c.state, SessionState::Closed);
    }

    #[test]
    fn channel_hello_requires_bound_token() {
        let mut h = host();
        h.set_channel_token(vec![3; 32]);
        let wrong = h
            .handle(&ControlMessage::ChannelHello(
                crate::control::ChannelHello {
                    channel: 3,
                    session_id: 7,
                    token: vec![9; 32],
                },
            ))
            .unwrap();
        assert!(matches!(
            wrong.as_slice(),
            [ControlMessage::Rejected(r)] if r.code == 13
        ));
        let right = h
            .handle(&ControlMessage::ChannelHello(
                crate::control::ChannelHello {
                    channel: 3,
                    session_id: 7,
                    token: vec![3; 32],
                },
            ))
            .unwrap();
        assert!(matches!(
            right.as_slice(),
            [ControlMessage::ChannelWelcome(w)] if w.channel == 3
        ));
    }

    #[test]
    fn channel_token_expires_after_60_seconds() {
        let mut h = host();
        h.set_channel_token_at(vec![3; 32], 1_000);
        h.set_now_ms(60_999);
        let ok = h
            .handle(&ControlMessage::ChannelHello(
                crate::control::ChannelHello {
                    channel: 2,
                    session_id: 7,
                    token: vec![3; 32],
                },
            ))
            .unwrap();
        assert!(matches!(
            ok.as_slice(),
            [ControlMessage::ChannelWelcome(w)] if w.channel == 2
        ));
        h.set_now_ms(61_001);
        let expired = h
            .handle(&ControlMessage::ChannelHello(
                crate::control::ChannelHello {
                    channel: 3,
                    session_id: 7,
                    token: vec![3; 32],
                },
            ))
            .unwrap();
        assert!(matches!(
            expired.as_slice(),
            [ControlMessage::Rejected(r)] if r.code == 13
        ));
    }

    #[test]
    fn handshake_messages_rejected_after_welcome() {
        let mut h = host();
        let err = h
            .handle(&ControlMessage::Hello(crate::message::Hello {
                version: 400,
                public_key: vec![],
                caps: 0,
                caps_required: 0,
                codec_caps: vec![],
                caps_ext: vec![],
                actor_host: String::new(),
                extensions: Vec::new(),
            }))
            .unwrap_err();
        assert!(matches!(err, MachineError::Protocol(_)));
    }

    #[test]
    fn plane_priorities_are_acked_once() {
        let mut h = host();
        let request = ControlMessage::PlanePriorities(PlanePriorities {
            req_id: 9,
            priorities: [100, 90, 50, 10, 60, 40, 30, 20],
            ack: false,
        });
        let replies = h.handle(&request).unwrap();
        assert!(matches!(
            replies.as_slice(),
            [ControlMessage::PlanePriorities(p)]
                if p.req_id == 9 && p.ack && p.priorities == [100, 90, 50, 10, 60, 40, 30, 20]
        ));
        let ack = ControlMessage::PlanePriorities(PlanePriorities {
            req_id: 9,
            priorities: [100, 90, 50, 10, 60, 40, 30, 20],
            ack: true,
        });
        assert!(h.handle(&ack).unwrap().is_empty());
    }
}
