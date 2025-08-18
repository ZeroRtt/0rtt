use std::{cell::UnsafeCell, collections::HashMap, time::Duration};

use quiche::{Connection, ConnectionId};

use crate::{
    Error, Readiness, Result, Token,
    conn::{ConnGuard, ConnState, LocKind},
};

#[allow(unused)]
#[derive(Default)]
pub struct Registration {
    /// Id generator for local connection id.
    conn_id_next: u32,
    /// Source connection id set.
    scids: HashMap<ConnectionId<'static>, Token>,
    /// registered connection states
    conn_stats: HashMap<Token, ConnState>,
    /// Unsafe code to get a mutable reference.
    readiness: UnsafeCell<Readiness>,
}

#[allow(unused)]
impl Registration {
    /// Register and host `quiche::Connection`
    #[inline(always)]
    pub fn register(
        &mut self,
        conn: Connection,
        release_timer_threshold: Duration,
    ) -> Result<Token> {
        let readiness = unsafe { self.readiness() };

        loop {
            let token = Token(self.conn_id_next);

            (self.conn_id_next, _) = self.conn_id_next.overflowing_add(1);

            if self.conn_stats.contains_key(&token) {
                continue;
            }

            assert!(
                self.scids
                    .insert(conn.source_id().into_owned(), token)
                    .is_none()
            );

            self.conn_stats.insert(
                token,
                ConnState::new_with_readiness(token, conn, release_timer_threshold, readiness),
            );

            return Ok(token);
        }
    }

    /// Deregister a hosted connection.
    #[inline(always)]
    pub fn deregister(&mut self, token: Token) -> Result<Connection> {
        if let Some(state) = self.conn_stats.remove(&token) {
            let conn: Connection = state.into();

            assert_eq!(
                self.scids.remove(&conn.source_id().into_owned()),
                Some(token),
                "scid mismatch."
            );

            Ok(conn)
        } else {
            Err(Error::NotFound)
        }
    }

    /// Carefully: use this function within the scope of spin-lock protection.
    #[inline(always)]
    pub unsafe fn readiness(&mut self) -> &'static mut Readiness {
        unsafe { self.readiness.get().as_mut().unwrap() }
    }

    /// Try lock one connection.
    #[inline(always)]
    pub fn try_lock_conn(&mut self, token: Token, kind: LocKind) -> Result<ConnGuard> {
        if let Some(state) = self.conn_stats.get_mut(&token) {
            state.try_lock(kind)
        } else {
            Err(Error::NotFound)
        }
    }

    /// Try lock one connection by `source_id`.
    #[inline(always)]
    pub fn try_lock_conn_by(
        &mut self,
        scid: &ConnectionId<'_>,
        kind: LocKind,
    ) -> Result<(Token, ConnGuard)> {
        if let Some(token) = self.scids.get(scid).cloned() {
            if let Some(state) = self.conn_stats.get_mut(&token) {
                return state.try_lock(kind).map(|guard| (token, guard));
            }
        }

        Err(Error::NotFound)
    }

    /// Try open a outbound stream.
    pub fn stream_open(&mut self, token: Token, release_timer_threshold: Duration) -> Result<u64> {
        let readiness = unsafe { self.readiness() };

        if let Some(state) = self.conn_stats.get_mut(&token) {
            state.stream_open(release_timer_threshold, readiness)
        } else {
            Err(Error::NotFound)
        }
    }

    /// Unlock connection.
    ///
    /// Carefully: use this function within the scope of spin-lock protection.
    #[inline(always)]
    pub unsafe fn unlock_conn(
        &mut self,
        release_timer_threshold: Duration,
        token: Token,
        lock_count: u64,
    ) -> Result<()> {
        let readiness = unsafe { self.readiness() };

        if let Some(state) = self.conn_stats.get_mut(&token) {
            state.unlock(lock_count, release_timer_threshold, readiness);
            return Ok(());
        }

        Err(Error::NotFound)
    }
}
