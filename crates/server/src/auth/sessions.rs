use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use uuid::Uuid;

use super::provider::VerifiedIdentity;
use crate::HttpError;

#[derive(Clone)]
pub(super) struct Session {
    pub method: &'static str,
    pub identity: VerifiedIdentity,
}

#[derive(Clone, Default)]
pub(super) struct Sessions(Arc<Mutex<HashMap<String, Session>>>);

impl Sessions {
    pub fn create(
        &self,
        method: &'static str,
        identity: VerifiedIdentity,
    ) -> Result<String, HttpError> {
        let token = Uuid::new_v4().simple().to_string();
        self.0
            .lock()
            .map_err(|_| HttpError::internal("登录会话锁不可用"))?
            .insert(token.clone(), Session { method, identity });
        Ok(token)
    }

    pub fn get(&self, token: &str) -> Result<Option<Session>, HttpError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| HttpError::internal("登录会话锁不可用"))?
            .get(token)
            .cloned())
    }

    pub fn revoke(&self, token: &str) -> Result<(), HttpError> {
        self.0
            .lock()
            .map_err(|_| HttpError::internal("登录会话锁不可用"))?
            .remove(token);
        Ok(())
    }
}
