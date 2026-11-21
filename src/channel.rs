//! 通道与分帧：ch0 JSON 控制、其它通道二进制 `[u8 ch][u16 type][u16 flags][u32 len]`。
//!
//! 真实 QUIC 每通道一条流；内存回环把 channel 放进帧头，语义一致。

use std::io;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::transport::MessageIo;

pub const CH_CONTROL: u8 = 0;
pub const CH_INPUT: u8 = 1;
pub const CH_VIDEO: u8 = 2;
pub const CH_FILE: u8 = 3;
pub const CH_CURSOR: u8 = 4;
pub const CH_AUDIO: u8 = 5;
pub const CH_FEEDBACK: u8 = 6;
pub const CH_CLIPBOARD: u8 = 7;

pub const MAX_CONTROL_LEN: usize = 1 << 20;
/// 未鉴权握手阶段单条 control 消息上限。
pub const MAX_HANDSHAKE_CONTROL_LEN: usize = 128 << 10;
pub const MAX_FRAME_LEN: usize = 16 << 20;
pub const FRAME_HEADER_LEN: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub channel: u8,
    pub type_id: u16,
    pub flags: u16,
    pub payload: Vec<u8>,
}

/// ch0 或 ch1–7 的任意一条消息；ch0 长度前缀首字节必为 0，通道号必为 1–7，
/// 因此首字节足以区分两类帧。
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Control(crate::message::ControlMessage),
    Frame(Frame),
}

impl Frame {
    pub fn json<T: DeserializeOwned>(&self) -> io::Result<T> {
        serde_json::from_slice(&self.payload)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

fn recv_one<C: MessageIo>(conn: &mut C) -> io::Result<Vec<u8>> {
    conn.recv()?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed"))
}

/// ch0：`[u32 len][json]`。
pub fn write_control<C: MessageIo, T: Serialize>(conn: &mut C, msg: &T) -> io::Result<()> {
    let bytes =
        serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_control_bytes(conn, &bytes)
}

pub fn read_control<C: MessageIo, T: DeserializeOwned>(conn: &mut C) -> io::Result<T> {
    let bytes = read_control_bytes_limit(conn, MAX_CONTROL_LEN)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// `ControlMessage` 的手动 JSON 编解码（未知 type 需要保留原始值）。
pub fn write_control_msg<C: MessageIo>(
    conn: &mut C,
    msg: &crate::message::ControlMessage,
) -> io::Result<()> {
    let text = msg
        .to_json()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))?;
    write_control_bytes(conn, text.as_bytes())
}

pub fn read_control_msg<C: MessageIo>(conn: &mut C) -> io::Result<crate::message::ControlMessage> {
    read_control_msg_limit(conn, MAX_CONTROL_LEN)
}

/// 带显式长度上限的 `ControlMessage` 读取；握手阶段用 128 KiB，其它阶段用
/// `MAX_CONTROL_LEN`。
pub fn read_control_msg_limit<C: MessageIo>(
    conn: &mut C,
    max_len: usize,
) -> io::Result<crate::message::ControlMessage> {
    let bytes = read_control_bytes_limit(conn, max_len)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    crate::message::ControlMessage::from_json(text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))
}

fn write_control_bytes<C: MessageIo>(conn: &mut C, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_CONTROL_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control message too large",
        ));
    }
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
    conn.send(&out)
}

fn read_control_bytes_limit<C: MessageIo>(conn: &mut C, max_len: usize) -> io::Result<Vec<u8>> {
    let buf = conn
        .recv_control()?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "control channel closed"))?;
    if buf.len() < 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control frame shorter than 4 bytes",
        ));
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len == 0 || len > max_len.min(MAX_CONTROL_LEN) || len + 4 != buf.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "bad control frame length: len={len}, buf={} bytes, head={:02x?}",
                buf.len(),
                &buf[..buf.len().min(8)]
            ),
        ));
    }
    Ok(buf[4..].to_vec())
}

/// 编码一条非 ch0 二进制帧，但不写出。
///
/// 异步视频发送泵需要先把整帧的多个分片编码成独立消息，再交给传输层
/// 的有界队列；这样队列项就是完整的 wire 消息。
pub fn encode_frame(channel: u8, type_id: u16, flags: u16, payload: &[u8]) -> io::Result<Vec<u8>> {
    if channel == CH_CONTROL {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "use write_control for ch0",
        ));
    }
    if payload.len() > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "binary frame too large",
        ));
    }
    let mut out = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    out.push(channel);
    out.extend_from_slice(&type_id.to_be_bytes());
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// 非 ch0：`[u8 ch][u16 type][u16 flags][u32 len][payload]`。
pub fn write_frame<C: MessageIo>(
    conn: &mut C,
    channel: u8,
    type_id: u16,
    flags: u16,
    payload: &[u8],
) -> io::Result<()> {
    let out = encode_frame(channel, type_id, flags, payload)?;
    conn.send(&out)
}

pub fn read_frame<C: MessageIo>(conn: &mut C) -> io::Result<Frame> {
    let buf = recv_one(conn)?;
    parse_frame(&buf)
}

fn parse_frame(buf: &[u8]) -> io::Result<Frame> {
    if buf.len() < FRAME_HEADER_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "binary frame shorter than header",
        ));
    }
    let channel = buf[0];
    if channel == CH_CONTROL {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ch0 frame in binary reader",
        ));
    }
    let type_id = u16::from_be_bytes([buf[1], buf[2]]);
    let flags = u16::from_be_bytes([buf[3], buf[4]]);
    let len = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]) as usize;
    if len > MAX_FRAME_LEN || len + FRAME_HEADER_LEN != buf.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad binary frame length",
        ));
    }
    Ok(Frame {
        channel,
        type_id,
        flags,
        payload: buf[FRAME_HEADER_LEN..].to_vec(),
    })
}

/// 读取“下一条 control 或二进制帧”，用于常驻多平面会话。
pub fn read_incoming<C: MessageIo>(conn: &mut C) -> io::Result<Incoming> {
    let buf = recv_one(conn)?;
    parse_incoming(&buf)
}

/// 带超时的 [`read_incoming`]；超时返回 `WouldBlock`。
pub fn read_incoming_timeout<C: MessageIo>(
    conn: &mut C,
    timeout: std::time::Duration,
) -> io::Result<Incoming> {
    let buf = conn
        .recv_timeout(timeout)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed"))?;
    parse_incoming(&buf)
}

fn parse_incoming(buf: &[u8]) -> io::Result<Incoming> {
    if buf.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty frame"));
    }
    match buf[0] {
        0 => {
            if buf.len() < 4 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "control frame shorter than 4 bytes",
                ));
            }
            let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
            if len == 0 || len > MAX_CONTROL_LEN || len + 4 != buf.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "bad control frame length",
                ));
            }
            let text = std::str::from_utf8(&buf[4..])
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let msg = crate::message::ControlMessage::from_json(text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))?;
            Ok(Incoming::Control(msg))
        }
        1..=7 => Ok(Incoming::Frame(parse_frame(buf)?)),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad channel {other}"),
        )),
    }
}

pub fn write_json_frame<C: MessageIo, T: Serialize>(
    conn: &mut C,
    channel: u8,
    type_id: u16,
    flags: u16,
    msg: &T,
) -> io::Result<()> {
    let payload =
        serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_frame(conn, channel, type_id, flags, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::pair;
    use crate::message::{FrameAck, Hello};
    use crate::transport::MessageIo;

    #[test]
    fn control_roundtrip() {
        let (mut a, mut b) = pair();
        let hello = Hello {
            version: 400,
            public_key: vec![1, 2, 3],
            caps: 0,
            caps_required: 0,
            codec_caps: vec![],
            caps_ext: vec![],
            actor_host: "test".into(),
            extensions: Vec::new(),
        };
        write_control(&mut a, &hello).unwrap();
        let back: Hello = read_control(&mut b).unwrap();
        assert_eq!(back, hello);
    }

    #[test]
    fn binary_frame_roundtrip() {
        let (mut a, mut b) = pair();
        write_frame(&mut a, CH_VIDEO, 1, 0x0001, b"payload").unwrap();
        let frame = read_frame(&mut b).unwrap();
        assert_eq!(frame.channel, CH_VIDEO);
        assert_eq!(frame.type_id, 1);
        assert_eq!(frame.flags, 0x0001);
        assert_eq!(frame.payload, b"payload");
    }

    #[test]
    fn frame_ack_binary_roundtrip() {
        let (mut a, mut b) = pair();
        let ack = FrameAck {
            ack_frame_id: 9,
            recv_ts_us: 1,
            decode_done_us: 2,
            queue_depth: 3,
            dropped: 4,
        };
        write_frame(&mut a, CH_FEEDBACK, 1, 0, &ack.encode()).unwrap();
        let frame = read_frame(&mut b).unwrap();
        assert_eq!(FrameAck::decode(&frame.payload).unwrap(), ack);
        assert_eq!(frame.payload.len(), FrameAck::LEN);
    }

    #[test]
    fn control_channel_rejected_in_binary_writer() {
        let (mut a, _b) = pair();
        assert!(write_frame(&mut a, CH_CONTROL, 1, 0, b"x").is_err());
    }

    #[test]
    fn oversized_control_rejected() {
        let (mut a, mut b) = pair();
        let big = vec![0u8; MAX_CONTROL_LEN + 1];
        let mut buf = Vec::new();
        buf.extend_from_slice(&(big.len() as u32).to_be_bytes());
        buf.extend_from_slice(&big);
        a.send(&buf).unwrap();
        assert!(read_control::<_, Hello>(&mut b).is_err());
    }

    #[test]
    fn handshake_message_limit_is_128_kib() {
        let (mut a, mut b) = pair();
        let big = vec![b'x'; MAX_HANDSHAKE_CONTROL_LEN + 1];
        let mut buf = Vec::new();
        buf.extend_from_slice(&(big.len() as u32).to_be_bytes());
        buf.extend_from_slice(&big);
        a.send(&buf).unwrap();
        let err = read_control_msg_limit(&mut b, MAX_HANDSHAKE_CONTROL_LEN).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
