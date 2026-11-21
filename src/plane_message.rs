//! Typed decode for the non-control data planes.
//!
//! `PlaneIo` deliberately exposes raw [`Frame`]s so callers can route them
//! without giving up control over buffering. This module is the next layer:
//! it turns a raw frame into the exact message registered in
//! `docs/45` §7.7.1, validating the fixed header and payload at the same time.

use std::fmt;

use crate::channel::{
    Frame, CH_AUDIO, CH_CLIPBOARD, CH_CURSOR, CH_FEEDBACK, CH_FILE, CH_INPUT, CH_VIDEO,
};
use crate::data::{ClipAck, CursorPos, FileAck, InputBatch, InputError};
use crate::media::{MediaError, MediaHeader};
use crate::message::FrameAck;
use crate::payload::{
    AudioFrame, ClipData, CursorShape, FileChunk, FileChunkZstdPayload, FileErrorPayload,
    PayloadError,
};
use crate::plane::{
    type_id, AudioFmt, CursorReset, FileEnd, FileResumeHint, NetReport, SliceNack, StreamEnd,
    StreamHello,
};

/// A decoded non-control plane message.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaneMessage {
    Input(InputBatch),
    StreamHello(StreamHello),
    VideoFrame {
        header: MediaHeader,
        payload: Vec<u8>,
    },
    StreamEnd(StreamEnd),
    FileChunk(FileChunk),
    FileAck(FileAck),
    FileEnd(FileEnd),
    FileError(FileErrorPayload),
    FileChunkZstd(FileChunkZstdPayload),
    FileResumeHint(FileResumeHint),
    CursorShape(CursorShape),
    CursorReset(CursorReset),
    CursorPos(CursorPos),
    AudioFmt(AudioFmt),
    AudioFrame(AudioFrame),
    FrameAck(FrameAck),
    NetReport(NetReport),
    SliceNack(SliceNack),
    ClipData(ClipData),
    ClipAck(ClipAck),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaneMessageError {
    BadChannel(u8),
    Unsupported { channel: u8, type_id: u16 },
    Input(InputError),
    Media(MediaError),
    Payload(PayloadError),
}

impl fmt::Display for PlaneMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadChannel(channel) => write!(f, "not a data-plane channel: {channel}"),
            Self::Unsupported { channel, type_id } => {
                write!(
                    f,
                    "unsupported data-plane type: channel={channel} type={type_id}"
                )
            }
            Self::Input(e) => write!(f, "input: {e}"),
            Self::Media(e) => write!(f, "media: {e:?}"),
            Self::Payload(e) => write!(f, "payload: {e}"),
        }
    }
}

impl std::error::Error for PlaneMessageError {}

impl From<InputError> for PlaneMessageError {
    fn from(value: InputError) -> Self {
        Self::Input(value)
    }
}

impl From<MediaError> for PlaneMessageError {
    fn from(value: MediaError) -> Self {
        Self::Media(value)
    }
}

impl From<PayloadError> for PlaneMessageError {
    fn from(value: PayloadError) -> Self {
        Self::Payload(value)
    }
}

impl PlaneMessage {
    /// Decode exactly one registered data-plane frame.
    ///
    /// `ch3/type=4` (`FileError`) is an 8 B header plus UTF-8 detail; it is
    /// distinct from the JSON control-plane `FileErrorInfo`.
    pub fn decode(frame: &Frame) -> Result<Self, PlaneMessageError> {
        match frame.channel {
            CH_INPUT => {
                if frame.type_id == type_id::input::INPUT_BATCH {
                    Ok(Self::Input(InputBatch::decode(&frame.payload)?))
                } else {
                    Err(unsupported(frame))
                }
            }
            CH_VIDEO => match frame.type_id {
                type_id::video::STREAM_HELLO => {
                    Ok(Self::StreamHello(StreamHello::decode(&frame.payload)?))
                }
                type_id::video::VIDEO_FRAME => {
                    let header = MediaHeader::decode_any(&frame.payload)?;
                    let hdr_len = header.hdr_len();
                    if frame.payload.len() < hdr_len {
                        return Err(PlaneMessageError::Media(MediaError::BadHdrLen));
                    }
                    Ok(Self::VideoFrame {
                        header,
                        payload: frame.payload[hdr_len..].to_vec(),
                    })
                }
                type_id::video::STREAM_END => {
                    Ok(Self::StreamEnd(StreamEnd::decode(&frame.payload)?))
                }
                _ => Err(unsupported(frame)),
            },
            CH_FILE => match frame.type_id {
                type_id::file::FILE_CHUNK => {
                    Ok(Self::FileChunk(FileChunk::decode(&frame.payload)?))
                }
                type_id::file::FILE_ACK => Ok(Self::FileAck(FileAck::decode(&frame.payload)?)),
                type_id::file::FILE_END => Ok(Self::FileEnd(FileEnd::decode(&frame.payload)?)),
                type_id::file::FILE_ERROR => {
                    Ok(Self::FileError(FileErrorPayload::decode(&frame.payload)?))
                }
                type_id::file::FILE_CHUNK_ZSTD => Ok(Self::FileChunkZstd(
                    FileChunkZstdPayload::decode(&frame.payload)?,
                )),
                type_id::file::FILE_RESUME_HINT => Ok(Self::FileResumeHint(
                    FileResumeHint::decode(&frame.payload)?,
                )),
                _ => Err(unsupported(frame)),
            },
            CH_CURSOR => match frame.type_id {
                type_id::cursor::CURSOR_SHAPE => {
                    Ok(Self::CursorShape(CursorShape::decode(&frame.payload)?))
                }
                type_id::cursor::CURSOR_RESET => {
                    Ok(Self::CursorReset(CursorReset::decode(&frame.payload)?))
                }
                type_id::cursor::CURSOR_POS => {
                    Ok(Self::CursorPos(CursorPos::decode(&frame.payload)?))
                }
                _ => Err(unsupported(frame)),
            },
            CH_AUDIO => match frame.type_id {
                type_id::audio::AUDIO_FMT => Ok(Self::AudioFmt(AudioFmt::decode(&frame.payload)?)),
                type_id::audio::AUDIO_FRAME => {
                    Ok(Self::AudioFrame(AudioFrame::decode(&frame.payload)?))
                }
                _ => Err(unsupported(frame)),
            },
            CH_FEEDBACK => match frame.type_id {
                type_id::feedback::FRAME_ACK => {
                    Ok(Self::FrameAck(FrameAck::decode(&frame.payload)?))
                }
                type_id::feedback::NET_REPORT => {
                    Ok(Self::NetReport(NetReport::decode(&frame.payload)?))
                }
                type_id::feedback::SLICE_NACK => {
                    Ok(Self::SliceNack(SliceNack::decode(&frame.payload)?))
                }
                _ => Err(unsupported(frame)),
            },
            CH_CLIPBOARD => match frame.type_id {
                type_id::clipboard::CLIP_DATA => {
                    Ok(Self::ClipData(ClipData::decode(&frame.payload)?))
                }
                type_id::clipboard::CLIP_ACK => Ok(Self::ClipAck(ClipAck::decode(&frame.payload)?)),
                _ => Err(unsupported(frame)),
            },
            other => Err(PlaneMessageError::BadChannel(other)),
        }
    }
}

fn unsupported(frame: &Frame) -> PlaneMessageError {
    PlaneMessageError::Unsupported {
        channel: frame.channel,
        type_id: frame.type_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Codec;
    use crate::data::{AudioHeader, ClipDataHeader, InputEvent};
    use crate::plane::{CursorShapeHeader, FileChunkZstd};

    fn frame(channel: u8, type_id: u16, payload: Vec<u8>) -> Frame {
        Frame {
            channel,
            type_id,
            flags: 0,
            payload,
        }
    }

    #[test]
    fn decodes_every_registered_fixed_or_variable_payload() {
        let input = InputBatch {
            seq: 1,
            ts_us: 2,
            monitor: 0,
            events: vec![InputEvent::Move { x: 3, y: 4 }],
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_INPUT, 1, input.encode().unwrap())).unwrap(),
            PlaneMessage::Input(input)
        );

        let hello = StreamHello {
            channel: 1,
            display: 0,
            codec: 0,
            profile: 0,
            chroma: 1,
            width: 1920,
            height: 1080,
            fps: 60,
            kbps: 8_000,
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_VIDEO, 0, hello.encode().to_vec())).unwrap(),
            PlaneMessage::StreamHello(hello)
        );

        let header = MediaHeader::new(Codec::H264, 7, 11);
        let mut video = header.encode().unwrap();
        video.extend_from_slice(&[0xaa, 0xbb]);
        assert_eq!(
            PlaneMessage::decode(&frame(CH_VIDEO, 1, video)).unwrap(),
            PlaneMessage::VideoFrame {
                header,
                payload: vec![0xaa, 0xbb],
            }
        );

        let end = StreamEnd { reason: 3 };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_VIDEO, 2, end.encode().to_vec())).unwrap(),
            PlaneMessage::StreamEnd(end)
        );

        let chunk = FileChunk {
            offset: 0,
            data: vec![1, 2, 3],
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_FILE, 1, chunk.encode().unwrap())).unwrap(),
            PlaneMessage::FileChunk(chunk)
        );

        let ack = FileAck {
            received_upto: 3,
            credit: 8,
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_FILE, 2, ack.encode().to_vec())).unwrap(),
            PlaneMessage::FileAck(ack)
        );

        let file_end = FileEnd { sha256: [9; 32] };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_FILE, 3, file_end.encode().to_vec())).unwrap(),
            PlaneMessage::FileEnd(file_end)
        );

        let file_error = FileErrorPayload {
            task_id: 7,
            code: 61,
            detail: "permission denied".to_owned(),
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_FILE, 4, file_error.encode().unwrap())).unwrap(),
            PlaneMessage::FileError(file_error)
        );

        let zstd = FileChunkZstdPayload {
            header: FileChunkZstd {
                offset: 0,
                uncompressed_len: 3,
            },
            data: vec![0x28, 0xb5, 0x2f, 0xfd],
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_FILE, 5, zstd.encode().unwrap())).unwrap(),
            PlaneMessage::FileChunkZstd(zstd)
        );

        let hint = FileResumeHint {
            task_id: 1,
            resume_offset: 2,
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_FILE, 6, hint.encode().to_vec())).unwrap(),
            PlaneMessage::FileResumeHint(hint)
        );

        let shape = CursorShape {
            header: CursorShapeHeader {
                cursor_id: 1,
                generation: 2,
                hx: 0,
                hy: 0,
                width: 2,
                height: 2,
                format: 0,
            },
            data: vec![0; 16],
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_CURSOR, 1, shape.encode().unwrap())).unwrap(),
            PlaneMessage::CursorShape(shape)
        );

        let reset = CursorReset { generation: 2 };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_CURSOR, 2, reset.encode().to_vec())).unwrap(),
            PlaneMessage::CursorReset(reset)
        );

        let pos = CursorPos {
            x: 1,
            y: 2,
            shape_id: 3,
            generation: 4,
            visible: true,
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_CURSOR, 3, pos.encode().to_vec())).unwrap(),
            PlaneMessage::CursorPos(pos)
        );

        let fmt = AudioFmt {
            codec: 1,
            rate: 48_000,
            channels: 2,
            frame_ms: 20,
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_AUDIO, 1, fmt.encode().to_vec())).unwrap(),
            PlaneMessage::AudioFmt(fmt)
        );

        let audio = AudioFrame {
            header: AudioHeader { seq: 1, pts_us: 2 },
            data: vec![1, 2],
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_AUDIO, 2, audio.encode().unwrap())).unwrap(),
            PlaneMessage::AudioFrame(audio)
        );

        let frame_ack = FrameAck {
            ack_frame_id: 1,
            recv_ts_us: 2,
            decode_done_us: 3,
            queue_depth: 4,
            dropped: 0,
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_FEEDBACK, 1, frame_ack.encode().to_vec())).unwrap(),
            PlaneMessage::FrameAck(frame_ack)
        );

        let net = NetReport {
            rtt_us: 1,
            loss_ppm: 2,
            recv_bps: 3,
            queue_ms: 4,
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_FEEDBACK, 2, net.encode().to_vec())).unwrap(),
            PlaneMessage::NetReport(net)
        );

        let nack = SliceNack {
            frame_id: 5,
            missing_bitmap: 6,
            ..Default::default()
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_FEEDBACK, 3, nack.encode().to_vec())).unwrap(),
            PlaneMessage::SliceNack(nack)
        );

        let clip = ClipData {
            header: ClipDataHeader {
                seq: 1,
                format: 1,
                offset: 0,
            },
            data: vec![1, 2, 3],
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_CLIPBOARD, 1, clip.encode().unwrap())).unwrap(),
            PlaneMessage::ClipData(clip)
        );

        let clip_ack = ClipAck {
            seq: 1,
            received_upto: 3,
            credit: 4,
        };
        assert_eq!(
            PlaneMessage::decode(&frame(CH_CLIPBOARD, 2, clip_ack.encode().to_vec())).unwrap(),
            PlaneMessage::ClipAck(clip_ack)
        );
    }

    #[test]
    fn file_error_rejects_truncated_and_trailing() {
        let short = PlaneMessage::decode(&frame(CH_FILE, 4, vec![1, 2, 3])).unwrap_err();
        assert_eq!(short, PlaneMessageError::Payload(PayloadError::TooShort));

        let mut trailing = FileErrorPayload {
            task_id: 1,
            code: 2,
            detail: "x".to_owned(),
        }
        .encode()
        .unwrap();
        trailing.push(0);
        let err = PlaneMessage::decode(&frame(CH_FILE, 4, trailing)).unwrap_err();
        assert_eq!(err, PlaneMessageError::Payload(PayloadError::Trailing));
    }
}
