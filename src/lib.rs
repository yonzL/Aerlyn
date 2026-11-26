//! aerlyn_wire：aerlyn V4 协议地基。service/client 只依赖本 crate，不依赖 UI/OS。
//!
//! 结构参考：
//! - RustDesk `libs/base/protos/message.proto` 的 `SupportedEncoding/Decoding`；
//! - Airhand `airhand-proto/src/session.rs` 的 `QualitySettings`；
//! - Airhand `airhand-proto/src/media.rs` 的 `VideoHeader`（保留未知 codec）。
//!
//! 本 crate 只做纯逻辑（无 IO、无编码器依赖）；真实 QUIC/TCP 由 `opsdesk-net` 实现。

/// Wire version carried in the handshake (1.2.x line).
pub const PROTO_VERSION: u32 = 400;
/// Primary ALPN.
pub const ALPN: &str = "aerlyn/4";
/// TCP downgrade ALPN.
pub const ALPN_TCP: &str = "aerlyn/4-tcp";

pub mod audio;
pub mod capabilities;
pub mod caps;
pub mod channel;
pub mod codec;
pub mod control;
pub mod data;
pub mod error;
pub mod ext;
pub mod fragment;
pub mod handshake;
pub mod machine;
pub mod media;
pub mod memory;
pub mod message;
pub mod payload;
pub mod plane;
pub mod plane_message;
pub mod privacy;
pub mod quality;
pub mod session;
pub mod settings;
pub mod tickets;
pub mod timeouts;
pub mod timing;
pub mod transfer;
pub mod transport;
pub mod video_datagram;

pub use audio::{
    AudioPacket, AudioRed, JitterBuffer, JitterStats, AUDIO_FRAME_MS, AUDIO_RATE,
    DEFAULT_TARGET_MS, MAX_RED_PACKETS, MAX_RED_PAYLOAD, MAX_TARGET_MS, MIN_TARGET_MS,
};
pub use capabilities::{CodecAbility, PreferCodec, SupportedDecoding, SupportedEncoding};
pub use caps::{
    bit_for, from_bits as video_caps_from_bits, FeatureCaps, VideoCaps, FEATURE_BANDWIDTH,
    FEATURE_DISPLAY_MODES, FEATURE_FILE_BROWSER, FEATURE_FILE_CLIPBOARD, FEATURE_FPS15,
    FEATURE_PREVIEW, FEATURE_PRIVACY, FEATURE_RESOLUTION_PREF, FEATURE_SEMANTIC_CURSOR,
    FEATURE_TEXT_INPUT, VIDEO_AV1, VIDEO_H264, VIDEO_H265, VIDEO_RAW, VIDEO_VP8, VIDEO_VP9,
};
pub use channel::{
    Frame, CH_AUDIO, CH_CLIPBOARD, CH_CONTROL, CH_CURSOR, CH_FEEDBACK, CH_FILE, CH_INPUT, CH_VIDEO,
    MAX_CONTROL_LEN, MAX_FRAME_LEN,
};
pub use codec::{
    bit_depth, cap_flags, chroma, select_codec, Codec, CodecCap, SelectError, SelectedCodec,
    VideoConstraints,
};
pub use control::*;
pub use data::{
    AudioHeader, ClipAck, ClipDataHeader, CursorPos, CursorState, FileAck, FileChunkHeader,
    InputBatch, InputError, InputEvent, MAX_INPUT_EVENTS,
};
pub use error::{ErrorClass, ErrorCode, ErrorInfo};
pub use ext::{ExtError, Extension, Extensions, MAX_EXTENSIONS, MAX_EXT_BYTES};
pub use fragment::{
    fragment_video, FragmentError, ReassembledFrame, SliceRecovery, VideoReassembler,
    FRAGMENT_BYTES, MAX_FRAGMENTED_FRAME_BYTES, MAX_FRAGMENTS, SLICE_RECOVERY_MS,
};
pub use handshake::{
    client_handshake, client_resume, server_handshake, server_resume, server_resume_with_request,
    server_resume_with_validator, transcript, ClientConfig, HandshakeError, ServerConfig, Session,
    Signer, Verifier,
};
pub use machine::{FileTask, MachineError, ResumeSnapshot, SessionMachine, Side, VideoSession};
pub use media::{
    frame_flags, validate_extradata, ConfigError, FrameError, FrameHeader, MediaError, MediaHeader,
    Rect, FRAME_HEADER_LEN, MAX_RECTS, MEDIA_HEADER_LEN,
};
pub use memory::{pair, MemoryConnection};
pub use message::{
    AdjustReason, Auth, Bye, Challenge, CodecConfig, ControlMessage, DecodeError, FrameAck, Hello,
    MessageError, VideoAdjusted, VideoConfig, VideoReady, Welcome,
};
pub use payload::{
    AudioFrame, ClipData, CursorShape, FileChunk, FileChunkZstdPayload, FileErrorPayload,
    PayloadError, MAX_AUDIO_FRAME, MAX_CLIP_CHUNK, MAX_CURSOR_BYTES, MAX_CURSOR_DIM,
    MAX_FILE_CHUNK, MAX_FILE_ERROR_DETAIL,
};
pub use plane::*;
pub use plane_message::{PlaneMessage, PlaneMessageError};
pub use privacy::PrivacyTracker;
pub use quality::QualityMode;
pub use session::{Role, SessionState};
pub use settings::{QualitySettings, VideoPriority};
pub use tickets::{TicketError, TicketStore};
pub use timeouts::{
    Deadline, TimeoutPhase, TimeoutPolicy, AUTH_WELCOME_TIMEOUT_MS, CHALLENGE_AUTH_TIMEOUT_MS,
    CHANNEL_HELLO_TIMEOUT_MS, CONNECT_TIMEOUT_MS, CONTROL_REQUEST_TIMEOUT_MS,
    HELLO_CHALLENGE_TIMEOUT_MS, TLS_TIMEOUT_MS, TOKEN_TTL_MS,
};
pub use timing::{ClockBaseline, RttEstimator};
pub use transfer::{
    validate_file_accept, validate_file_offer, ClipRecvBuffer, ClipRecvWindow, ClipSendCursor,
    ClipSendWindow, ClipWindowConfig, FileBitmap, FileRecvBuffer, FileRecvWindow, FileSendCursor,
    FileSendWindow, FileSparseRecvWindow, FileSparseSendWindow, FileWindowConfig, TransferError,
    DEFAULT_CLIP_BLOCK, DEFAULT_CLIP_CREDIT, DEFAULT_FILE_BLOCK, DEFAULT_FILE_CREDIT,
    MAX_CLIP_BYTES, MAX_FILE_BITMAP_BYTES, MAX_FILE_BLOCK, MAX_FILE_BLOCKS, MAX_FILE_BYTES,
    MIN_FILE_BLOCK,
};
pub use transport::MessageIo;
pub use video_datagram::{
    fragment_video as fragment_video_datagram, fragment_video_fec as fragment_video_fec_datagram,
    VideoDatagram, VideoDatagramError, VideoDatagramFrame, VideoDatagramHeader,
    VideoDatagramReassembler, MAX_VIDEO_FRAGMENTS, VIDEO_DATAGRAM_HEADER, VIDEO_DATAGRAM_KEY,
    VIDEO_DATAGRAM_MTU, VIDEO_DATAGRAM_PAYLOAD,
};
