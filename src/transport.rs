//! 传输抽象：协议引擎只依赖 [`MessageIo`]；QUIC/TCP/异步泵在 `opsdesk-net` 实现。

use std::io;
use std::time::Duration;

/// 纯消息边界：协议引擎只需要这五个动作，不感知 TCP/QUIC/流/异步。
///
/// 硬契约（docs/45 §7.10.1）：`send` 一次只提交一条完整消息，`recv` 一次只返回
/// 一条完整消息；底层字节流的分帧/重组由实现负责，调用方不得二次分帧。
/// 半条消息返回 `UnexpectedEof`，不得静默截断。
pub trait MessageIo: Send {
    /// 返回 `Ok(None)` 表示对端关闭。
    fn recv(&mut self) -> io::Result<Option<Vec<u8>>>;

    /// 读取下一条控制面消息。多流传输应只从 control 流读取；单流实现
    /// 默认退化为 `recv`。
    fn recv_control(&mut self) -> io::Result<Option<Vec<u8>>> {
        self.recv()
    }

    /// 带超时的单消息读取；超时返回 `WouldBlock`，正常关闭返回 `Ok(None)`。
    fn recv_timeout(&mut self, _timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "recv_timeout is not supported by this transport",
        ))
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// 等待此前提交的 `send`/`send_video_batch` 真正写出。
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn close(&mut self) -> io::Result<()>;
}
