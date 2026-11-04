use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use qrcode::{QrCode, render::svg};
use serde::Serialize;
use serde_json::json;

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
        let id = aow_id::new_id();
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
            .ok_or_else(|| anyhow!(aow_i18n::message("im.wechat_did_not_return_a_qr_code", &[])))?;
        let content = response["qrcode_img_content"].as_str().ok_or_else(|| {
            anyhow!(aow_i18n::message(
                "im.wechat_did_not_return_qr_code_content",
                &[]
            ))
        })?;
        // The upstream value is QR payload, NOT an image URL. Render locally;
        // never send a login QR to a third-party image-generation service.
        let svg = QrCode::new(content.as_bytes())
            .map_err(|_| anyhow!(aow_i18n::message("im.invalid_wechat_qr_code_content", &[])))?
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
        login.view.message =
            aow_i18n::text("im.scan_with_wechat_on_your_phone_and_confirm_the").into();
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
                aow_i18n::message("im.enter_the_number_displayed_in_wechat_on_your_phone", &[])
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
        ensure!(
            !snapshot.qrcode.is_empty(),
            aow_i18n::message("im.qr_code_is_not_ready_yet", &[])
        );
        let _polling = snapshot.polling.try_lock().map_err(|_| {
            anyhow!(aow_i18n::message(
                "im.waiting_for_wechat_scan_status_try_again_shortly",
                &[]
            ))
        })?;
        let response = self
            .api
            .qr_status(&snapshot.base_url, &snapshot.qrcode, verify_code)
            .await?;
        let mut active = self.active.lock().unwrap();
        let login = current(&mut active, id)?;
        let status = response["status"].as_str().ok_or_else(|| {
            anyhow!(aow_i18n::message(
                "im.wechat_did_not_return_scan_status",
                &[]
            ))
        })?;
        let message = match status {
            "wait" => aow_i18n::text("im.scan_with_wechat_on_your_phone"),
            "scaned" => aow_i18n::text("im.scanned_continue_confirmation_in_wechat_on_your_phone"),
            "need_verifycode" => {
                if verify_code.is_some() {
                    aow_i18n::text("im.number_mismatch_enter_the_number_displayed_in_wechat_on")
                } else {
                    aow_i18n::text("im.enter_the_number_displayed_in_wechat_on_your_phone_2")
                }
            }
            "expired" => aow_i18n::text("im.qr_code_expired_generate_a_new_one"),
            "verify_code_blocked" => {
                aow_i18n::text("im.too_many_verification_attempts_generate_a_new_qr_code")
            }
            "scaned_but_redirect" => {
                let host = response["redirect_host"].as_str().ok_or_else(|| {
                    anyhow!(aow_i18n::message(
                        "im.wechat_did_not_return_a_redirect_url",
                        &[]
                    ))
                })?;
                login.base_url = validate_base(&format!("https://{host}"))?;
                aow_i18n::text("im.scanned_continuing_verification")
            }
            "confirmed" => {
                let field = |name| {
                    response[name]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| {
                            anyhow!(aow_i18n::message(
                                "im.wechat_returned_incomplete_binding_information",
                                &[]
                            ))
                        })
                };
                let credentials = Credentials {
                    account_id: field("ilink_bot_id")?,
                    user_id: field("ilink_user_id")?,
                    bot_token: field("bot_token")?,
                    base_url: validate_base(
                        response["baseurl"].as_str().unwrap_or(&login.base_url),
                    )?,
                    binding_id: aow_id::new_id(),
                };
                credentials.validate()?;
                accept(credentials)?;
                aow_i18n::text("im.wechat_bot_linked_notifications_go_to_the_account_used")
            }
            "binded_redirect" => {
                let previous = login.previous.clone().ok_or_else(|| {
                    anyhow!(aow_i18n::message(
                        "im.wechat_reports_an_existing_binding_but_no_matching_local",
                        &[]
                    ))
                })?;
                ensure!(
                    response["ilink_bot_id"]
                        .as_str()
                        .is_none_or(|id| id == previous.account_id)
                        && response["ilink_user_id"]
                            .as_str()
                            .is_none_or(|id| id == previous.user_id),
                    aow_i18n::message("im.the_binding_reported_by_wechat_does_not_match_the", &[])
                );
                accept(previous)?;
                aow_i18n::text("im.restored_the_locally_saved_wechat_binding")
            }
            _ => bail!(aow_i18n::message(
                "im.wechat_returned_an_unknown_scan_status_generate_a_new",
                &[]
            )),
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
        .ok_or_else(|| {
            anyhow!(aow_i18n::message(
                "im.scan_canceled_or_replaced_by_a_new_qr_code",
                &[]
            ))
        })?;
    if login.started.elapsed() >= Duration::from_secs(300) && login.view.status != "confirmed" {
        login.view.status = "expired".into();
        login.view.qr_image = None;
        login.previous = None;
        bail!(aow_i18n::message(
            "im.qr_code_expired_generate_a_new_one_2",
            &[]
        ));
    }
    Ok(login)
}

#[cfg(test)]
mod tests;
