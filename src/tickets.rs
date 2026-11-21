//! 服务端恢复票登记表。
//!
//! 单张 `ResumeTicket` 只描述票据本身；服务端还需要记录它属于哪个会话、是否已用、
//! 何时过期。这里故意不生成随机 nonce：调用方提供 16 字节 nonce，便于测试和替换
//! 密码学随机源。

use std::collections::BTreeMap;

use crate::control::{Resume, ResumeError, ResumeTicket};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketError {
    Unknown,
    SessionMismatch,
    NonceMismatch,
    Expired,
    AlreadyUsed,
}

impl From<TicketError> for ResumeError {
    fn from(error: TicketError) -> Self {
        match error {
            TicketError::Expired => ResumeError::Expired,
            TicketError::Unknown
            | TicketError::SessionMismatch
            | TicketError::NonceMismatch
            | TicketError::AlreadyUsed => ResumeError::TicketMismatch,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct StoredTicket {
    session_id: u64,
    ticket: ResumeTicket,
    used: bool,
}

#[derive(Debug, Default)]
pub struct TicketStore {
    next_id: u64,
    tickets: BTreeMap<u64, StoredTicket>,
}

impl TicketStore {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            tickets: BTreeMap::new(),
        }
    }

    /// 签发一张票；`nonce` 由调用方的 CSPRNG 提供。
    ///
    /// 签发前顺手清掉已过期的票。已使用但未过期的票会保留，用来把重放
    /// 区分为 `AlreadyUsed` 而不是 `Unknown`。
    pub fn issue(&mut self, session_id: u64, now_ms: u64, nonce: [u8; 16]) -> ResumeTicket {
        self.prune_expired(now_ms);
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let ticket = ResumeTicket {
            id,
            nonce,
            exp_ms: now_ms.saturating_add(ResumeTicket::TTL_MS),
        };
        self.tickets.insert(
            id,
            StoredTicket {
                session_id,
                ticket,
                used: false,
            },
        );
        ticket
    }

    /// 登记外部生成的票；同 id 已存在时返回 `None`，避免静默覆盖。
    pub fn register(&mut self, session_id: u64, ticket: ResumeTicket) -> Option<()> {
        if self.tickets.contains_key(&ticket.id) {
            return None;
        }
        self.next_id = self.next_id.max(ticket.id.saturating_add(1));
        self.tickets.insert(
            ticket.id,
            StoredTicket {
                session_id,
                ticket,
                used: false,
            },
        );
        Some(())
    }

    /// 赎回旧票并原子换发新票。失败时不消耗旧票，除非错误是 `AlreadyUsed`。
    pub fn redeem(
        &mut self,
        session_id: u64,
        resume: &Resume,
        now_ms: u64,
        next_nonce: [u8; 16],
    ) -> Result<ResumeTicket, TicketError> {
        let stored = self
            .tickets
            .get(&resume.ticket_id)
            .copied()
            .ok_or(TicketError::Unknown)?;
        if stored.session_id != session_id {
            return Err(TicketError::SessionMismatch);
        }
        if stored.ticket.nonce != resume.nonce {
            return Err(TicketError::NonceMismatch);
        }
        if now_ms >= stored.ticket.exp_ms {
            return Err(TicketError::Expired);
        }
        if stored.used {
            return Err(TicketError::AlreadyUsed);
        }
        let new = self.issue(session_id, now_ms, next_nonce);
        if let Some(entry) = self.tickets.get_mut(&resume.ticket_id) {
            entry.used = true;
        }
        Ok(new)
    }

    /// 只删除已过期的票，保留已使用但仍在 TTL 内的票用于重放识别。
    /// 返回删除数量。
    pub fn prune_expired(&mut self, now_ms: u64) -> usize {
        let before = self.tickets.len();
        self.tickets
            .retain(|_, stored| now_ms < stored.ticket.exp_ms);
        before - self.tickets.len()
    }

    /// 删除已过期或已使用的票，避免登记表无限增长。
    pub fn prune(&mut self, now_ms: u64) {
        self.tickets
            .retain(|_, stored| !stored.used && now_ms < stored.ticket.exp_ms);
    }

    pub fn len(&self) -> usize {
        self.tickets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resume(ticket: ResumeTicket) -> Resume {
        Resume {
            ticket_id: ticket.id,
            nonce: ticket.nonce,
            last_frame_id: 7,
            last_input_seq: 9,
        }
    }

    #[test]
    fn issue_and_redeem_rotate_once() {
        let mut store = TicketStore::new();
        let first = store.issue(42, 1_000, [1; 16]);
        let next = store.redeem(42, &resume(first), 1_100, [2; 16]).unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(next.id, 2);
        assert_eq!(next.nonce, [2; 16]);
        assert_eq!(store.len(), 2);
        assert_eq!(
            store.redeem(42, &resume(first), 1_200, [3; 16]),
            Err(TicketError::AlreadyUsed)
        );
    }

    #[test]
    fn wrong_session_and_nonce_do_not_consume() {
        let mut store = TicketStore::new();
        let ticket = store.issue(42, 1_000, [1; 16]);
        assert_eq!(
            store.redeem(43, &resume(ticket), 1_100, [2; 16]),
            Err(TicketError::SessionMismatch)
        );
        let mut bad = resume(ticket);
        bad.nonce = [9; 16];
        assert_eq!(
            store.redeem(42, &bad, 1_100, [2; 16]),
            Err(TicketError::NonceMismatch)
        );
        assert!(store.redeem(42, &resume(ticket), 1_100, [2; 16]).is_ok());
    }

    #[test]
    fn expired_is_not_redeemable() {
        let mut store = TicketStore::new();
        let ticket = store.issue(42, 1_000, [1; 16]);
        assert_eq!(
            store.redeem(42, &resume(ticket), 1_000 + ResumeTicket::TTL_MS, [2; 16]),
            Err(TicketError::Expired)
        );
    }

    #[test]
    fn prune_drops_used_and_expired() {
        let mut store = TicketStore::new();
        let a = store.issue(42, 1_000, [1; 16]);
        let b = store.issue(42, 1_000, [2; 16]);
        store.redeem(42, &resume(a), 1_100, [3; 16]).unwrap();
        store.prune(1_100);
        assert_eq!(store.len(), 2); // b + 新票
        store.prune(1_100 + ResumeTicket::TTL_MS);
        assert!(store.is_empty());
        let _ = b;
    }

    #[test]
    fn prune_expired_keeps_used_ticket_within_ttl() {
        let mut store = TicketStore::new();
        let a = store.issue(42, 1_000, [1; 16]);
        let _b = store.issue(42, 1_000, [2; 16]);
        store.redeem(42, &resume(a), 1_100, [3; 16]).unwrap();
        assert_eq!(store.len(), 3);

        assert_eq!(store.prune_expired(1_100), 0);
        assert_eq!(store.len(), 3);
        assert_eq!(
            store.redeem(42, &resume(a), 1_200, [4; 16]),
            Err(TicketError::AlreadyUsed)
        );

        assert_eq!(store.prune_expired(1_000 + ResumeTicket::TTL_MS + 1), 2);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn issue_auto_prunes_expired_tickets() {
        let mut store = TicketStore::new();
        store.issue(42, 1_000, [1; 16]);
        assert_eq!(store.len(), 1);
        store.issue(42, 1_000 + ResumeTicket::TTL_MS + 1, [2; 16]);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn duplicate_register_is_rejected() {
        let mut store = TicketStore::new();
        let ticket = ResumeTicket {
            id: 7,
            nonce: [7; 16],
            exp_ms: 9_000,
        };
        assert_eq!(store.register(1, ticket), Some(()));
        assert_eq!(store.register(1, ticket), None);
    }
}
