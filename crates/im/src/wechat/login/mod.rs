use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use qrcode::{QrCode, render::svg};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use super::{
    Credentials,
    api::{Api, BASE_URL, validate_base},
};

#[derive(Clone, Serialize)]
pub struct LoginView {
    pub id: String,
    pub status: String,
    pub qr_image: Option<String>,
    pub message: String,
}

#[derive(Clone)]
struct Login {
    view: LoginView,
    qrcode: String,
    base_url: String,
    started: Instant,
    previous: Option<Credentials>,
    polling: Arc<tokio::sync::Mutex<()>>,
}

pub struct LoginManager {
    api: Api,
    base_url: String,
    active: Mutex<Option<Login>>,
}

impl LoginManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: Api::new()?,
            base_url: BASE_URL.into(),
            active: Mutex::new(None),
        })
    }

    pub async fn start(&self, previous: Option<Credentials>) -> Result<LoginView> {
        let id = Uuid::new_v4().to_string();
        let tokens: Vec<_> = previous
            .iter()
            .map(|credentials| credentials.bot_token.clone())
            .collect();
        *self.active.lock().unwrap() = Some(Login {
            view: LoginView {
                id: id.clone(),
                status: "preparing".into(),
                qr_image: None,
                message: String::new(),
            },
            qrcode: String::new(),
            base_url: self.base_url.clone(),
            started: Instant::now(),
            previous,
            polling: Arc::new(tokio::sync::Mutex::new(())),
        });
        let response = self
            .api
            .post(
                &self.base_url,
                "/ilink/bot/get_bot_qrcode?bot_type=3",
                None,
                json!({"local_token_list":tokens}),
                Duration::from_secs(15),
            )
            .await?;
        let qrcode = response["qrcode"]
            .as_str()
            .filter(|code| !code.is_empty())
            .ok_or_else(|| anyhow!("微信未返回二维码"))?;
        let content = response["qrcode_img_content"]
            .as_str()
            .ok_or_else(|| anyhow!("微信未返回二维码内容"))?;
        // The upstream value is QR payload, NOT an image URL. Render locally;
        // never send a login QR to a third-party image-generation service.
        let svg = QrCode::new(content.as_bytes())
            .map_err(|_| anyhow!("微信二维码内容无效"))?
            .render::<svg::Color>()
            .min_dimensions(256, 256)
            .build();
        let mut active = self.active.lock().unwrap();
        let login = current(&mut active, &id)?;
        login.qrcode = qrcode.into();
        login.view.status = "wait".into();
        login.view.qr_image = Some(format!(
            "data:image/svg+xml;base64,{}",
            STANDARD.encode(svg)
        ));
        login.view.message = "请用手机微信扫码，并在手机上确认连接。".into();
        Ok(login.view.clone())
    }

    /// Poll outside the state lock. The commit callback runs under that lock,
    /// so an expired, cancelled or replaced QR can never save credentials later.
    pub async fn poll(
        &self,
        id: &str,
        verify_code: Option<&str>,
        accept: impl FnOnce(Credentials) -> Result<()>,
    ) -> Result<LoginView> {
        if let Some(code) = verify_code {
            ensure!(
                !code.is_empty()
                    && code.len() <= 16
                    && code.bytes().all(|byte| byte.is_ascii_digit()),
                "请填写手机微信显示的数字"
            );
        }
        let snapshot = {
            let mut active = self.active.lock().unwrap();
            current(&mut active, id)?.clone()
        };
        if matches!(
            snapshot.view.status.as_str(),
            "confirmed" | "expired" | "verify_code_blocked"
        ) {
            return Ok(snapshot.view);
        }
        ensure!(!snapshot.qrcode.is_empty(), "二维码尚未准备好");
        let _polling = snapshot
            .polling
            .try_lock()
            .map_err(|_| anyhow!("正在等待微信扫码状态，请稍后重试"))?;
        let response = self
            .api
            .qr_status(&snapshot.base_url, &snapshot.qrcode, verify_code)
            .await?;
        let mut active = self.active.lock().unwrap();
        let login = current(&mut active, id)?;
        let status = response["status"]
            .as_str()
            .ok_or_else(|| anyhow!("微信未返回扫码状态"))?;
        let message = match status {
            "wait" => "请用手机微信扫码。",
            "scaned" => "已扫码，请在手机微信上继续确认。",
            "need_verifycode" => {
                if verify_code.is_some() {
                    "数字不匹配，请重新输入手机微信显示的数字。"
                } else {
                    "请输入手机微信显示的数字，以继续连接。"
                }
            }
            "expired" => "二维码已过期，请重新生成。",
            "verify_code_blocked" => "验证次数过多，请稍后重新生成二维码。",
            "scaned_but_redirect" => {
                let host = response["redirect_host"]
                    .as_str()
                    .ok_or_else(|| anyhow!("微信未返回跳转地址"))?;
                login.base_url = validate_base(&format!("https://{host}"))?;
                "已扫码，正在继续验证。"
            }
            "confirmed" => {
                let field = |name| {
                    response[name]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| anyhow!("微信返回的绑定信息不完整"))
                };
                let credentials = Credentials {
                    account_id: field("ilink_bot_id")?,
                    user_id: field("ilink_user_id")?,
                    bot_token: field("bot_token")?,
                    base_url: validate_base(
                        response["baseurl"].as_str().unwrap_or(&login.base_url),
                    )?,
                    binding_id: Uuid::new_v4().to_string(),
                };
                credentials.validate()?;
                accept(credentials)?;
                "微信 Bot 已绑定，通知将发给本次扫码的微信账号。"
            }
            "binded_redirect" => {
                let previous = login.previous.clone().ok_or_else(|| {
                    anyhow!("微信提示已绑定，但本机没有对应凭证，请在微信解除旧连接后重新扫码")
                })?;
                ensure!(
                    response["ilink_bot_id"]
                        .as_str()
                        .is_none_or(|id| id == previous.account_id)
                        && response["ilink_user_id"]
                            .as_str()
                            .is_none_or(|id| id == previous.user_id),
                    "微信提示的绑定与本机账号不一致，请在微信解除旧连接后重新扫码"
                );
                accept(previous)?;
                "已恢复本机保存的微信绑定。"
            }
            _ => bail!("微信返回未知扫码状态，请重新生成二维码"),
        };
        login.view.status = if status == "binded_redirect" {
            "confirmed"
        } else {
            status
        }
        .into();
        login.view.message = message.into();
        if matches!(
            login.view.status.as_str(),
            "confirmed" | "expired" | "verify_code_blocked"
        ) {
            login.view.qr_image = None;
            login.previous = None;
        }
        Ok(login.view.clone())
    }

    pub fn cancel(&self, id: &str) {
        let mut active = self.active.lock().unwrap();
        if active.as_ref().is_some_and(|login| login.view.id == id) {
            *active = None;
        }
    }

    pub fn cancel_all(&self) {
        *self.active.lock().unwrap() = None;
    }
}

fn current<'a>(active: &'a mut Option<Login>, id: &str) -> Result<&'a mut Login> {
    let login = active
        .as_mut()
        .filter(|login| login.view.id == id)
        .ok_or_else(|| anyhow!("扫码已取消或被新的二维码替换"))?;
    if login.started.elapsed() >= Duration::from_secs(300) && login.view.status != "confirmed" {
        login.view.status = "expired".into();
        login.view.qr_image = None;
        login.previous = None;
        bail!("二维码已过期，请重新生成");
    }
    Ok(login)
}

#[cfg(test)]
mod tests;
