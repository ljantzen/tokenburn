use anyhow::{Result, anyhow, bail};

use crate::credentials::get_access_token;
use crate::models::UsageSnapshot;

const API_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const API_BETA_HEADER: &str = "oauth-2025-04-20";

pub struct UsageClient {
    agent: ureq::Agent,
}

impl UsageClient {
    pub fn new() -> Self {
        UsageClient {
            agent: ureq::AgentBuilder::new().build(),
        }
    }

    pub fn fetch_usage(&self) -> Result<UsageSnapshot> {
        let token = get_access_token()?;

        let mut last_err = anyhow!("no attempt");
        for delay in [0u64, 1, 2] {
            if delay > 0 {
                std::thread::sleep(std::time::Duration::from_secs(delay));
            }
            match self
                .agent
                .get(API_URL)
                .set("Accept", "application/json")
                .set("Authorization", &format!("Bearer {token}"))
                .set("anthropic-beta", API_BETA_HEADER)
                .call()
            {
                Ok(resp) => {
                    let body: serde_json::Value = resp.into_json()?;
                    return Ok(UsageSnapshot::from_api_response(&body));
                }
                Err(ureq::Error::Status(429, _)) => {
                    bail!(
                        "Rate limited (429). Try running 'tokenburn collect' via statusline to avoid this."
                    );
                }
                Err(ureq::Error::Status(401, _)) => {
                    bail!("Authentication failed (401). Run 'claude' to refresh your token.");
                }
                Err(ureq::Error::Status(403, _)) => {
                    bail!("Access forbidden (403).");
                }
                Err(ureq::Error::Status(code, _)) if code >= 500 => {
                    last_err = anyhow!("Server error {code}");
                }
                Err(e) => {
                    last_err = anyhow!("Network error: {e}");
                }
            }
        }
        Err(last_err)
    }
}

impl Default for UsageClient {
    fn default() -> Self {
        Self::new()
    }
}
