use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::{env, fs};

pub fn claude_config_dir() -> PathBuf {
    if let Ok(dir) = env::var("CLAUDE_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        dirs_home().join(".claude")
    }
}

pub fn tokenburn_data_dir() -> PathBuf {
    let claude_dir = claude_config_dir();
    let suffix = claude_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".claude")
        .strip_prefix(".claude")
        .unwrap_or("")
        .to_string();
    claude_dir
        .parent()
        .unwrap_or(&claude_dir)
        .join(format!(".tokenburn{suffix}"))
}

fn dirs_home() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn credentials_path() -> PathBuf {
    if let Ok(p) = env::var("CREDENTIALS_PATH") {
        return PathBuf::from(p);
    }
    claude_config_dir().join(".credentials.json")
}

fn read_credentials_file() -> Option<serde_json::Value> {
    let path = credentials_path();
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn get_access_token() -> Result<String> {
    let creds = read_credentials_file().ok_or_else(|| {
        anyhow!(
            "Credentials not found at {:?}. Run 'claude' to log in.",
            credentials_path()
        )
    })?;

    let oauth = creds
        .get("claudeAiOauth")
        .ok_or_else(|| anyhow!("No OAuth section in credentials"))?;

    // Check expiry
    if let Some(expires_at) = oauth.get("expiresAt") {
        let expired = if let Some(s) = expires_at.as_str() {
            let expiry: Result<DateTime<Utc>, _> = s.parse();
            expiry.map(|e| Utc::now() >= e).unwrap_or(false)
        } else if let Some(n) = expires_at.as_f64() {
            let ms = if n > 1e12 { n / 1000.0 } else { n };
            let expiry = DateTime::from_timestamp(ms as i64, 0).unwrap_or(Utc::now());
            Utc::now() >= expiry
        } else {
            false
        };
        if expired {
            bail!("OAuth token has expired. Run 'claude' to refresh.");
        }
    }

    oauth
        .get("accessToken")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("No access token in credentials. Run 'claude' to log in."))
}
