//! 二进制热路径消息：输入批量、光标位置、文件/剪贴板 ack、音频头。
//! 布局对应 V4：输入 datagram、ch3 文件、ch7 剪贴板、ch5 音频。

use std::io;

pub const MAX_INPUT_EVENTS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Move {
        x: i32,
        y: i32,
    },
    MoveRel {
        dx: i32,
        dy: i32,
    },
    Button {
        button: u8,
        down: bool,
        x: i32,
        y: i32,
    },
    Wheel {
        delta: i32,
        pan: i32,
        x: i32,
        y: i32,
    },
    Key {
        vk: u16,
        scan: u32,
        chr: u32,
        mods: u16,
        down: bool,
    },
    Text {
        text: String,
    },
    Touch {
        id: u8,
        phase: u8,
        x: u16,
        y: u16,
    },
}

fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn get_i32(buf: &[u8], off: usize) -> i32 {
    i32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn get_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn get_u64(buf: &[u8], off: usize) -> u64 {
    u64::from_be_bytes([
        buf[off],
        buf[off + 1],
        buf[off + 2],
        buf[off + 3],
        buf[off + 4],
        buf[off + 5],
        buf[off + 6],
        buf[off + 7],
    ])
}

impl InputEvent {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::Move { x, y } => {
                out.push(1);
                out.push(0);
                put_i32(out, *x);
                put_i32(out, *y);
            }
            Self::MoveRel { dx, dy } => {
                out.push(2);
                out.push(0);
                put_i32(out, *dx);
                put_i32(out, *dy);
            }
            Self::Button { button, down, x, y } => {
                out.push(3);
                out.push(u8::from(*down));
                out.push(*button);
                put_i32(out, *x);
                put_i32(out, *y);
            }
            Self::Wheel { delta, pan, x, y } => {
                out.push(4);
                out.push(0);
                put_i32(out, *delta);
                put_i32(out, *pan);
                put_i32(out, *x);
                put_i32(out, *y);
            }
            Self::Key {
                vk,
                scan,
                chr,
                mods,
                down,
            } => {
                out.push(5);
                out.push(u8::from(*down));
                out.extend_from_slice(&vk.to_be_bytes());
                out.extend_from_slice(&scan.to_be_bytes());
                out.extend_from_slice(&chr.to_be_bytes());
                out.extend_from_slice(&mods.to_be_bytes());
            }
            Self::Text { text } => {
                out.push(6);
                out.push(0);
                out.push(text.len().min(255) as u8);
                out.extend_from_slice(&text.as_bytes()[..text.len().min(255)]);
            }
            Self::Touch { id, phase, x, y } => {
                out.push(7);
                out.push(0);
                out.push(*id);
                out.push(*phase);
                out.extend_from_slice(&x.to_be_bytes());
                out.extend_from_slice(&y.to_be_bytes());
            }
        }
    }

    fn decode(buf: &[u8], off: &mut usize) -> Result<Self, InputError> {
        if *off + 2 > buf.len() {
            return Err(InputError::TooShort);
        }
        let kind = buf[*off];
        let flags = buf[*off + 1];
        *off += 2;
        let need = match kind {
            1 | 2 => 8,
            3 => 9,
            4 => 16,
            5 => 12,
            6 => {
                if *off >= buf.len() {
                    return Err(InputError::TooShort);
                }
                let n = buf[*off] as usize;
                if *off + 1 + n > buf.len() {
                    return Err(InputError::TooShort);
                }
                let text = std::str::from_utf8(&buf[*off + 1..*off + 1 + n])
                    .map_err(|_| InputError::BadUtf8)?
                    .to_owned();
                *off += 1 + n;
                return Ok(Self::Text { text });
            }
            7 => 6,
            _ => return Err(InputError::BadKind),
        };
        if *off + need > buf.len() {
            return Err(InputError::TooShort);
        }
        let event = match kind {
            1 => Self::Move {
                x: get_i32(buf, *off),
                y: get_i32(buf, *off + 4),
            },
            2 => Self::MoveRel {
                dx: get_i32(buf, *off),
                dy: get_i32(buf, *off + 4),
            },
            3 => Self::Button {
                button: buf[*off],
                down: flags & 1 != 0,
                x: get_i32(buf, *off + 1),
                y: get_i32(buf, *off + 5),
            },
            4 => Self::Wheel {
                delta: get_i32(buf, *off),
                pan: get_i32(buf, *off + 4),
                x: get_i32(buf, *off + 8),
                y: get_i32(buf, *off + 12),
            },
            5 => Self::Key {
                vk: u16::from_be_bytes([buf[*off], buf[*off + 1]]),
                scan: get_u32(buf, *off + 2),
                chr: get_u32(buf, *off + 6),
                mods: u16::from_be_bytes([buf[*off + 10], buf[*off + 11]]),
                down: flags & 1 != 0,
            },
            7 => Self::Touch {
                id: buf[*off],
                phase: buf[*off + 1],
                x: u16::from_be_bytes([buf[*off + 2], buf[*off + 3]]),
                y: u16::from_be_bytes([buf[*off + 4], buf[*off + 5]]),
            },
            _ => return Err(InputError::BadKind),
        };
        *off += need;
        Ok(event)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputBatch {
    pub seq: u32,
    pub ts_us: u64,
    pub monitor: u8,
    pub events: Vec<InputEvent>,
}

impl InputBatch {
    pub fn encode(&self) -> Result<Vec<u8>, InputError> {
        if self.events.len() > MAX_INPUT_EVENTS {
            return Err(InputError::TooManyEvents);
        }
        let mut out = Vec::new();
        out.extend_from_slice(&self.seq.to_be_bytes());
        put_u64(&mut out, self.ts_us);
        out.push(self.monitor);
        out.push(self.events.len() as u8);
        for e in &self.events {
            e.encode(&mut out);
        }
        Ok(out)
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() < 14 {
            return Err(InputError::TooShort);
        }
        let seq = get_u32(buf, 0);
        let ts_us = get_u64(buf, 4);
        let monitor = buf[12];
        let count = buf[13] as usize;
        if count > MAX_INPUT_EVENTS {
            return Err(InputError::TooManyEvents);
        }
        let mut off = 14;
        let mut events = Vec::with_capacity(count);
        for _ in 0..count {
            events.push(InputEvent::decode(buf, &mut off)?);
        }
        if off != buf.len() {
            return Err(InputError::Trailing);
        }
        Ok(Self {
            seq,
            ts_us,
            monitor,
            events,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPos {
    pub x: i32,
    pub y: i32,
    pub shape_id: u32,
    pub generation: u16,
    pub visible: bool,
}

impl CursorPos {
    pub const LEN: usize = 15;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..4].copy_from_slice(&self.x.to_be_bytes());
        out[4..8].copy_from_slice(&self.y.to_be_bytes());
        out[8..12].copy_from_slice(&self.shape_id.to_be_bytes());
        out[12..14].copy_from_slice(&self.generation.to_be_bytes());
        out[14] = u8::from(self.visible);
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() != Self::LEN {
            return Err(InputError::TooShort);
        }
        Ok(Self {
            x: get_i32(buf, 0),
            y: get_i32(buf, 4),
            shape_id: get_u32(buf, 8),
            generation: u16::from_be_bytes([buf[12], buf[13]]),
            visible: buf[14] != 0,
        })
    }
}

/// Semantic cursor shapes aligned with the usual desktop/CSS set.
///
/// The wire value is a `u16`; unknown values are preserved so peers can add
/// shapes without a protocol break. `CursorShape` remains the bitmap fallback.
pub mod cursor_semantic {
    pub const DEFAULT: u16 = 0;
    pub const ARROW: u16 = 1;
    pub const IBEAM: u16 = 2;
    pub const CROSSHAIR: u16 = 3;
    pub const POINTING_HAND: u16 = 4;
    pub const RESIZE_N: u16 = 5;
    pub const RESIZE_S: u16 = 6;
    pub const RESIZE_E: u16 = 7;
    pub const RESIZE_W: u16 = 8;
    pub const RESIZE_NE: u16 = 9;
    pub const RESIZE_NW: u16 = 10;
    pub const RESIZE_SE: u16 = 11;
    pub const RESIZE_SW: u16 = 12;
    pub const RESIZE_EW: u16 = 13;
    pub const RESIZE_NS: u16 = 14;
    pub const RESIZE_ALL: u16 = 15;
    pub const NOT_ALLOWED: u16 = 16;
    pub const GRAB: u16 = 17;
    pub const GRABBING: u16 = 18;
    pub const HELP: u16 = 19;
    pub const WAIT: u16 = 20;
    pub const PROGRESS: u16 = 21;
    pub const CONTEXT_MENU: u16 = 22;
    pub const CELL: u16 = 23;
    pub const VERTICAL_TEXT: u16 = 24;
    pub const ALIAS: u16 = 25;
    pub const COPY: u16 = 26;
    pub const MOVE: u16 = 27;
    pub const NO_DROP: u16 = 28;
}

/// How the client should render the cursor.
pub mod cursor_render_mode {
    pub const LEGACY: u8 = 0;
    pub const SEPARATE: u8 = 1;
    pub const EMBEDDED: u8 = 2;
    pub const HIDDEN: u8 = 3;
    pub const UNKNOWN: u8 = 4;
}

/// Why the cursor is hidden when `render_mode == HIDDEN`.
pub mod cursor_hidden_reason {
    pub const NONE: u8 = 0;
    pub const SYSTEM_HIDDEN: u8 = 1;
    pub const SYSTEM_SUPPRESSED: u8 = 2;
    pub const NO_POINTING_DEVICE: u8 = 3;
    pub const SAMPLING_FAILED: u8 = 4;
    pub const SECURE_DESKTOP_PENDING: u8 = 5;
    pub const UNKNOWN: u8 = 255;
}

/// Semantic cursor state; `CursorPos` remains the bitmap-compatible message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    pub x: i32,
    pub y: i32,
    pub semantic: u16,
    pub render_mode: u8,
    pub hidden_reason: u8,
    pub visual_offset_x: i16,
    pub visual_offset_y: i16,
    pub display_id: u8,
    pub position_valid: bool,
    pub visible: bool,
    pub seq: u32,
}

impl CursorState {
    pub const LEN: usize = 22;
    pub const FLAG_POSITION_VALID: u8 = 1 << 0;
    pub const FLAG_VISIBLE: u8 = 1 << 1;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..4].copy_from_slice(&self.x.to_be_bytes());
        out[4..8].copy_from_slice(&self.y.to_be_bytes());
        out[8..10].copy_from_slice(&self.semantic.to_be_bytes());
        out[10] = self.render_mode;
        out[11] = self.hidden_reason;
        out[12..14].copy_from_slice(&self.visual_offset_x.to_be_bytes());
        out[14..16].copy_from_slice(&self.visual_offset_y.to_be_bytes());
        out[16] = self.display_id;
        let mut flags = 0u8;
        if self.position_valid {
            flags |= Self::FLAG_POSITION_VALID;
        }
        if self.visible {
            flags |= Self::FLAG_VISIBLE;
        }
        out[17] = flags;
        out[18..22].copy_from_slice(&self.seq.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() != Self::LEN {
            return Err(InputError::TooShort);
        }
        let flags = buf[17];
        Ok(Self {
            x: get_i32(buf, 0),
            y: get_i32(buf, 4),
            semantic: u16::from_be_bytes([buf[8], buf[9]]),
            render_mode: buf[10],
            hidden_reason: buf[11],
            visual_offset_x: i16::from_be_bytes([buf[12], buf[13]]),
            visual_offset_y: i16::from_be_bytes([buf[14], buf[15]]),
            display_id: buf[16],
            position_valid: flags & Self::FLAG_POSITION_VALID != 0,
            visible: flags & Self::FLAG_VISIBLE != 0,
            seq: u32::from_be_bytes([buf[18], buf[19], buf[20], buf[21]]),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileAck {
    pub received_upto: u64,
    pub credit: u16,
}

impl FileAck {
    pub const LEN: usize = 10;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..8].copy_from_slice(&self.received_upto.to_be_bytes());
        out[8..10].copy_from_slice(&self.credit.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() != Self::LEN {
            return Err(InputError::TooShort);
        }
        Ok(Self {
            received_upto: get_u64(buf, 0),
            credit: u16::from_be_bytes([buf[8], buf[9]]),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileChunkHeader {
    pub offset: u64,
}

impl FileChunkHeader {
    pub fn encode(&self) -> [u8; 8] {
        self.offset.to_be_bytes()
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() < 8 {
            return Err(InputError::TooShort);
        }
        Ok(Self {
            offset: get_u64(buf, 0),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipDataHeader {
    pub seq: u32,
    pub format: u16,
    pub offset: u32,
}

impl ClipDataHeader {
    pub fn encode(&self) -> [u8; 10] {
        let mut out = [0u8; 10];
        out[0..4].copy_from_slice(&self.seq.to_be_bytes());
        out[4..6].copy_from_slice(&self.format.to_be_bytes());
        out[6..10].copy_from_slice(&self.offset.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() < 10 {
            return Err(InputError::TooShort);
        }
        Ok(Self {
            seq: get_u32(buf, 0),
            format: u16::from_be_bytes([buf[4], buf[5]]),
            offset: get_u32(buf, 6),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipAck {
    pub seq: u32,
    pub received_upto: u32,
    pub credit: u16,
}

impl ClipAck {
    pub fn encode(&self) -> [u8; 10] {
        let mut out = [0u8; 10];
        out[0..4].copy_from_slice(&self.seq.to_be_bytes());
        out[4..8].copy_from_slice(&self.received_upto.to_be_bytes());
        out[8..10].copy_from_slice(&self.credit.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() != 10 {
            return Err(InputError::TooShort);
        }
        Ok(Self {
            seq: get_u32(buf, 0),
            received_upto: get_u32(buf, 4),
            credit: u16::from_be_bytes([buf[8], buf[9]]),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioHeader {
    pub seq: u32,
    pub pts_us: u64,
}

impl AudioHeader {
    pub const LEN: usize = 12;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[0..4].copy_from_slice(&self.seq.to_be_bytes());
        out[4..12].copy_from_slice(&self.pts_us.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() < Self::LEN {
            return Err(InputError::TooShort);
        }
        Ok(Self {
            seq: get_u32(buf, 0),
            pts_us: get_u64(buf, 4),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    TooShort,
    BadKind,
    BadUtf8,
    TooManyEvents,
    Trailing,
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for InputError {}

impl From<InputError> for io::Error {
    fn from(e: InputError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_batch_roundtrip() {
        let batch = InputBatch {
            seq: 9,
            ts_us: 1_700_000_000_000,
            monitor: 1,
            events: vec![
                InputEvent::Move { x: 100, y: 200 },
                InputEvent::Button {
                    button: 0,
                    down: true,
                    x: 100,
                    y: 200,
                },
                InputEvent::Wheel {
                    delta: -120,
                    pan: 0,
                    x: 100,
                    y: 200,
                },
                InputEvent::Key {
                    vk: 65,
                    scan: 4,
                    chr: 97,
                    mods: 2,
                    down: true,
                },
                InputEvent::Text {
                    text: "你好".into(),
                },
                InputEvent::Touch {
                    id: 1,
                    phase: 2,
                    x: 300,
                    y: 400,
                },
            ],
        };
        let bytes = batch.encode().unwrap();
        assert_eq!(InputBatch::decode(&bytes).unwrap(), batch);
    }

    #[test]
    fn input_batch_rejects_too_many_events() {
        let batch = InputBatch {
            seq: 1,
            ts_us: 0,
            monitor: 0,
            events: vec![InputEvent::Move { x: 0, y: 0 }; MAX_INPUT_EVENTS + 1],
        };
        assert_eq!(batch.encode(), Err(InputError::TooManyEvents));
    }

    #[test]
    fn cursor_pos_roundtrip() {
        let p = CursorPos {
            x: -1,
            y: 42,
            shape_id: 7,
            generation: 3,
            visible: true,
        };
        assert_eq!(CursorPos::decode(&p.encode()).unwrap(), p);
    }

    #[test]
    fn cursor_state_roundtrip_preserves_unknown_semantics() {
        let s = CursorState {
            x: -12,
            y: 34,
            semantic: 999,
            render_mode: cursor_render_mode::SEPARATE,
            hidden_reason: cursor_hidden_reason::NONE,
            visual_offset_x: -3,
            visual_offset_y: 5,
            display_id: 2,
            position_valid: true,
            visible: true,
            seq: 17,
        };
        assert_eq!(CursorState::decode(&s.encode()).unwrap(), s);
        assert_eq!(s.encode().len(), CursorState::LEN);
    }

    #[test]
    fn file_and_clipboard_acks_roundtrip() {
        let f = FileAck {
            received_upto: 1 << 40,
            credit: 8,
        };
        assert_eq!(FileAck::decode(&f.encode()).unwrap(), f);
        let c = ClipAck {
            seq: 3,
            received_upto: 1024,
            credit: 4,
        };
        assert_eq!(ClipAck::decode(&c.encode()).unwrap(), c);
        let h = ClipDataHeader {
            seq: 3,
            format: 0,
            offset: 512,
        };
        assert_eq!(ClipDataHeader::decode(&h.encode()).unwrap(), h);
        let chunk = FileChunkHeader { offset: 1 << 30 };
        assert_eq!(FileChunkHeader::decode(&chunk.encode()).unwrap(), chunk);
    }

    #[test]
    fn audio_header_roundtrip() {
        let a = AudioHeader {
            seq: 5,
            pts_us: 123_456,
        };
        assert_eq!(AudioHeader::decode(&a.encode()).unwrap(), a);
    }
}
