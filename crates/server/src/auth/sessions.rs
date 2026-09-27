use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::provider::VerifiedIdentity;
use crate::HttpError;

pub(super) const ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MAX_SESSIONS: usize = 64;

#[derive(Clone)]
pub(super) struct Session {
    pub method: &'static str,
    pub identity: VerifiedIdentity,
    pub cancelled: CancellationToken,
    created: Instant,
    last_active: Instant,
}

impl Session {
    fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.created) >= ABSOLUTE_TIMEOUT
            || now.duration_since(self.last_active) >= IDLE_TIMEOUT
    }
}

#[derive(Clone, Default)]
pub(super) struct Sessions(Arc<Mutex<HashMap<String, Session>>>);

impl Sessions {
    pub fn create(
        &self,
        method: &'static str,
        identity: VerifiedIdentity,
    ) -> Result<String, HttpError> {
        let now = Instant::now();
        let mut sessions = self
            .0
            .lock()
            .map_err(|_| HttpError::internal("登录会话锁不可用"))?;
        Self::prune(&mut sessions, now);
        if sessions.len() >= MAX_SESSIONS {
            let oldest = sessions
                .iter()
                .min_by_key(|(_, session)| session.created)
                .map(|(token, _)| token.clone())
                .expect("nonempty session store");
            if let Some(session) = sessions.remove(&oldest) {
                session.cancelled.cancel();
            }
        }
        let token = Uuid::new_v4().simple().to_string();
        sessions.insert(
            token.clone(),
            Session {
                method,
                identity,
                cancelled: CancellationToken::new(),
                created: now,
                last_active: now,
            },
        );
        Ok(token)
    }

    pub fn get(&self, token: &str, touch: bool) -> Result<Option<Session>, HttpError> {
        let now = Instant::now();
        let mut sessions = self
            .0
            .lock()
            .map_err(|_| HttpError::internal("登录会话锁不可用"))?;
        Self::prune(&mut sessions, now);
        Ok(sessions.get_mut(token).map(|session| {
            if touch {
                session.last_active = now;
            }
            session.clone()
        }))
    }

    pub fn revoke(&self, token: &str) -> Result<(), HttpError> {
        if let Some(session) = self
            .0
            .lock()
            .map_err(|_| HttpError::internal("登录会话锁不可用"))?
            .remove(token)
        {
            session.cancelled.cancel();
        }
        Ok(())
    }

    fn prune(sessions: &mut HashMap<String, Session>, now: Instant) {
        sessions.retain(|_, session| {
            if session.expired(now) {
                session.cancelled.cancel();
                false
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> VerifiedIdentity {
        VerifiedIdentity {
            subject: "user".into(),
            revision: "1".into(),
        }
    }

    #[test]
    fn absolute_and_idle_expiry_cancel_connections_and_cannot_be_refreshed() {
        for absolute in [false, true] {
            let sessions = Sessions::default();
            let token = sessions.create("test", identity()).unwrap();
            let cancelled = sessions.get(&token, false).unwrap().unwrap().cancelled;
            {
                let mut entries = sessions.0.lock().unwrap();
                let entry = entries.get_mut(&token).unwrap();
                if absolute {
                    entry.created -= ABSOLUTE_TIMEOUT;
                } else {
                    entry.last_active -= IDLE_TIMEOUT;
                }
            }
            assert!(sessions.get(&token, true).unwrap().is_none());
            assert!(cancelled.is_cancelled());
        }
    }

    #[test]
    fn passive_checks_do_not_extend_idle_lifetime_and_store_is_bounded() {
        let sessions = Sessions::default();
        let token = sessions.create("test", identity()).unwrap();
        let before = Instant::now() - Duration::from_secs(60);
        sessions
            .0
            .lock()
            .unwrap()
            .get_mut(&token)
            .unwrap()
            .last_active = before;
        assert_eq!(
            sessions.get(&token, false).unwrap().unwrap().last_active,
            before
        );
        assert!(sessions.get(&token, true).unwrap().unwrap().last_active > before);
        let cancelled = sessions.get(&token, false).unwrap().unwrap().cancelled;
        for _ in 0..MAX_SESSIONS {
            sessions.create("test", identity()).unwrap();
        }
        assert!(cancelled.is_cancelled());
        assert_eq!(sessions.0.lock().unwrap().len(), MAX_SESSIONS);
        let token = sessions.create("test", identity()).unwrap();
        let cancelled = sessions.get(&token, false).unwrap().unwrap().cancelled;
        sessions.revoke(&token).unwrap();
        assert!(cancelled.is_cancelled());
        assert!(sessions.get(&token, false).unwrap().is_none());
    }
}
