use anyhow::{Context, Result, ensure};
use ring::hmac;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
type CachedEnvironment = Option<(u64, BTreeMap<String, String>)>;
#[derive(Clone, Default)]
pub struct Access(
    Arc<Mutex<Option<Configuration>>>,
    Arc<Mutex<CachedEnvironment>>,
);
#[derive(Clone, Serialize, Deserialize)]
struct Configuration {
    url: String,
    key_file: PathBuf,
    directory: PathBuf,
}
#[derive(Serialize)]
pub struct Grant {
    pub id: String,
    pub token: String,
    pub expires_at: u64,
}
fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
fn database(config: &Configuration) -> Result<Connection> {
    let connection = Connection::open(config.directory.join("grants.sqlite"))?;
    fs::set_permissions(
        config.directory.join("grants.sqlite"),
        fs::Permissions::from_mode(0o600),
    )?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS grants(id TEXT PRIMARY KEY, expires INTEGER NOT NULL)",
    )?;
    Ok(connection)
}
fn key(config: &Configuration) -> Result<hmac::Key> {
    let metadata = fs::symlink_metadata(&config.key_file)?;
    ensure!(
        metadata.is_file() && metadata.permissions().mode() & 0o077 == 0,
        "CLI token file must be private (0600)"
    );
    let token = fs::read_to_string(&config.key_file)?;
    let token = token.trim();
    ensure!(
        token.len() >= 32 && token.bytes().all(|c| c.is_ascii_graphic()),
        "Invalid CLI token file"
    );
    Ok(hmac::Key::new(hmac::HMAC_SHA256, token.as_bytes()))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn unhex(value: &str) -> Result<Vec<u8>> {
    ensure!(value.len() == 64, "Invalid CLI token");
    (0..value.len())
        .step_by(2)
        .map(|i| Ok(u8::from_str_radix(&value[i..i + 2], 16)?))
        .collect()
}
impl Access {
    pub fn configure(&self, state: &Path, url: &str, key_file: Option<PathBuf>) -> Result<()> {
        ensure!(
            url.starts_with("http://") || url.starts_with("https://"),
            "CLI callback URL must use HTTP(S)"
        );
        let directory = state.join("runtime/cli");
        fs::create_dir_all(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let key_file = key_file.unwrap_or_else(|| directory.join("token"));
        if !key_file.exists() {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&key_file)?;
            writeln!(
                file,
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )?;
            file.sync_all()?;
        }
        let key_file = key_file.canonicalize()?;
        let directory = directory.canonicalize()?;
        let config = Configuration {
            url: url.trim_end_matches('/').into(),
            key_file,
            directory,
        };
        key(&config)?;
        database(&config)?;
        let path = config.directory.join("endpoint.json");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(&serde_json::to_vec(&config)?)?;
        file.sync_all()?;
        *self.0.lock().unwrap() = Some(config);
        *self.1.lock().unwrap() = None;
        Ok(())
    }
    pub fn from_state(state: &Path) -> Result<Self> {
        let path = state.join("runtime/cli/endpoint.json");
        let config = match fs::read(path) {
            Ok(bytes) => Some(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self(Arc::new(Mutex::new(config)), Default::default()))
    }
    pub fn issue(&self, ttl: u64) -> Result<Grant> {
        ensure!(
            (60..=30 * 24 * 3600).contains(&ttl),
            "CLI token lifetime must be 60 seconds to 30 days"
        );
        let config = self
            .0
            .lock()
            .unwrap()
            .clone()
            .context("CLI access is not configured")?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let expires_at = now()? + ttl;
        let message = format!("cli.{id}.{expires_at}");
        let token = format!(
            "{message}.{}",
            hex(hmac::sign(&key(&config)?, message.as_bytes()).as_ref())
        );
        let db = database(&config)?;
        db.execute("DELETE FROM grants WHERE expires <= ?1", [now()?])?;
        db.execute(
            "INSERT INTO grants(id,expires) VALUES(?1,?2)",
            params![id, expires_at],
        )?;
        Ok(Grant {
            id,
            token,
            expires_at,
        })
    }
    pub fn authorize(&self, token: &str) -> Result<()> {
        ensure!(token.len() <= 256 && token.is_ascii(), "Invalid CLI token");
        let config = self
            .0
            .lock()
            .unwrap()
            .clone()
            .context("CLI access is not configured")?;
        let signing = key(&config)?;
        if hmac::verify(
            &signing,
            b"master",
            hmac::sign(
                &hmac::Key::new(hmac::HMAC_SHA256, token.as_bytes()),
                b"master",
            )
            .as_ref(),
        )
        .is_ok()
        {
            return Ok(());
        }
        let (message, signature) = token.rsplit_once('.').context("Invalid CLI token")?;
        hmac::verify(&signing, message.as_bytes(), &unhex(signature)?)
            .map_err(|_| anyhow::anyhow!("Invalid CLI token"))?;
        let fields: Vec<_> = message.split('.').collect();
        ensure!(fields.len() == 3 && fields[0] == "cli", "Invalid CLI token");
        let expires: u64 = fields[2].parse()?;
        ensure!(expires > now()?, "CLI token expired");
        let stored: u64 = database(&config)?
            .query_row(
                "SELECT expires FROM grants WHERE id=?1",
                [fields[1]],
                |row| row.get(0),
            )
            .context("CLI token revoked")?;
        ensure!(stored == expires, "Invalid CLI token");
        Ok(())
    }
    pub fn revoke(&self, id: &str) -> Result<()> {
        let config = self
            .0
            .lock()
            .unwrap()
            .clone()
            .context("CLI access is not configured")?;
        database(&config)?.execute("DELETE FROM grants WHERE id=?1", [id])?;
        Ok(())
    }
    /// ACP shares one adapter across sessions; credentials must not change its cache key on each call.
    pub fn cached_environment(&self) -> Result<BTreeMap<String, String>> {
        let mut cached = self.1.lock().unwrap();
        if let Some((expires, environment)) = cached.as_ref()
            && *expires > now()? + 300
            && environment
                .get("AOW_SERVER_TOKEN")
                .is_some_and(|token| self.authorize(token).is_ok())
        {
            return Ok(environment.clone());
        }
        let environment = self.environment()?;
        *cached = Some((now()? + 86400, environment.clone()));
        Ok(environment)
    }
    pub fn environment(&self) -> Result<BTreeMap<String, String>> {
        let config = self.0.lock().unwrap().clone();
        let Some(config) = config else {
            return Ok(BTreeMap::new());
        };
        let grant = self.issue(24 * 3600)?;
        Ok(BTreeMap::from([
            ("AOW_SERVER_URL".into(), config.url),
            ("AOW_SERVER_TOKEN".into(), grant.token),
        ]))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_survive_restart_and_support_expiry_revocation_and_key_rotation() {
        let root = tempfile::tempdir().unwrap();
        let access = Access::default();
        access
            .configure(root.path(), "http://server:9800", None)
            .unwrap();
        assert_eq!(
            access.cached_environment().unwrap(),
            access.cached_environment().unwrap()
        );
        let config = access.0.lock().unwrap().clone().unwrap();
        let master = fs::read_to_string(&config.key_file).unwrap();
        access.authorize(master.trim()).unwrap();
        let message = format!("cli.expired.{}", now().unwrap() - 1);
        let expired = format!(
            "{message}.{}",
            hex(hmac::sign(&key(&config).unwrap(), message.as_bytes()).as_ref())
        );
        assert!(
            access
                .authorize(&expired)
                .unwrap_err()
                .to_string()
                .contains("expired")
        );
        let grant = access.issue(60).unwrap();
        let restored = Access::from_state(root.path()).unwrap();
        restored.authorize(&grant.token).unwrap();
        assert!(restored.authorize(&(grant.token.clone() + "a")).is_err());
        restored.revoke(&grant.id).unwrap();
        assert!(access.authorize(&grant.token).is_err());
        let next = access.issue(60).unwrap();
        fs::write(root.path().join("runtime/cli/token"), "a".repeat(64)).unwrap();
        assert!(access.authorize(&next.token).is_err());
        assert!(access.issue(0).is_err());
        fs::set_permissions(&config.key_file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(access.cached_environment().is_err());
    }
}
