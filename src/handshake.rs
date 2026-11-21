//! 版本协商与两次握手（Hello → Challenge → Auth → Welcome）。
//!
//! 只做应用层流程；签名/验签通过 `Signer`/`Verifier` 注入。真实实现用 ed25519，
//! 测试用假签名器，避免把密码学依赖塞进协议地基。

use std::io;

use sha2::{Digest, Sha256};

use crate::channel;
use crate::codec::CodecCap;
use crate::control::{Resume, ResumeError, ResumeResult, ResumeTicket};
use crate::ext::{Extension, Extensions};
use crate::message::{Auth, Bye, Challenge, ControlMessage, Hello, Welcome};
use crate::transport::MessageIo;

pub trait Signer {
    fn sign(&self, transcript: &[u8]) -> Vec<u8>;
}

pub trait Verifier {
    fn verify(&self, transcript: &[u8], signature: &[u8]) -> bool;
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub version: u16,
    pub public_key: Vec<u8>,
    pub caps: u64,
    pub caps_required: u64,
    pub codec_caps: Vec<CodecCap>,
    pub actor_host: String,
    /// 客户端期望的服务端指纹；`None` 表示只依赖 TLS 证书校验。
    pub expected_host_fp: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub version: u16,
    pub host_fp: Vec<u8>,
    pub nonce: Vec<u8>,
    pub caps: u64,
    pub session_id: u64,
    pub token: Vec<u8>,
    /// 服务端可签发一张恢复票；`None` 表示本会话不支持恢复。
    pub resume_ticket: Option<ResumeTicket>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub version: u16,
    pub caps: u64,
    pub session_id: u64,
    pub token: Vec<u8>,
    /// Wall-clock time when the session token was issued, for TTL checks.
    pub token_issued_ms: u64,
    pub host_fp: Vec<u8>,
    pub public_key: Vec<u8>,
    pub codec_caps: Vec<CodecCap>,
    pub resume_ticket: Option<ResumeTicket>,
}

#[derive(Debug)]
pub enum HandshakeError {
    Io(io::Error),
    VersionMismatch,
    Rejected(String),
    SignatureRejected,
    FingerprintMismatch,
    ResumeRejected(ResumeError),
    ResumeDenied(u16),
    Protocol(&'static str),
}

impl From<io::Error> for HandshakeError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// 当前线格式的 ALPN 主版本；`aerlyn/4` / `aerlyn/4-tcp`。
pub const ALPN_MAJOR: u16 = 4;

fn require_32(field: &'static str, bytes: &[u8]) -> Result<(), HandshakeError> {
    if bytes.len() == 32 {
        Ok(())
    } else {
        Err(HandshakeError::Protocol(field))
    }
}

/// 签名原文（V3 §9.2）：
/// `nonce(32) ‖ u16be alpn_major ‖ u16be version ‖ host_fp(32)
///  ‖ u64be caps ‖ u64be caps_required ‖ sha256(caps_ext) ‖ public_key`。
pub fn transcript(
    nonce: &[u8],
    alpn_major: u16,
    version: u16,
    host_fp: &[u8],
    caps: u64,
    caps_required: u64,
    caps_ext: &[u8],
    public_key: &[u8],
) -> Vec<u8> {
    let ext_hash = Sha256::digest(caps_ext);
    let mut out = Vec::with_capacity(
        nonce.len() + 2 + 2 + host_fp.len() + 8 + 8 + ext_hash.len() + public_key.len(),
    );
    out.extend_from_slice(nonce);
    out.extend_from_slice(&alpn_major.to_be_bytes());
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(host_fp);
    out.extend_from_slice(&caps.to_be_bytes());
    out.extend_from_slice(&caps_required.to_be_bytes());
    out.extend_from_slice(&ext_hash);
    out.extend_from_slice(public_key);
    out
}

fn send_bye<C: MessageIo>(conn: &mut C, code: u16, reason: &str) -> io::Result<()> {
    channel::write_control_msg(
        conn,
        &ControlMessage::Bye(Bye {
            code,
            reason: reason.to_owned(),
        }),
    )
}

/// 握手 TLV 标签：1=wire version，2=协商能力位图。
pub const EXT_WIRE_VERSION: u16 = 1;
pub const EXT_CAPS: u16 = 2;

/// 生成握手扩展：`wire_version(u16)` 与 `caps(u64)`。
///
/// `Hello.caps_ext` 使用它的编码字节，`Hello.extensions` / `Welcome.extensions`
/// 使用解码后的同一组条目；这样签名转录不再对空字节串做常量哈希。
pub fn handshake_extensions(version: u16, caps: u64) -> Extensions {
    let mut ext = Extensions::new();
    ext.push(EXT_WIRE_VERSION, version.to_be_bytes().to_vec())
        .expect("fixed handshake extension");
    ext.push(EXT_CAPS, caps.to_be_bytes().to_vec())
        .expect("fixed handshake extension");
    ext
}

fn handshake_extension_items(version: u16, caps: u64) -> Vec<Extension> {
    handshake_extensions(version, caps).into_items()
}

pub fn client_handshake<C: MessageIo>(
    conn: &mut C,
    cfg: &ClientConfig,
    signer: &dyn Signer,
) -> Result<Session, HandshakeError> {
    let hello_ext = handshake_extensions(cfg.version, cfg.caps);
    let caps_ext = hello_ext
        .encode()
        .map_err(|_| HandshakeError::Protocol("handshake extensions too large"))?;
    channel::write_control_msg(
        conn,
        &ControlMessage::Hello(Hello {
            version: cfg.version,
            public_key: cfg.public_key.clone(),
            caps: cfg.caps,
            caps_required: cfg.caps_required,
            codec_caps: cfg.codec_caps.clone(),
            caps_ext,
            actor_host: cfg.actor_host.clone(),
            extensions: hello_ext.into_items(),
        }),
    )?;

    let challenge = match channel::read_control_msg_limit(conn, channel::MAX_HANDSHAKE_CONTROL_LEN)?
    {
        ControlMessage::Challenge(c) => c,
        ControlMessage::Bye(b) => return Err(HandshakeError::Rejected(b.reason)),
        _ => return Err(HandshakeError::Protocol("expected Challenge")),
    };
    if challenge.version != cfg.version {
        return Err(HandshakeError::Protocol("chosen version mismatch"));
    }
    require_32("challenge nonce must be 32 bytes", &challenge.nonce)?;
    require_32("challenge host_fp must be 32 bytes", &challenge.host_fp)?;
    if let Some(expected) = cfg.expected_host_fp.as_deref() {
        if expected != challenge.host_fp.as_slice() {
            let _ = send_bye(
                conn,
                crate::error::ErrorCode::HostFingerprintMismatch.code(),
                "host fingerprint mismatch",
            );
            return Err(HandshakeError::FingerprintMismatch);
        }
    }
    let t = transcript(
        &challenge.nonce,
        ALPN_MAJOR,
        challenge.version,
        &challenge.host_fp,
        challenge.caps,
        cfg.caps_required,
        &challenge.caps_ext,
        &cfg.public_key,
    );
    let signature = signer.sign(&t);
    channel::write_control_msg(conn, &ControlMessage::Auth(Auth { signature }))?;

    let welcome = match channel::read_control_msg_limit(conn, channel::MAX_HANDSHAKE_CONTROL_LEN)? {
        ControlMessage::Welcome(w) => w,
        ControlMessage::Bye(b) => return Err(HandshakeError::Rejected(b.reason)),
        _ => return Err(HandshakeError::Protocol("expected Welcome")),
    };
    if welcome.version != challenge.version || welcome.caps != challenge.caps {
        return Err(HandshakeError::Protocol("Welcome disagrees with Challenge"));
    }
    Ok(Session {
        version: welcome.version,
        caps: welcome.caps,
        session_id: welcome.session_id,
        token: welcome.token,
        token_issued_ms: crate::timeouts::unix_ms(),
        host_fp: challenge.host_fp,
        public_key: cfg.public_key.clone(),
        codec_caps: cfg.codec_caps.clone(),
        resume_ticket: welcome.resume_ticket,
    })
}

pub fn server_handshake<C: MessageIo>(
    conn: &mut C,
    cfg: &ServerConfig,
    verifier: &dyn Verifier,
) -> Result<Session, HandshakeError> {
    require_32("server nonce must be 32 bytes", &cfg.nonce)?;
    require_32("server host_fp must be 32 bytes", &cfg.host_fp)?;
    let hello = match channel::read_control_msg_limit(conn, channel::MAX_HANDSHAKE_CONTROL_LEN)? {
        ControlMessage::Hello(h) => h,
        _ => {
            let _ = send_bye(conn, 8, "expected Hello");
            return Err(HandshakeError::Protocol("expected Hello"));
        }
    };
    if hello.version != cfg.version {
        let _ = send_bye(conn, 1, "version mismatch");
        return Err(HandshakeError::VersionMismatch);
    }
    let version = hello.version;
    if hello.caps_required & !cfg.caps != 0 {
        let _ = send_bye(conn, 2, "required capability missing");
        return Err(HandshakeError::Rejected(
            "required capability missing".to_owned(),
        ));
    }
    let caps = hello.caps & cfg.caps;
    channel::write_control_msg(
        conn,
        &ControlMessage::Challenge(Challenge {
            nonce: cfg.nonce.clone(),
            host_fp: cfg.host_fp.clone(),
            version,
            caps,
            caps_ext: hello.caps_ext.clone(),
        }),
    )?;
    // 2-RTT: Challenge 与 Welcome 同一服务端航班发出。客户端拿到 Welcome 后
    // 立即发 Auth（可与首条业务消息同航班）；服务端先验 Auth 再读业务消息。
    // Auth 失败时补发 Bye，客户端在业务首包前会看到该 Bye。
    channel::write_control_msg(
        conn,
        &ControlMessage::Welcome(Welcome {
            version,
            caps,
            session_id: cfg.session_id,
            token: cfg.token.clone(),
            host_fp: cfg.host_fp.clone(),
            resume_ticket: cfg.resume_ticket,
            extensions: if hello.extensions.is_empty() {
                handshake_extension_items(version, caps)
            } else {
                hello.extensions.clone()
            },
        }),
    )?;

    let auth = match channel::read_control_msg_limit(conn, channel::MAX_HANDSHAKE_CONTROL_LEN)? {
        ControlMessage::Auth(a) => a,
        _ => {
            let _ = send_bye(conn, 8, "expected Auth");
            return Err(HandshakeError::Protocol("expected Auth"));
        }
    };
    let t = transcript(
        &cfg.nonce,
        ALPN_MAJOR,
        version,
        &cfg.host_fp,
        caps,
        hello.caps_required,
        &hello.caps_ext,
        &hello.public_key,
    );
    if !verifier.verify(&t, &auth.signature) {
        let _ = send_bye(conn, 12, "signature invalid");
        return Err(HandshakeError::SignatureRejected);
    }
    Ok(Session {
        version,
        caps,
        session_id: cfg.session_id,
        token: cfg.token.clone(),
        token_issued_ms: crate::timeouts::unix_ms(),
        host_fp: cfg.host_fp.clone(),
        public_key: hello.public_key,
        codec_caps: hello.codec_caps,
        resume_ticket: cfg.resume_ticket,
    })
}

fn resume_code(err: ResumeError) -> u16 {
    match err {
        ResumeError::Expired => crate::error::ErrorCode::ResumeExpired.code(),
        ResumeError::TicketMismatch => crate::error::ErrorCode::ResumeMetaMismatch.code(),
    }
}

/// TLS 后的恢复路径：客户端用上一会话的票据发 `Resume`，成功后拿到新票。
/// 失败返回 `ResumeDenied`，调用方应在同一连接上改走完整 `Hello`。
pub fn client_resume<C: MessageIo>(
    conn: &mut C,
    session: &Session,
    last_frame_id: u32,
    last_input_seq: u32,
) -> Result<Session, HandshakeError> {
    let Some(ticket) = session.resume_ticket else {
        return Err(HandshakeError::Protocol("session has no resume ticket"));
    };
    channel::write_control_msg(
        conn,
        &ControlMessage::Resume(Resume {
            ticket_id: ticket.id,
            nonce: ticket.nonce,
            last_frame_id,
            last_input_seq,
        }),
    )?;
    let result = match channel::read_control_msg_limit(conn, channel::MAX_HANDSHAKE_CONTROL_LEN)? {
        ControlMessage::ResumeResult(r) => r,
        ControlMessage::Bye(b) => return Err(HandshakeError::Rejected(b.reason)),
        _ => return Err(HandshakeError::Protocol("expected ResumeResult")),
    };
    if !result.ok {
        return Err(HandshakeError::ResumeDenied(result.code));
    }
    if result.session_id != session.session_id {
        return Err(HandshakeError::Protocol("resume changed session id"));
    }
    let mut resumed = session.clone();
    resumed.resume_ticket = result.ticket;
    Ok(resumed)
}

/// 服务端恢复路径：调用方提供上一会话快照；成功时必须换发新票。
pub fn server_resume<C: MessageIo>(
    conn: &mut C,
    session: &Session,
    now_ms: u64,
    next_ticket: Option<ResumeTicket>,
) -> Result<Session, HandshakeError> {
    server_resume_with_request(conn, session, now_ms, next_ticket).map(|(session, _)| session)
}

/// 与 [`server_resume`] 相同，但把客户端的 `Resume` 一并返回，供会话层校验
/// `last_frame_id` / `last_input_seq` 是否超前。
pub fn server_resume_with_request<C: MessageIo>(
    conn: &mut C,
    session: &Session,
    now_ms: u64,
    next_ticket: Option<ResumeTicket>,
) -> Result<(Session, Resume), HandshakeError> {
    server_resume_with_validator(conn, session, |resume| {
        let ticket = session.resume_ticket.ok_or(ResumeError::TicketMismatch)?;
        ticket.validate(resume, now_ms)?;
        Ok(next_ticket)
    })
}

/// 读取 `Resume` 后由调用方提供票据校验/换票逻辑。`TicketStore` 用它在
/// 登记表内原子赎回旧票；校验失败时回写 `ResumeResult{ok:false}`，旧票不消耗。
pub fn server_resume_with_validator<C, F>(
    conn: &mut C,
    session: &Session,
    validate: F,
) -> Result<(Session, Resume), HandshakeError>
where
    C: MessageIo,
    F: FnOnce(&Resume) -> Result<Option<ResumeTicket>, ResumeError>,
{
    let resume = match channel::read_control_msg_limit(conn, channel::MAX_HANDSHAKE_CONTROL_LEN)? {
        ControlMessage::Resume(r) => r,
        ControlMessage::Bye(b) => return Err(HandshakeError::Rejected(b.reason)),
        _ => {
            let _ = send_bye(conn, 8, "expected Resume");
            return Err(HandshakeError::Protocol("expected Resume"));
        }
    };
    let next_ticket = match validate(&resume) {
        Ok(ticket) => ticket,
        Err(err) => {
            let _ = channel::write_control_msg(
                conn,
                &ControlMessage::ResumeResult(ResumeResult {
                    ok: false,
                    code: resume_code(err),
                    need: 0,
                    ticket: None,
                    session_id: session.session_id,
                }),
            );
            return Err(HandshakeError::ResumeRejected(err));
        }
    };
    channel::write_control_msg(
        conn,
        &ControlMessage::ResumeResult(ResumeResult {
            ok: true,
            code: 0,
            need: 0,
            ticket: next_ticket,
            session_id: session.session_id,
        }),
    )?;
    let mut resumed = session.clone();
    resumed.resume_ticket = next_ticket;
    Ok((resumed, resume))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::pair;
    use crate::message::Bye;
    use crate::transport::MessageIo;
    use std::thread;

    struct FakeSigner;
    impl Signer for FakeSigner {
        fn sign(&self, _transcript: &[u8]) -> Vec<u8> {
            vec![0xAB; 64]
        }
    }

    struct FakeVerifier;
    impl Verifier for FakeVerifier {
        fn verify(&self, _transcript: &[u8], signature: &[u8]) -> bool {
            signature == vec![0xAB; 64]
        }
    }

    fn client_cfg() -> ClientConfig {
        ClientConfig {
            version: 400,
            public_key: vec![7; 32],
            caps: 0x3,
            caps_required: 0,
            codec_caps: vec![],
            actor_host: "client".into(),
            expected_host_fp: None,
        }
    }

    fn server_cfg() -> ServerConfig {
        ServerConfig {
            version: 400,
            host_fp: vec![9; 32],
            nonce: vec![1; 32],
            caps: 0x3,
            session_id: 42,
            token: vec![3; 32],
            resume_ticket: None,
        }
    }

    struct EchoSigner;
    impl Signer for EchoSigner {
        fn sign(&self, transcript: &[u8]) -> Vec<u8> {
            transcript.to_vec()
        }
    }

    struct EchoVerifier;
    impl Verifier for EchoVerifier {
        fn verify(&self, transcript: &[u8], signature: &[u8]) -> bool {
            signature == transcript
        }
    }

    #[test]
    fn transcript_covers_alpn_caps_required_and_ext_hash() {
        let nonce = [1u8; 32];
        let host_fp = [2u8; 32];
        let public_key = [3u8; 32];
        let caps_ext = b"caps-ext";
        let t = transcript(
            &nonce,
            ALPN_MAJOR,
            400,
            &host_fp,
            0x0f,
            0x03,
            caps_ext,
            &public_key,
        );
        assert_eq!(t.len(), 32 + 2 + 2 + 32 + 8 + 8 + 32 + 32);
        assert_eq!(&t[..32], &nonce);
        assert_eq!(u16::from_be_bytes([t[32], t[33]]), ALPN_MAJOR);
        assert_eq!(u16::from_be_bytes([t[34], t[35]]), 400);
        assert_eq!(&t[36..68], &host_fp);
        assert_eq!(u64::from_be_bytes(t[68..76].try_into().unwrap()), 0x0f);
        assert_eq!(u64::from_be_bytes(t[76..84].try_into().unwrap()), 0x03);
        assert_eq!(&t[84..116], Sha256::digest(caps_ext).as_slice());
        assert_eq!(&t[116..], &public_key);
    }

    #[test]
    fn handshake_extensions_are_non_empty_and_roundtrip() {
        let ext = handshake_extensions(400, 0x3);
        assert_eq!(ext.len(), 2);
        assert_eq!(ext.get(EXT_WIRE_VERSION), Some(&400u16.to_be_bytes()[..]));
        assert_eq!(ext.get(EXT_CAPS), Some(&0x3u64.to_be_bytes()[..]));
        let wire = ext.encode().unwrap();
        let (decoded, skipped) = Extensions::decode(&wire, &[EXT_WIRE_VERSION, EXT_CAPS]).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(decoded, ext);
        assert!(!wire.is_empty());
    }

    #[test]
    fn client_and_server_sign_same_extended_transcript() {
        let (client_conn, server_conn) = pair();
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_handshake(&mut conn, &server_cfg(), &EchoVerifier)
        });
        let mut conn = client_conn;
        let session = client_handshake(&mut conn, &client_cfg(), &EchoSigner).unwrap();
        assert_eq!(session.session_id, 42);
        server.join().unwrap().unwrap();
    }

    #[test]
    fn client_rejects_short_challenge_nonce() {
        let (mut client_conn, mut server_conn) = pair();
        let server = thread::spawn(move || {
            let _ = channel::read_control_msg(&mut server_conn).unwrap();
            channel::write_control_msg(
                &mut server_conn,
                &ControlMessage::Challenge(Challenge {
                    nonce: vec![0; 31],
                    host_fp: vec![9; 32],
                    version: 400,
                    caps: 0x3,
                    caps_ext: vec![],
                }),
            )
            .unwrap();
        });
        let err = client_handshake(&mut client_conn, &client_cfg(), &FakeSigner).unwrap_err();
        assert!(matches!(
            err,
            HandshakeError::Protocol("challenge nonce must be 32 bytes")
        ));
        server.join().unwrap();
    }

    #[test]
    fn server_rejects_short_nonce_before_hello() {
        let (_client_conn, mut server_conn) = pair();
        let mut cfg = server_cfg();
        cfg.nonce = vec![1; 31];
        let err = server_handshake(&mut server_conn, &cfg, &FakeVerifier).unwrap_err();
        assert!(matches!(
            err,
            HandshakeError::Protocol("server nonce must be 32 bytes")
        ));
    }

    #[test]
    fn handshake_succeeds() {
        let (client_conn, server_conn) = pair();
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_handshake(&mut conn, &server_cfg(), &FakeVerifier)
        });
        let mut conn = client_conn;
        let session = client_handshake(&mut conn, &client_cfg(), &FakeSigner).unwrap();
        assert_eq!(session.version, 400);
        assert_eq!(session.caps, 0x3);
        assert_eq!(session.session_id, 42);
        assert_eq!(session.token, vec![3; 32]);
        assert_eq!(session.host_fp, vec![9; 32]);
        let server_session = server.join().unwrap().unwrap();
        assert_eq!(server_session.session_id, 42);
        assert_eq!(server_session.public_key, vec![7; 32]);
    }

    #[test]
    fn single_version_is_not_negotiated() {
        let (client_conn, server_conn) = pair();
        let mut client_cfg = client_cfg();
        client_cfg.version = 500;
        let mut server_cfg = server_cfg();
        server_cfg.version = 500;
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_handshake(&mut conn, &server_cfg, &FakeVerifier)
        });
        let mut conn = client_conn;
        let session = client_handshake(&mut conn, &client_cfg, &FakeSigner).unwrap();
        assert_eq!(session.version, 500);
        assert_eq!(server.join().unwrap().unwrap().version, 500);
    }

    #[test]
    fn resume_ticket_is_delivered_on_both_sides() {
        let ticket = ResumeTicket {
            id: 11,
            nonce: [0x5A; 16],
            exp_ms: 123_456,
        };
        let mut server_cfg = server_cfg();
        server_cfg.resume_ticket = Some(ticket);
        let (client_conn, server_conn) = pair();
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_handshake(&mut conn, &server_cfg, &FakeVerifier)
        });
        let mut conn = client_conn;
        let session = client_handshake(&mut conn, &client_cfg(), &FakeSigner).unwrap();
        assert_eq!(session.resume_ticket, Some(ticket));
        let server_session = server.join().unwrap().unwrap();
        assert_eq!(server_session.resume_ticket, Some(ticket));
    }

    fn resumable_session(ticket: ResumeTicket) -> Session {
        Session {
            version: 400,
            caps: 0x3,
            session_id: 42,
            token: vec![3; 32],
            token_issued_ms: 0,
            host_fp: vec![9; 32],
            public_key: vec![7; 32],
            codec_caps: vec![],
            resume_ticket: Some(ticket),
        }
    }

    #[test]
    fn resume_rotates_the_ticket() {
        let old = ResumeTicket {
            id: 1,
            nonce: [1; 16],
            exp_ms: 10_000,
        };
        let next = ResumeTicket {
            id: 2,
            nonce: [2; 16],
            exp_ms: 20_000,
        };
        let session = resumable_session(old);
        let (client_conn, server_conn) = pair();
        let server_session = session.clone();
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_resume(&mut conn, &server_session, 1_000, Some(next))
        });
        let mut conn = client_conn;
        let resumed = client_resume(&mut conn, &session, 10, 3).unwrap();
        assert_eq!(resumed.resume_ticket, Some(next));
        let server_resumed = server.join().unwrap().unwrap();
        assert_eq!(server_resumed.resume_ticket, Some(next));
    }

    #[test]
    fn expired_resume_is_rejected_with_expired_code() {
        let old = ResumeTicket {
            id: 1,
            nonce: [1; 16],
            exp_ms: 1_000,
        };
        let session = resumable_session(old);
        let (client_conn, server_conn) = pair();
        let server_session = session.clone();
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_resume(&mut conn, &server_session, 1_000, None)
        });
        let mut conn = client_conn;
        let err = client_resume(&mut conn, &session, 0, 0).unwrap_err();
        assert!(matches!(err, HandshakeError::ResumeDenied(21)));
        assert!(matches!(
            server.join().unwrap(),
            Err(HandshakeError::ResumeRejected(ResumeError::Expired))
        ));
    }

    #[test]
    fn version_mismatch_rejected() {
        let (client_conn, server_conn) = pair();
        let mut server_cfg = server_cfg();
        server_cfg.version = 500;
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_handshake(&mut conn, &server_cfg, &FakeVerifier)
        });
        let mut conn = client_conn;
        let err = client_handshake(&mut conn, &client_cfg(), &FakeSigner).unwrap_err();
        assert!(matches!(err, HandshakeError::Rejected(_)));
        assert!(matches!(
            server.join().unwrap(),
            Err(HandshakeError::VersionMismatch)
        ));
    }

    #[test]
    fn host_fingerprint_mismatch_rejected() {
        let (client_conn, server_conn) = pair();
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_handshake(&mut conn, &server_cfg(), &FakeVerifier)
        });
        let mut cfg = client_cfg();
        cfg.expected_host_fp = Some(vec![0; 32]);
        let mut conn = client_conn;
        let err = client_handshake(&mut conn, &cfg, &FakeSigner).unwrap_err();
        assert!(matches!(err, HandshakeError::FingerprintMismatch));
        assert!(server.join().unwrap().is_err());
    }

    #[test]
    fn expected_host_fingerprint_is_accepted() {
        let (client_conn, server_conn) = pair();
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_handshake(&mut conn, &server_cfg(), &FakeVerifier)
        });
        let mut cfg = client_cfg();
        cfg.expected_host_fp = Some(vec![9; 32]);
        let mut conn = client_conn;
        let session = client_handshake(&mut conn, &cfg, &FakeSigner).unwrap();
        assert_eq!(session.host_fp, vec![9; 32]);
        assert!(server.join().unwrap().is_ok());
    }

    #[test]
    fn bad_signature_rejected() {
        struct RejectVerifier;
        impl Verifier for RejectVerifier {
            fn verify(&self, _transcript: &[u8], _signature: &[u8]) -> bool {
                false
            }
        }
        let (client_conn, server_conn) = pair();
        let server = thread::spawn(move || {
            let mut conn = server_conn;
            server_handshake(&mut conn, &server_cfg(), &RejectVerifier)
        });
        let mut conn = client_conn;
        // 2-RTT: 客户端先拿到 optimistic Welcome，随后才看到服务端 Auth 失败的 Bye。
        let _session = client_handshake(&mut conn, &client_cfg(), &FakeSigner).unwrap();
        let bye: ControlMessage = channel::read_control_msg(&mut conn).unwrap();
        assert!(matches!(bye, ControlMessage::Bye(Bye { code: 12, .. })));
        assert!(matches!(
            server.join().unwrap(),
            Err(HandshakeError::SignatureRejected)
        ));
    }

    #[test]
    fn unauthenticated_oversized_hello_is_rejected_before_parse() {
        let (mut client, mut server) = pair();
        let body = vec![b'x'; channel::MAX_HANDSHAKE_CONTROL_LEN + 1];
        let mut frame = Vec::with_capacity(4 + body.len());
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&body);
        client.send(&frame).unwrap();
        let err = server_handshake(&mut server, &server_cfg(), &FakeVerifier).unwrap_err();
        match err {
            HandshakeError::Io(e) => assert_eq!(e.kind(), io::ErrorKind::InvalidData),
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn bye_carries_reason() {
        let (mut a, mut b) = pair();
        send_bye(&mut a, 12, "signature invalid").unwrap();
        let msg: ControlMessage = channel::read_control_msg(&mut b).unwrap();
        assert_eq!(
            msg,
            ControlMessage::Bye(Bye {
                code: 12,
                reason: "signature invalid".into()
            })
        );
    }
}
