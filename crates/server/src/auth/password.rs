use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
};

use axum::http::StatusCode;
use ring::{digest, pbkdf2};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use uuid::Uuid;

use super::provider::{LoginProvider, VerifiedIdentity};
use crate::HttpError;

pub(super) const METHOD: &str = "password";
pub(super) const CREDENTIALS_FILE: &str = "credentials.json";
const PASSWORD_ITERATIONS: NonZeroU32 = NonZeroU32::new(600_000).unwrap();

// Version 1 matches the installer's local credential format.
#[derive(Deserialize, Serialize)]
struct Credentials {
    version: u32,
    username: String,
    salt: String,
    password_hash: String,
}

impl Credentials {
    fn identity(&self) -> VerifiedIdentity {
        let document = serde_json::to_vec(self).expect("credential strings are serializable");
        let revision = digest::digest(&digest::SHA256, &document)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        VerifiedIdentity {
            subject: self.username.clone(),
            revision,
        }
    }
}

#[derive(Deserialize)]
struct PasswordCredentials {
    username: String,
    password: String,
}

pub(super) struct PasswordProvider {
    credentials_path: PathBuf,
}

impl PasswordProvider {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            credentials_path: state_dir.join(CREDENTIALS_FILE),
        }
    }

    fn configuration(&self) -> Result<Credentials, HttpError> {
        let path = &self.credentials_path;
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(account_not_configured());
            }
            Err(error) => {
                return Err(configuration_error(format!(
                    "无法读取账户配置 {}：{error}",
                    path.display()
                )));
            }
        };
        match serde_json::from_str::<Credentials>(&contents) {
            Ok(credentials)
                if credentials.version == 1
                    && valid_username(&credentials.username)
                    && decode_hex::<16>(&credentials.salt).is_some()
                    && decode_hex::<32>(&credentials.password_hash).is_some() =>
            {
                Ok(credentials)
            }
            _ => Err(configuration_error(format!(
                "账户配置 {} 无效；请重新运行 `aow account`",
                path.display()
            ))),
        }
    }
}

impl LoginProvider for PasswordProvider {
    fn id(&self) -> &'static str {
        METHOD
    }
    fn label(&self) -> &'static str {
        "账号密码"
    }
    fn check_configured(&self) -> Result<(), HttpError> {
        self.configuration().map(|_| ())
    }

    fn authenticate(&self, credentials: &Value) -> Result<VerifiedIdentity, HttpError> {
        let request: PasswordCredentials =
            serde_json::from_value(credentials.clone()).map_err(|_| {
                HttpError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_credentials",
                    "请输入账号和密码",
                    None,
                )
            })?;
        if !valid_username(&request.username)
            || request.password.is_empty()
            || request.password.len() > 1024
        {
            return Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                "invalid_credentials",
                "请输入有效的账号和密码",
                None,
            ));
        }
        let configured = self.configuration()?;
        let salt = decode_hex::<16>(&configured.salt).expect("validated salt");
        let hash = decode_hex::<32>(&configured.password_hash).expect("validated hash");
        let password_matches = pbkdf2::verify(
            pbkdf2::PBKDF2_HMAC_SHA256,
            PASSWORD_ITERATIONS,
            &salt,
            request.password.as_bytes(),
            &hash,
        )
        .is_ok();
        if !password_matches || request.username != configured.username {
            return Err(HttpError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "账号或密码不正确",
                None,
            ));
        }
        Ok(configured.identity())
    }

    fn validate_session(&self, identity: &VerifiedIdentity) -> Result<bool, HttpError> {
        Ok(self.configuration()?.identity() == *identity)
    }
}

fn configuration_error(message: String) -> HttpError {
    HttpError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "account_configuration_invalid",
        message,
        None,
    )
}

fn valid_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 256
        && !username
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

fn decode_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

fn account_not_configured() -> HttpError {
    HttpError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "account_not_configured",
        "AoW 尚未设置登录账户；请在服务器上运行 `aow account`",
        None,
    )
}

#[cfg(test)]
pub(crate) fn write_credentials(directory: &Path, username: &str, password: &str) {
    let salt = Uuid::new_v4();
    let mut hash = [0; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        PASSWORD_ITERATIONS,
        salt.as_bytes(),
        password.as_bytes(),
        &mut hash,
    );
    let credentials = Credentials {
        version: 1,
        username: username.into(),
        salt: salt.simple().to_string(),
        password_hash: hash.iter().map(|byte| format!("{byte:02x}")).collect(),
    };
    std::fs::write(
        directory.join(CREDENTIALS_FILE),
        serde_json::to_vec(&credentials).unwrap(),
    )
    .unwrap();
}
