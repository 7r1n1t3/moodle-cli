//! Save locations via the xdg-desktop-portal FileChooser.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ashpd::desktop::ResponseError;
use ashpd::desktop::file_chooser::SelectedFiles;

/// Ask where to save each of `names`. `None` means that file was skipped.
pub async fn save_locations(names: &[String]) -> Result<Vec<Option<PathBuf>>> {
    match names {
        [] => Ok(Vec::new()),
        [name] => Ok(vec![save_one(name).await?]),
        _ => match save_many(names).await {
            Ok(paths) => Ok(paths),
            // Backends without `SaveFiles` support: ask once per file.
            Err(_) => {
                let mut out = Vec::with_capacity(names.len());
                for name in names {
                    out.push(save_one(name).await?);
                }
                Ok(out)
            }
        },
    }
}

async fn save_one(name: &str) -> Result<Option<PathBuf>> {
    let res = SelectedFiles::save_file()
        .title(format!("Save {name}").as_str())
        .accept_label("Save")
        .modal(true)
        .current_name(name)
        .send()
        .await
        .context("opening the file chooser portal")?
        .response();
    match res {
        Ok(sel) => Ok(sel.uris().first().map(uri_to_path).transpose()?),
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(None),
        Err(e) => Err(e).context("file chooser portal"),
    }
}

/// Uses `SaveFiles`. Cancelling skips every file; other errors are returned
/// so the caller can fall back to `SaveFile`.
async fn save_many(names: &[String]) -> Result<Vec<Option<PathBuf>>> {
    let res = SelectedFiles::save_files()
        .title(format!("Save {} files", names.len()).as_str())
        .accept_label("Save")
        .modal(true)
        .files(names.iter().map(String::as_str).collect::<Vec<_>>())?
        .send()
        .await?
        .response();
    match res {
        Ok(sel) => {
            let mut out: Vec<Option<PathBuf>> = sel
                .uris()
                .iter()
                .map(|u| uri_to_path(u).map(Some))
                .collect::<Result<_>>()?;
            out.resize(names.len(), None);
            Ok(out)
        }
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(vec![None; names.len()]),
        Err(e) => Err(e.into()),
    }
}

fn uri_to_path(uri: &ashpd::Uri) -> Result<PathBuf> {
    url::Url::parse(uri.as_str())
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .with_context(|| format!("portal returned a non-file URI: {}", uri.as_str()))
}
