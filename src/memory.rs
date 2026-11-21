//! 内存回环连接：给 service/client 的测试与示例用，不涉及真实网络。

use std::io;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::transport::MessageIo;

pub struct MemoryConnection {
    tx: Option<Sender<Vec<u8>>>,
    rx: Receiver<Vec<u8>>,
    dgram_tx: Option<Sender<Vec<u8>>>,
    dgram_rx: Receiver<Vec<u8>>,
    closed: bool,
}

/// 建立一对互通的连接：A 的 send 到 B 的 recv，反之亦然。
pub fn pair() -> (MemoryConnection, MemoryConnection) {
    let (a_tx, a_rx) = mpsc::channel();
    let (b_tx, b_rx) = mpsc::channel();
    let (a_dgram_tx, a_dgram_rx) = mpsc::channel();
    let (b_dgram_tx, b_dgram_rx) = mpsc::channel();
    (
        MemoryConnection {
            tx: Some(a_tx),
            rx: b_rx,
            dgram_tx: Some(a_dgram_tx),
            dgram_rx: b_dgram_rx,
            closed: false,
        },
        MemoryConnection {
            tx: Some(b_tx),
            rx: a_rx,
            dgram_tx: Some(b_dgram_tx),
            dgram_rx: a_dgram_rx,
            closed: false,
        },
    )
}

impl MessageIo for MemoryConnection {
    fn recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self.rx.recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(_) => Ok(None),
        }
    }

    fn recv_timeout(&mut self, timeout: std::time::Duration) -> io::Result<Option<Vec<u8>>> {
        match self.rx.recv_timeout(timeout) {
            Ok(msg) => Ok(Some(msg)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "memory receive timed out",
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.closed {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        let tx = self
            .tx
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "closed"))?;
        tx.send(bytes.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "peer closed"))
    }

    fn close(&mut self) -> io::Result<()> {
        self.closed = true;
        self.tx = None;
        self.dgram_tx = None;
        Ok(())
    }
}

impl MemoryConnection {
    pub fn send_datagram_raw(&mut self, data: &[u8]) -> io::Result<()> {
        if self.closed {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        if data.len() > self.max_datagram_size_raw() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "datagram exceeds 1200 bytes",
            ));
        }
        let tx = self
            .dgram_tx
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "closed"))?;
        tx.send(data.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "peer closed"))
    }

    pub fn recv_datagram_raw(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self.dgram_rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => Ok(None),
        }
    }

    pub fn max_datagram_size_raw(&self) -> usize {
        1200
    }
}

/// 一条把每个消息重新分块、并允许把多条消息拼进同一底层写入的内存连接。
///
/// `MemoryConnection` 模拟“一次 send 恰好一次 recv”的理想传输；它无法暴露
/// 分帧/合帧错误。`ChoppyConnection` 在发送侧加上 4 字节长度前缀，再随机切成
/// 1..=max_chunk 字节的块；接收侧必须按长度前缀重组。测试可据此覆盖：
///
/// - 一条消息被拆成多块；
/// - 多条消息被合并到一次底层写入；
/// - 半条消息/截断；
/// - 超时与关闭。
pub struct ChoppyConnection {
    tx: Option<Sender<Vec<u8>>>,
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    rng: u64,
    max_chunk: usize,
    closed: bool,
}

/// 建立一对 `ChoppyConnection`；`seed` 决定分块序列，`max_chunk` 是单块上限。
pub fn choppy_pair(seed: u64, max_chunk: usize) -> (ChoppyConnection, ChoppyConnection) {
    let (a_tx, a_rx) = mpsc::channel();
    let (b_tx, b_rx) = mpsc::channel();
    let max_chunk = max_chunk.max(1);
    (
        ChoppyConnection {
            tx: Some(a_tx),
            rx: b_rx,
            buf: Vec::new(),
            rng: seed ^ 0x9E37_79B9_7F4A_7C15,
            max_chunk,
            closed: false,
        },
        ChoppyConnection {
            tx: Some(b_tx),
            rx: a_rx,
            buf: Vec::new(),
            rng: seed ^ 0xD1B5_4A32_D192_ED03,
            max_chunk,
            closed: false,
        },
    )
}

impl ChoppyConnection {
    fn next_random(&mut self) -> usize {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng >> 33) as usize
    }

    fn write_frame(&mut self, message: &[u8]) -> io::Result<()> {
        if self.closed {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        if message.len() > crate::channel::MAX_FRAME_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "message exceeds frame limit",
            ));
        }
        let tx = self
            .tx
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "closed"))?
            .clone();
        let mut wire = Vec::with_capacity(4 + message.len());
        wire.extend_from_slice(&(message.len() as u32).to_be_bytes());
        wire.extend_from_slice(message);
        let mut offset = 0;
        while offset < wire.len() {
            let remaining = wire.len() - offset;
            let upper = self.max_chunk.min(remaining).max(1);
            let len = 1 + self.next_random() % upper;
            tx.send(wire[offset..offset + len].to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "peer closed"))?;
            offset += len;
        }
        Ok(())
    }

    fn take_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
        if self.buf.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
        if len > crate::channel::MAX_FRAME_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "framed message exceeds limit",
            ));
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        let message = self.buf[4..4 + len].to_vec();
        self.buf.drain(..4 + len);
        Ok(Some(message))
    }

    /// 把多条完整消息编码进同一底层写入流，用于验证合帧。
    pub fn send_batch(&mut self, messages: &[Vec<u8>]) -> io::Result<()> {
        if self.closed {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        let tx = self
            .tx
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "closed"))?
            .clone();
        let mut wire = Vec::new();
        for message in messages {
            if message.len() > crate::channel::MAX_FRAME_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "message exceeds frame limit",
                ));
            }
            wire.extend_from_slice(&(message.len() as u32).to_be_bytes());
            wire.extend_from_slice(message);
        }
        let mut offset = 0;
        while offset < wire.len() {
            let remaining = wire.len() - offset;
            let upper = self.max_chunk.min(remaining).max(1);
            let len = 1 + self.next_random() % upper;
            tx.send(wire[offset..offset + len].to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "peer closed"))?;
            offset += len;
        }
        Ok(())
    }
}

impl MessageIo for ChoppyConnection {
    fn recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(message) = self.take_frame()? {
                return Ok(Some(message));
            }
            match self.rx.recv() {
                Ok(chunk) => self.buf.extend_from_slice(&chunk),
                Err(_) if self.buf.is_empty() => return Ok(None),
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "truncated framed message",
                    ))
                }
            }
        }
    }

    fn recv_timeout(&mut self, timeout: std::time::Duration) -> io::Result<Option<Vec<u8>>> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(message) = self.take_frame()? {
                return Ok(Some(message));
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "choppy receive timed out",
                ));
            }
            match self.rx.recv_timeout(remaining) {
                Ok(chunk) => self.buf.extend_from_slice(&chunk),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "choppy receive timed out",
                    ))
                }
                Err(mpsc::RecvTimeoutError::Disconnected) if self.buf.is_empty() => {
                    return Ok(None)
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "truncated framed message",
                    ))
                }
            }
        }
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.write_frame(bytes)
    }

    fn close(&mut self) -> io::Result<()> {
        self.closed = true;
        self.tx = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MessageIo;

    #[test]
    fn pair_roundtrip() {
        let (mut a, mut b) = pair();
        a.send(b"hello").unwrap();
        assert_eq!(b.recv().unwrap(), Some(b"hello".to_vec()));
        b.send(b"world").unwrap();
        assert_eq!(a.recv().unwrap(), Some(b"world".to_vec()));
    }

    #[test]
    fn datagram_roundtrip() {
        let (mut a, mut b) = pair();
        a.send_datagram_raw(b"audio").unwrap();
        assert_eq!(b.recv_datagram_raw().unwrap(), Some(b"audio".to_vec()));
        assert_eq!(a.recv_datagram_raw().unwrap(), None);
    }

    #[test]
    fn close_makes_peer_eof() {
        let (mut a, mut b) = pair();
        a.close().unwrap();
        assert_eq!(b.recv().unwrap(), None);
    }

    #[test]
    fn choppy_pair_preserves_messages_across_random_chunks() {
        let (mut a, mut b) = choppy_pair(7, 13);
        let mut sent = Vec::new();
        for i in 0..200usize {
            let len = (i * 37) % 300;
            let message = (0..len).map(|j| (i + j) as u8).collect::<Vec<_>>();
            a.send(&message).unwrap();
            sent.push(message);
        }
        for message in sent {
            let got = b
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .expect("message");
            assert_eq!(got, message);
        }
        assert!(matches!(
            b.recv_timeout(std::time::Duration::from_millis(10)),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock
        ));
    }

    #[test]
    fn choppy_pair_reassembles_coalesced_messages() {
        let (mut a, mut b) = choppy_pair(11, 29);
        let messages = vec![b"alpha".to_vec(), vec![0u8; 0], vec![7u8; 65]];
        a.send_batch(&messages).unwrap();
        for expected in messages {
            let got = b
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .expect("message");
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn message_io_trait_only_needs_five_actions() {
        fn read_one<C: MessageIo>(conn: &mut C) -> Vec<u8> {
            conn.recv().unwrap().expect("message")
        }
        let (mut a, mut b) = choppy_pair(5, 7);
        a.send(b"message-io").unwrap();
        assert_eq!(read_one(&mut b), b"message-io");
    }

    #[test]
    fn choppy_pair_reports_truncated_message() {
        let (mut a, mut b) = choppy_pair(3, 64);
        a.tx.as_ref().unwrap().send(vec![0, 0, 0, 8, 1]).unwrap();
        a.tx = None;
        assert!(matches!(
            b.recv(),
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof
        ));
    }
}
