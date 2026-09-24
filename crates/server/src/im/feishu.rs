use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::ImProvider;
use crate::notifications::AutomationFailureNotification;
use crate::terminal::notifications::TaskStopNotification;

mod cards;

const BASE_URL: &str = "https://open.feishu.cn/open-apis";
const TOKEN_PATH: &str = "/auth/v3/tenant_access_token/internal";
const APP_PATH: &str = "/application/v6/applications/me?lang=zh_cn&user_id_type=open_id";
const MESSAGE_PATH: &str = "/im/v1/messages?receive_id_type=open_id";

struct CachedToken {
    value: String,
    refresh_at: Instant,
}

pub(crate) struct FeishuClient {
    http: Client,
    base_url: String,
    app_id: String,
    app_secret: String,
    token: Mutex<Option<CachedToken>>,
    next_delivery: Mutex<Instant>,
}

impl FeishuClient {
    pub(super) fn new(app_id: String, app_secret: String) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .build()
                .context("无法初始化飞书 HTTP 客户端")?,
            base_url: BASE_URL.to_owned(),
            app_id,
            app_secret,
            token: Mutex::new(None),
            next_delivery: Mutex::new(Instant::now()),
        })
    }

    async fn token(&self) -> Result<String> {
        // Hold the lock across the request: concurrent deliveries share one refresh.
        let mut cached = self.token.lock().await;
        if let Some(token) = cached.as_ref()
            && Instant::now() < token.refresh_at
        {
            return Ok(token.value.clone());
        }
        let started = Instant::now();
        let response = self
            .request(
                Method::POST,
                TOKEN_PATH,
                None,
                Some(&json!({
                    "app_id": self.app_id, "app_secret": self.app_secret,
                })),
            )
            .await?;
        check_code(&response)?;
        let value = response["tenant_access_token"]
            .as_str()
            .filter(|token| !token.is_empty())
            .context("飞书未返回 tenant_access_token")?
            .to_owned();
        let seconds = response["expire"]
            .as_u64()
            .filter(|seconds| *seconds > 0 && *seconds <= 7200)
            .context("飞书返回了无效的 token 有效期")?;
        let margin = 60.min(seconds / 10);
        *cached = Some(CachedToken {
            value: value.clone(),
            refresh_at: started + Duration::from_secs(seconds - margin),
        });
        Ok(value)
    }

    async fn invalidate_token(&self, rejected: &str) {
        let mut cached = self.token.lock().await;
        if cached.as_ref().is_some_and(|token| token.value == rejected) {
            *cached = None;
        }
    }

    async fn authenticated(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        for attempt in 0..2 {
            let token = self.token().await?;
            let response = self
                .request(method.clone(), path, Some(&token), body)
                .await?;
            // Only retry when Feishu explicitly rejected the token, never on an
            // ambiguous timeout that could have already delivered the message.
            if matches!(response["code"].as_i64(), Some(99991663 | 99991665)) && attempt == 0 {
                self.invalidate_token(&token).await;
                continue;
            }
            check_code(&response)?;
            return Ok(response);
        }
        unreachable!()
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<&Value>,
    ) -> Result<Value> {
        for attempt in 0..2 {
            let mut request = self
                .http
                .request(method.clone(), format!("{}{path}", self.base_url));
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request.send().await.context("飞书网络请求失败")?;
            let status = response.status();
            let delay = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1);
            let bytes = response.bytes().await.context("无法读取飞书响应")?;
            let data = serde_json::from_slice::<Value>(&bytes).ok();
            let limited = status == StatusCode::TOO_MANY_REQUESTS
                || data
                    .as_ref()
                    .is_some_and(|data| matches!(data["code"].as_i64(), Some(99991400 | 230020)));
            if limited {
                if attempt == 0 && delay <= 5 {
                    tokio::time::sleep(Duration::from_secs(delay.max(1))).await;
                    continue;
                }
                bail!("飞书请求受到限流");
            }
            // Feishu may return token errors with HTTP 401/400. Preserve the code
            // for the authentication layer; never log the response body or secrets.
            if let Some(data) = data
                && data["code"].as_i64().is_some()
            {
                if status.is_success() || data["code"].as_i64() != Some(0) {
                    return Ok(data);
                }
            }
            bail!("飞书响应无效（HTTP {}）", status.as_u16());
        }
        unreachable!()
    }

    async fn owner(&self) -> Result<String> {
        let response = self.authenticated(Method::GET, APP_PATH, None).await?;
        let app = &response["data"]["app"];
        if app["owner"]["type"].as_u64() != Some(2) {
            bail!("飞书应用 owner 不是企业内部成员，请配置企业自建应用机器人");
        }
        app["owner"]["owner_id"]
            .as_str()
            .filter(|id| id.starts_with("ou_") && id.len() > 3)
            .map(str::to_owned)
            .context("飞书未返回有效的 owner Open ID")
    }
    async fn send_cards(&self, build: impl FnOnce(&str) -> Result<Vec<Value>>) -> Result<()> {
        // Serialize whole notifications so their parts stay in order. Pace each
        // card below the recipient's 5 QPS limit, including multipart messages.
        let mut next = self.next_delivery.lock().await;
        // Resolve the current owner per delivery, so transferring app ownership
        // never leaves a long-lived cached recipient pointing at the old owner.
        let owner = self.owner().await?;
        let messages = build(&owner)?;
        let total = messages.len();
        for (index, message) in messages.into_iter().enumerate() {
            if let Some(delay) = next.checked_duration_since(Instant::now()) {
                tokio::time::sleep(delay).await;
            }
            *next = Instant::now() + Duration::from_millis(250);
            self.authenticated(Method::POST, MESSAGE_PATH, Some(&message))
                .await
                .with_context(|| format!("飞书卡片发送失败（{}/{total}）", index + 1))?;
        }
        Ok(())
    }
}

impl ImProvider for FeishuClient {
    async fn send_notification(
        &self,
        event: &TaskStopNotification,
        delivery_id: &str,
    ) -> Result<()> {
        self.send_cards(|owner| cards::messages(event, owner, delivery_id))
            .await
    }

    async fn send_automation_failure(
        &self,
        event: &AutomationFailureNotification,
        delivery_id: &str,
    ) -> Result<()> {
        self.send_cards(|owner| Ok(vec![cards::automation_failure(event, owner, delivery_id)?]))
            .await
    }
}

fn check_code(response: &Value) -> Result<()> {
    match response["code"].as_i64() {
        Some(0) => Ok(()),
        Some(code) => {
            let hint = match code {
                99991672 | 99991676 => {
                    "请检查 application:application:self_manage 和 im:message:send_as_bot 权限"
                }
                230013 => "请确认 owner 在应用可用范围内",
                _ => "请检查飞书应用配置及权限",
            };
            Err(anyhow!("飞书接口错误 {code}：{hint}"))
        }
        None => Err(anyhow!("飞书响应缺少状态码")),
    }
}

#[cfg(test)]
mod tests;
