use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{SessionTimeouts, provider::VerifiedIdentity};
use crate::HttpError;

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
    fn expired(&self, now: Instant, timeouts: SessionTimeouts) -> bool {
        now.duration_since(self.created) >= timeouts.max_age
            || now.duration_since(self.last_active) >= timeouts.idle_timeout
    }
}

#[derive(Clone, Default)]
pub(super) struct Sessions {
    entries: Arc<Mutex<HashMap<String, Session>>>,
    pub(super) timeouts: SessionTimeouts,
}

impl Sessions {
    pub fn create(
        &self,
        method: &'static str,
        identity: VerifiedIdentity,
    ) -> Result<String, HttpError> {
        let now = Instant::now();
        let mut sessions = self.entries.lock().map_err(|_| {
            HttpError::internal(aow_i18n::text("server.login_session_lock_unavailable"))
        })?;
        self.prune(&mut sessions, now);
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
        let mut sessions = self.entries.lock().map_err(|_| {
            HttpError::internal(aow_i18n::text("server.login_session_lock_unavailable"))
        })?;
        self.prune(&mut sessions, now);
        Ok(sessions.get_mut(token).map(|session| {
            if touch {
                session.last_active = now;
            }
            session.clone()
        }))
    }

    pub fn revoke(&self, token: &str) -> Result<(), HttpError> {
        if let Some(session) = self
            .entries
            .lock()
            .map_err(|_| {
                HttpError::internal(aow_i18n::text("server.login_session_lock_unavailable"))
            })?
            .remove(token)
        {
            session.cancelled.cancel();
        }
        Ok(())
    }

    fn prune(&self, sessions: &mut HashMap<String, Session>, now: Instant) {
        sessions.retain(|_, session| {
            if session.expired(now, self.timeouts) {
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
    use std::time::Duration;

    use super::*;

    fn identity() -> VerifiedIdentity {
        VerifiedIdentity {
            subject: "user".into(),
            revision: "1".into(),
        }
    }

    #[test]
    fn absolute_and_idle_expiry_cancel_connections_and_cannot_be_refreshed() {
        for timeouts in [
            SessionTimeouts::default(),
            SessionTimeouts::from_seconds(10, 2).unwrap(),
            SessionTimeouts::from_seconds(86400, 3600).unwrap(),
        ] {
            for absolute in [false, true] {
                let sessions = Sessions {
                    timeouts,
                    ..Sessions::default()
                };
                let token = sessions.create("test", identity()).unwrap();
                let cancelled = sessions.get(&token, false).unwrap().unwrap().cancelled;
                {
                    let mut entries = sessions.entries.lock().unwrap();
                    let entry = entries.get_mut(&token).unwrap();
                    if absolute {
                        entry.created -= sessions.timeouts.max_age;
                    } else {
                        entry.last_active -= sessions.timeouts.idle_timeout;
                    }
                }
                assert!(sessions.get(&token, true).unwrap().is_none());
                assert!(cancelled.is_cancelled());
            }
        }
    }

    #[test]
    fn longer_limits_allow_sessions_past_the_defaults_and_activity_only_refreshes_idle() {
        let sessions = Sessions {
            timeouts: SessionTimeouts::from_seconds(86400, 7200).unwrap(),
            ..Sessions::default()
        };
        let token = sessions.create("test", identity()).unwrap();
        let created = Instant::now() - Duration::from_secs(13 * 60 * 60);
        let last_active = Instant::now() - Duration::from_secs(60 * 60);
        {
            let mut entries = sessions.entries.lock().unwrap();
            let entry = entries.get_mut(&token).unwrap();
            entry.created = created;
            entry.last_active = last_active;
        }
        let passive = sessions.get(&token, false).unwrap().unwrap();
        assert_eq!(passive.created, created);
        assert_eq!(passive.last_active, last_active);
        let active = sessions.get(&token, true).unwrap().unwrap();
        assert_eq!(active.created, created);
        assert!(active.last_active > last_active);
    }

    #[test]
    fn passive_checks_do_not_extend_idle_lifetime_and_store_is_bounded() {
        let sessions = Sessions::default();
        let token = sessions.create("test", identity()).unwrap();
        let before = Instant::now() - Duration::from_secs(60);
        sessions
            .entries
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
        assert_eq!(sessions.entries.lock().unwrap().len(), MAX_SESSIONS);
        let token = sessions.create("test", identity()).unwrap();
        let cancelled = sessions.get(&token, false).unwrap().unwrap().cancelled;
        sessions.revoke(&token).unwrap();
        assert!(cancelled.is_cancelled());
        assert!(sessions.get(&token, false).unwrap().is_none());
    }
}
