use std::path::PathBuf;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

/// Top-level settings loaded from TOML config.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub ai: AiSettings,
    pub ui: UiSettings,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AiSettings {
    pub default_model: String,
    pub claude_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub minimax_api_key: Option<String>,
    pub minimax_cn_api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct UiSettings {
    pub tick_rate_ms: u64,
    pub show_ai_panel: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ai: AiSettings::default(),
            ui: UiSettings::default(),
        }
    }
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            default_model: "claude".to_string(),
            claude_api_key: None,
            openai_api_key: None,
            minimax_api_key: None,
            minimax_cn_api_key: None,
        }
    }
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            tick_rate_ms: 250,
            show_ai_panel: true,
        }
    }
}

impl Settings {
    /// Load settings from `~/.config/gitclaw/config.toml`.
    /// Falls back to defaults if file doesn't exist.
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();
        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)?;
        let settings: Settings = toml::from_str(&content)?;
        Ok(settings)
    }

    /// Persist current settings to `~/.config/gitclaw/config.toml`.
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("gitclaw")
            .join("config.toml")
    }
}

impl AiSettings {
    /// Get Claude API key from config or ANTHROPIC_API_KEY env var.
    pub fn claude_api_key(&self) -> Result<String> {
        if let Some(ref key) = self.claude_api_key {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }
        std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow!("No Claude API key. Set ANTHROPIC_API_KEY or add claude_api_key to config.toml"))
    }

    /// Get OpenAI API key from config or OPENAI_API_KEY env var.
    pub fn openai_api_key(&self) -> Result<String> {
        if let Some(ref key) = self.openai_api_key {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }
        std::env::var("OPENAI_API_KEY")
            .map_err(|_| anyhow!("No OpenAI API key. Set OPENAI_API_KEY or add openai_api_key to config.toml"))
    }

    /// Get MiniMax API key from config or MINIMAX_API_KEY env var.
    pub fn minimax_api_key(&self) -> Result<String> {
        if let Some(ref key) = self.minimax_api_key {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }
        std::env::var("MINIMAX_API_KEY")
            .map_err(|_| anyhow!("No MiniMax API key. Set MINIMAX_API_KEY or add minimax_api_key to config.toml"))
    }

    /// Get MiniMax CN API key from config or MINIMAX_CN_API_KEY env var.
    pub fn minimax_cn_api_key(&self) -> Result<String> {
        if let Some(ref key) = self.minimax_cn_api_key {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }
        std::env::var("MINIMAX_CN_API_KEY")
            .map_err(|_| anyhow!("No MiniMax CN API key. Set MINIMAX_CN_API_KEY or add minimax_cn_api_key to config.toml"))
    }

    /// Persist an API key for the given model into the settings struct.
    /// Call `Settings::save()` afterwards to write to disk.
    pub fn set_api_key(&mut self, model: &str, key: String) {
        match model {
            "claude"      => self.claude_api_key      = Some(key),
            "openai"      => self.openai_api_key      = Some(key),
            "minimax"     => self.minimax_api_key     = Some(key),
            "minimax-cn"  => self.minimax_cn_api_key  = Some(key),
            _ => {}
        }
    }

    /// Return the API-key dashboard URL for the given provider.
    pub fn oauth_url(model: &str) -> Option<&'static str> {
        match model {
            "minimax"    => Some("https://platform.minimax.io/user-center/basic-information/interface-key"),
            "minimax-cn" => Some("https://platform.minimaxi.com/user-center/basic-information/interface-key"),
            "claude"     => Some("https://console.anthropic.com/settings/keys"),
            "openai"     => Some("https://platform.openai.com/api-keys"),
            _ => None,
        }
    }
}
