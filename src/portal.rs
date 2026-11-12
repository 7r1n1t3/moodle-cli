//! Pick a download folder via the xdg-desktop-portal FileChooser.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ashpd::desktop::ResponseError;
use ashpd::desktop::file_chooser::SelectedFiles;

/// Ask for a folder to save `count` files into. `None` means cancelled.
pub async fn pick_folder(count: usize) -> Result<Option<PathBuf>> {
    let res = SelectedFiles::open_file()
        .title(format!("Save {count} file(s) to folder").as_str())
        .accept_label("Save here")
        .modal(true)
        .directory(true)
        .multiple(false)
        .send()
        .await
        .context("opening the file chooser portal")?
        .response();
    match res {
        Ok(sel) => Ok(sel.uris().first().map(uri_to_path).transpose()?),
        // Some backends (e.g. termfilechooser) report closing the chooser
        // without a choice as `Other` rather than `Cancelled`.
        Err(ashpd::Error::Response(ResponseError::Cancelled | ResponseError::Other)) => Ok(None),
        Err(e) => Err(e).context("file chooser portal"),
    }
}

fn uri_to_path(uri: &ashpd::Uri) -> Result<PathBuf> {
    url::Url::parse(uri.as_str())
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .with_context(|| format!("portal returned a non-file URI: {}", uri.as_str()))
}
