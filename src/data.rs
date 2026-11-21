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
    pub fn encode(&self) -> [u8; 15] {
        let mut out = [0u8; 15];
        out[0..4].copy_from_slice(&self.x.to_be_bytes());
        out[4..8].copy_from_slice(&self.y.to_be_bytes());
        out[8..12].copy_from_slice(&self.shape_id.to_be_bytes());
        out[12..14].copy_from_slice(&self.generation.to_be_bytes());
        out[14] = u8::from(self.visible);
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() != 15 {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileAck {
    pub received_upto: u64,
    pub credit: u16,
}

impl FileAck {
    pub fn encode(&self) -> [u8; 10] {
        let mut out = [0u8; 10];
        out[0..8].copy_from_slice(&self.received_upto.to_be_bytes());
        out[8..10].copy_from_slice(&self.credit.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() != 10 {
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
    pub fn encode(&self) -> [u8; 12] {
        let mut out = [0u8; 12];
        out[0..4].copy_from_slice(&self.seq.to_be_bytes());
        out[4..12].copy_from_slice(&self.pts_us.to_be_bytes());
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InputError> {
        if buf.len() < 12 {
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
