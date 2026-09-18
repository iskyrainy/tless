//! Site scaffold generation for `tless site -i`.

use std::{env, fs};

use anyhow::{Context, Result, bail};

use crate::server::template;

/// Initialize a complete, deploy-ready site scaffold in the current directory.
///
/// Everything is created at the repository root: `tless.toml`, the source and
/// theme directories, plus the `.github/workflows/deploy.yml` and `.gitignore`
/// needed to publish the site to GitHub Pages.
pub fn init() -> Result<()> {
    let current_dir = env::current_dir().context("Cannot get current directory")?;
    if current_dir.join("tless.toml").exists() {
        bail!("Site already initialized in this directory");
    }

    // Empty directories that must survive in git get a .gitkeep
    let tracked_dirs = [
        "helper",
        "plugin",
        "source/draft",
        "source/post",
        "source/page",
        "source/i18n",
    ];
    for dir in tracked_dirs {
        let dir = current_dir.join(dir);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(".gitkeep"), "")?;
    }
    // Build output, ignored by .gitignore
    fs::create_dir_all(current_dir.join("public"))?;

    template::write_base_site(&current_dir)?;
    Ok(())
}
