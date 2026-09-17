//! Site configuration loaded from `tless.toml`.

use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::{BASE_DIR, server::I18nConfig};

/// Configuration structure for the application.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Config {
    pub site: SiteConfig,
    pub i18n: Option<I18nConfig>,
}

/// Part of `[site]` configuration details.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct SiteConfig {
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub rights: String,
    pub author: String,
    pub url: String,
    pub zone: String,
    pub theme: String,
    pub favicon: String,
    pub menu: Vec<Menu>,
}

impl SiteConfig {
    /// Timezone configured in `[site] zone`, falling back to UTC.
    #[inline]
    pub(crate) fn zone(&self) -> Tz {
        self.zone.trim().parse().unwrap_or(Tz::UTC)
    }
}

/// Menu item structure for site navigation.
/// # Fields
/// * `name` - The display name of the menu item.
/// * `link` - The URL or path the menu item points to.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Menu {
    pub name: String,
    pub link: String,
}

/// Path to the configuration file (`tless.toml`).
#[inline]
fn config_path() -> PathBuf {
    BASE_DIR.join("tless.toml")
}

/// Load `tless.toml` from the working directory.
pub(crate) fn load() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        bail!("Configuration file not found at {}", path.display());
    }
    let text = fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

/// Timezone of the configured site, falling back to UTC.
pub(crate) fn zone() -> Tz {
    load().map(|config| config.site.zone()).unwrap_or(Tz::UTC)
}
