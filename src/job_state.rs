use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const STATE_FILE: &str = ".webcloner-job.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobOptionsSnapshot {
    pub include_external_assets: bool,
    pub follow_external_pages: bool,
    pub concurrency: usize,
    pub timeout_secs: u64,
    pub user_agent: String,
    pub block_tracking: bool,
    pub report_broken_links: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    pub version: u32,
    pub start_url: String,
    pub site_host: String,
    pub planned_pages: Vec<String>,
    pub done_pages: Vec<String>,
    pub planned_assets: Vec<String>,
    pub done_assets: Vec<String>,
    /// absolute URL -> relative path string
    pub url_map: HashMap<String, String>,
    pub options: JobOptionsSnapshot,
    pub phase: String,
    pub updated_at: u64,
}

impl JobState {
    pub fn path_for(out_dir: &Path) -> PathBuf {
        out_dir.join(STATE_FILE)
    }

    pub fn load(out_dir: &Path) -> Result<Option<Self>> {
        let path = Self::path_for(out_dir);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("خواندن وضعیت از {} ناموفق بود", path.display()))?;
        let state: Self = serde_json::from_str(&raw).context("فایل وضعیت خراب است.")?;
        Ok(Some(state))
    }

    pub fn save(&self, out_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(out_dir)?;
        let path = Self::path_for(out_dir);
        let raw = serde_json::to_string_pretty(self).context("serialize job state")?;
        std::fs::write(&path, raw)
            .with_context(|| format!("نوشتن وضعیت در {} ناموفق بود", path.display()))?;
        Ok(())
    }

    pub fn clear(out_dir: &Path) -> Result<()> {
        let path = Self::path_for(out_dir);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    pub fn now_secs() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}
