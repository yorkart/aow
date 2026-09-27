//! Bound anonymous work before submitting password verification to the blocking pool.
//! A single local account uses a shared budget; client-supplied usernames and
//! forwarded IP headers cannot create new buckets or bypass the limit.
use crate::HttpError;
use axum::http::{HeaderValue, StatusCode, header::RETRY_AFTER};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const BURST: f64 = 10.0;
const REFILL_SECONDS: f64 = 6.0;
const MAX_CONCURRENT: usize = 2;

#[derive(Clone)]
pub(super) struct LoginLimiter(Arc<Mutex<State>>);
struct State {
    active: usize,
    tokens: f64,
    updated: Instant,
    failures: u32,
    blocked_until: Instant,
}

impl Default for LoginLimiter {
    fn default() -> Self {
        let now = Instant::now();
        Self(Arc::new(Mutex::new(State {
            active: 0,
            tokens: BURST,
            updated: now,
            failures: 0,
            blocked_until: now,
        })))
    }
}

pub(super) struct Attempt {
    limiter: LoginLimiter,
}

impl LoginLimiter {
    pub fn begin(&self) -> Result<Attempt, HttpError> {
        self.begin_at(Instant::now())
    }

    fn begin_at(&self, now: Instant) -> Result<Attempt, HttpError> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| HttpError::internal("登录限流不可用"))?;
        state.tokens = (state.tokens
            + now.duration_since(state.updated).as_secs_f64() / REFILL_SECONDS)
            .min(BURST);
        state.updated = now;
        let wait = if now < state.blocked_until {
            (state.blocked_until - now).as_secs_f64()
        } else if state.active >= MAX_CONCURRENT {
            1.0
        } else if state.tokens < 1.0 {
            (1.0 - state.tokens) * REFILL_SECONDS
        } else {
            0.0
        };
        if wait > 0.0 {
            return Err(limited(wait.ceil() as u64));
        }
        state.tokens -= 1.0;
        state.active += 1;
        Ok(Attempt {
            limiter: self.clone(),
        })
    }
}

impl Attempt {
    pub fn finish(&self, success: bool, bad_credentials: bool) {
        self.finish_at(success, bad_credentials, Instant::now());
    }
    fn finish_at(&self, success: bool, bad_credentials: bool, now: Instant) {
        if let Ok(mut state) = self.limiter.0.lock() {
            if success {
                state.failures = 0;
                state.blocked_until = now;
            } else if bad_credentials {
                state.failures = state.failures.saturating_add(1);
                if state.failures >= 5 {
                    state.blocked_until =
                        now + Duration::from_secs((1_u64 << (state.failures - 5).min(6)).min(60));
                }
            }
        }
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if let Ok(mut state) = self.limiter.0.lock() {
            state.active -= 1;
        }
    }
}
fn limited(seconds: u64) -> HttpError {
    let mut error = HttpError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "login_rate_limited",
        "登录尝试过于频繁，请稍后重试",
        None,
    );
    error.extra_headers.insert(
        RETRY_AFTER,
        HeaderValue::from_str(&seconds.max(1).to_string()).unwrap(),
    );
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrency_and_rate_budgets_recover_without_an_unbounded_queue() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        let first = limiter.begin_at(now).unwrap();
        let second = limiter.begin_at(now).unwrap();
        assert_eq!(
            limiter.begin_at(now).err().unwrap().status,
            StatusCode::TOO_MANY_REQUESTS
        );
        drop(first);
        drop(second);
        for _ in 2..10 {
            drop(limiter.begin_at(now).unwrap());
        }
        let error = limiter.begin_at(now).err().unwrap();
        assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.extra_headers[RETRY_AFTER], "6");
        drop(limiter.begin_at(now + Duration::from_secs(6)).unwrap());
    }
    #[test]
    fn repeated_failures_back_off_and_rejections_do_not_extend_lockout() {
        let limiter = LoginLimiter::default();
        let now = Instant::now();
        for _ in 0..5 {
            let attempt = limiter.begin_at(now).unwrap();
            attempt.finish_at(false, true, now);
        }
        assert_eq!(
            limiter.begin_at(now).err().unwrap().extra_headers[RETRY_AFTER],
            "1"
        );
        let later = now + Duration::from_secs(1);
        let attempt = limiter.begin_at(later).unwrap();
        attempt.finish_at(false, true, later);
        drop(attempt);
        assert_eq!(
            limiter.begin_at(later).err().unwrap().extra_headers[RETRY_AFTER],
            "2"
        );
        let later = later + Duration::from_secs(2);
        let attempt = limiter.begin_at(later).unwrap();
        attempt.finish_at(true, false, later);
        drop(attempt);
        assert!(limiter.begin_at(later).is_ok());
    }
}
