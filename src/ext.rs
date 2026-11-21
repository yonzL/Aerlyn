//! 通用 TLV 扩展容器。
//!
//! 0.4-dev 还没有把扩展块嵌进所有消息；本模块先把线格式和未知 tag 的
//! 前向兼容规则固定下来，供后续可选扩展字段使用。
//!
//! 线格式（大端）：
//!
//! ```text
//! u16 count
//! repeat count:
//!   u16 tag
//!   u16 len
//!   u8  value[len]
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_EXTENSIONS: usize = 64;
pub const MAX_EXT_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extension {
    pub tag: u16,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extensions {
    items: Vec<Extension>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtError {
    TooManyExtensions(usize),
    TooLarge(usize),
    DuplicateTag(u16),
    Truncated,
    Trailing,
    BadLength(usize),
}

impl fmt::Display for ExtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyExtensions(n) => write!(f, "too many extensions: {n}"),
            Self::TooLarge(n) => write!(f, "extensions too large: {n} bytes"),
            Self::DuplicateTag(tag) => write!(f, "duplicate extension tag: {tag}"),
            Self::Truncated => f.write_str("truncated extension block"),
            Self::Trailing => f.write_str("trailing bytes after extension block"),
            Self::BadLength(n) => write!(f, "bad extension length: {n}"),
        }
    }
}

impl std::error::Error for ExtError {}

impl Extensions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, tag: u16) -> Option<&[u8]> {
        self.items
            .iter()
            .find(|e| e.tag == tag)
            .map(|e| e.value.as_slice())
    }

    pub fn items(&self) -> &[Extension] {
        &self.items
    }

    pub fn into_items(self) -> Vec<Extension> {
        self.items
    }

    pub fn push(&mut self, tag: u16, value: impl Into<Vec<u8>>) -> Result<(), ExtError> {
        if self.items.len() >= MAX_EXTENSIONS {
            return Err(ExtError::TooManyExtensions(self.items.len() + 1));
        }
        if self.items.iter().any(|e| e.tag == tag) {
            return Err(ExtError::DuplicateTag(tag));
        }
        let value = value.into();
        let total = self.encoded_len() + 4 + value.len();
        if total > MAX_EXT_BYTES {
            return Err(ExtError::TooLarge(total));
        }
        self.items.push(Extension { tag, value });
        Ok(())
    }

    /// 编码后的总长度；调用方可用它做外层预算。
    pub fn encoded_len(&self) -> usize {
        2 + self.items.iter().map(|e| 4 + e.value.len()).sum::<usize>()
    }

    pub fn encode(&self) -> Result<Vec<u8>, ExtError> {
        let len = self.encoded_len();
        if len > MAX_EXT_BYTES {
            return Err(ExtError::TooLarge(len));
        }
        let mut out = Vec::with_capacity(len);
        out.extend_from_slice(&(self.items.len() as u16).to_be_bytes());
        for e in &self.items {
            if e.value.len() > u16::MAX as usize {
                return Err(ExtError::BadLength(e.value.len()));
            }
            out.extend_from_slice(&e.tag.to_be_bytes());
            out.extend_from_slice(&(e.value.len() as u16).to_be_bytes());
            out.extend_from_slice(&e.value);
        }
        Ok(out)
    }

    /// 解码扩展块；`known` 之外的 tag 跳过，返回值第二项是跳过数量。
    pub fn decode(data: &[u8], known: &[u16]) -> Result<(Self, u32), ExtError> {
        if data.len() > MAX_EXT_BYTES {
            return Err(ExtError::TooLarge(data.len()));
        }
        if data.len() < 2 {
            return Err(ExtError::Truncated);
        }
        let count = u16::from_be_bytes([data[0], data[1]]) as usize;
        if count > MAX_EXTENSIONS {
            return Err(ExtError::TooManyExtensions(count));
        }
        let mut pos = 2usize;
        let mut items = Vec::with_capacity(count);
        let mut skipped = 0u32;
        for _ in 0..count {
            if pos + 4 > data.len() {
                return Err(ExtError::Truncated);
            }
            let tag = u16::from_be_bytes([data[pos], data[pos + 1]]);
            let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
            pos += 4;
            if pos + len > data.len() {
                return Err(ExtError::Truncated);
            }
            let value = &data[pos..pos + len];
            pos += len;
            if !known.contains(&tag) {
                skipped += 1;
                continue;
            }
            if items.iter().any(|e: &Extension| e.tag == tag) {
                return Err(ExtError::DuplicateTag(tag));
            }
            items.push(Extension {
                tag,
                value: value.to_vec(),
            });
        }
        if pos != data.len() {
            return Err(ExtError::Trailing);
        }
        Ok((Self { items }, skipped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tags_roundtrip_and_unknown_are_counted() {
        let mut ext = Extensions::new();
        ext.push(1, b"one".to_vec()).unwrap();
        ext.push(2, b"two".to_vec()).unwrap();
        let mut wire = ext.encode().unwrap();
        // 追加一个未知 tag 3，并修正 count。
        wire[0..2].copy_from_slice(&3u16.to_be_bytes());
        wire.extend_from_slice(&3u16.to_be_bytes());
        wire.extend_from_slice(&2u16.to_be_bytes());
        wire.extend_from_slice(b"xx");
        let (decoded, skipped) = Extensions::decode(&wire, &[1, 2]).unwrap();
        assert_eq!(decoded, ext);
        assert_eq!(skipped, 1);
        assert_eq!(decoded.get(1), Some(b"one".as_slice()));
    }

    #[test]
    fn duplicate_known_tag_is_rejected() {
        let mut ext = Extensions::new();
        ext.push(7, b"a".to_vec()).unwrap();
        let mut wire = ext.encode().unwrap();
        wire[0..2].copy_from_slice(&2u16.to_be_bytes());
        wire.extend_from_slice(&7u16.to_be_bytes());
        wire.extend_from_slice(&1u16.to_be_bytes());
        wire.push(b'b');
        assert_eq!(
            Extensions::decode(&wire, &[7]),
            Err(ExtError::DuplicateTag(7))
        );
    }

    #[test]
    fn truncated_and_trailing_are_rejected() {
        assert_eq!(
            Extensions::decode(&[0, 1, 0], &[]),
            Err(ExtError::Truncated)
        );
        assert_eq!(Extensions::decode(&[0, 0, 9], &[]), Err(ExtError::Trailing));
    }

    #[test]
    fn oversize_and_duplicate_push_are_rejected() {
        let mut ext = Extensions::new();
        assert_eq!(
            ext.push(1, vec![0u8; MAX_EXT_BYTES]),
            Err(ExtError::TooLarge(MAX_EXT_BYTES + 6))
        );
        ext.push(1, b"x".to_vec()).unwrap();
        assert_eq!(ext.push(1, b"y".to_vec()), Err(ExtError::DuplicateTag(1)));
    }
}
