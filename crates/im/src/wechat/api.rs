use std::time::Duration;

use anyhow::{Result, anyhow, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{Client, Method, Url};
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) const BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const CHANNEL_VERSION: &str = "2.4.9";

#[derive(Clone)]
pub(super) struct Api {
    pub(super) http: Client,
}

impl Api {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(40))
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .build()?,
        })
    }

    pub(super) async fn post(
        &self,
        base: &str,
        path: &str,
        token: Option<&str>,
        mut body: Value,
        timeout: Duration,
    ) -> Result<Value> {
        if token.is_some() {
            body["base_info"] = json!({"channel_version":CHANNEL_VERSION, "bot_agent":concat!("AoW/", env!("CARGO_PKG_VERSION"))});
        }
        self.request(Method::POST, base, path, token, Some(body), &[], timeout)
            .await
    }

    pub(super) async fn qr_status(
        &self,
        base: &str,
        qrcode: &str,
        code: Option<&str>,
    ) -> Result<Value> {
        let mut query = vec![("qrcode", qrcode)];
        if let Some(code) = code {
            query.push(("verify_code", code));
        }
        self.request(
            Method::GET,
            base,
            "/ilink/bot/get_qrcode_status",
            None,
            None,
            &query,
            Duration::from_secs(35),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn request(
        &self,
        method: Method,
        base: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        query: &[(&str, &str)],
        timeout: Duration,
    ) -> Result<Value> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", base.trim_end_matches('/')))
            .header("iLink-App-Id", "bot")
            .header(
                "iLink-App-ClientVersion",
                ((2 << 16) | (4 << 8) | 9).to_string(),
            )
            .query(query)
            .timeout(timeout);
        if let Some(body) = body {
            let random = u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap());
            request = request
                .header("AuthorizationType", "ilink_bot_token")
                .header("X-WECHAT-UIN", STANDARD.encode(random.to_string()))
                .json(&body);
        }
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        // reqwest errors include query parameters. Never expose QR/verification codes,
        // upstream response bodies or credentials in logs/API errors.
        let mut response = request
            .send()
            .await
            .map_err(|_| anyhow!("微信网络请求失败或超时"))?;
        let status = response.status();
        ensure!(status.is_success(), "微信接口 HTTP {}", status.as_u16());
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow!("无法读取微信响应"))?
        {
            ensure!(bytes.len() + chunk.len() <= 2 * 1024 * 1024, "微信响应过大");
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| anyhow!("微信接口返回无效 JSON"))
    }
}

pub(super) fn validate_base(value: &str) -> Result<String> {
    let url = Url::parse(value).map_err(|_| anyhow!("微信返回了无效的服务地址"))?;
    let host = url.host_str().unwrap_or_default();
    ensure!(
        url.scheme() == "https"
            && (host == "ilinkai.weixin.qq.com" || host.ends_with(".weixin.qq.com"))
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && matches!(url.path(), "" | "/")
            && url.query().is_none()
            && url.fragment().is_none(),
        "微信返回了不受信任的服务地址"
    );
    Ok(url.origin().ascii_serialization())
}

pub(super) fn stale_context(data: &Value) -> bool {
    let ret = data["ret"].as_i64();
    let code = data["errcode"].as_i64();
    ret == Some(-14)
        || code == Some(-14)
        || ((ret == Some(-2) || code == Some(-2))
            && matches!(
                data["errmsg"]
                    .as_str()
                    .unwrap_or_default()
                    .to_lowercase()
                    .as_str(),
                "unknown error" | "prepare failed"
            ))
}

pub(super) fn check(data: &Value) -> Result<()> {
    ensure!(data.is_object(), "微信响应格式无效");
    for field in ["ret", "errcode"] {
        ensure!(
            data.get(field).is_none_or(|code| code.as_i64().is_some()),
            "微信响应状态码格式无效（{field}）"
        );
    }
    // iLink status fields are optional: successful getupdates/sendmessage
    // responses may omit both, as handled by Tencent's client and Hermes.
    let ret = data["ret"].as_i64();
    let code = data["errcode"].as_i64();
    if ret.is_some_and(|code| code != 0) || code.is_some_and(|code| code != 0) {
        if stale_context(data) {
            bail!("微信会话尚未就绪或已失效，请先向 Bot 发一条消息；仍失败时重新扫码绑定");
        }
        bail!(
            "微信接口错误（ret={}，errcode={}）",
            ret.unwrap_or(0),
            code.unwrap_or(0)
        );
    }
    Ok(())
}
