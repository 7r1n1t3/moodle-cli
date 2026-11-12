//! `moodle-cli download`: pick course files and save them via the portal.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use inquire::error::InquireError;
use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet, Styled};
use inquire::{Confirm, MultiSelect, Select};

use crate::api::{Client, Course, FileEntry};
use crate::db::{Db, DownloadRow, LastDownload};
use crate::fmt::{BOLD, CYAN, DIM, GREEN, RED, YELLOW, col, day_month, size, tilde};
use crate::portal;

/// An entry in a picker: shown with colours, filtered on plain text.
struct Item<T> {
    plain: String,
    display: String,
    value: T,
}

impl<T> fmt::Display for Item<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display)
    }
}

/// Case-insensitive filter on the plain label; every word must match.
/// Equal scores keep the original order.
fn contains_all<T>(input: &str, item: &Item<T>, _: &str, _: usize) -> Option<i64> {
    let hay = item.plain.to_lowercase();
    input
        .to_lowercase()
        .split_whitespace()
        .all(|w| hay.contains(w))
        .then_some(0)
}

#[derive(Clone)]
struct RemoteFile {
    url: String,
    filename: String,
    timemodified: i64,
}

pub fn render_config() -> RenderConfig<'static> {
    if !*crate::fmt::COLOR {
        return RenderConfig::empty();
    }
    RenderConfig::default_colored()
        .with_prompt_prefix(Styled::new("?").with_fg(Color::LightCyan))
        .with_answered_prompt_prefix(Styled::new("✓").with_fg(Color::LightGreen))
        .with_highlighted_option_prefix(Styled::new("❯").with_fg(Color::LightCyan))
        .with_selected_checkbox(Styled::new("[✓]").with_fg(Color::LightGreen))
        .with_unselected_checkbox(Styled::new("[ ]").with_fg(Color::DarkGrey))
        .with_help_message(StyleSheet::new().with_fg(Color::DarkYellow))
        .with_answer(
            StyleSheet::new()
                .with_fg(Color::LightGreen)
                .with_attr(Attributes::BOLD),
        )
        .with_selected_option(Some(StyleSheet::new().with_fg(Color::LightCyan)))
        .with_canceled_prompt_indicator(Styled::new("<cancelled>").with_fg(Color::DarkRed))
}

/// True when the user pressed Esc or Ctrl-C.
fn aborted(e: &InquireError) -> bool {
    matches!(
        e,
        InquireError::OperationCanceled | InquireError::OperationInterrupted
    )
}

pub async fn run(
    client: &Client,
    db: &Db,
    courses: Vec<Course>,
    now: impl Fn() -> i64,
) -> Result<()> {
    if courses.is_empty() {
        println!("{}", col("No courses found.", DIM));
        return Ok(());
    }
    let ids: Vec<i64> = courses.iter().map(|c| c.id).collect();
    let assigns: HashMap<i64, _> = client
        .assignments(&ids)
        .await?
        .into_iter()
        .map(|c| (c.id, c.assignments))
        .collect();

    'courses: loop {
        let options: Vec<Item<Course>> = courses
            .iter()
            .map(|c| Item {
                plain: c.name().to_string(),
                display: col(c.name(), BOLD),
                value: c.clone(),
            })
            .collect();
        let course = match Select::new("Course:", options)
            .with_scorer(&contains_all)
            .with_formatter(&|o| o.value.plain.clone())
            .with_help_message("↑↓ to move, type to filter, enter to pick, esc to quit")
            .prompt()
        {
            Ok(c) => c.value,
            Err(e) if aborted(&e) => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        println!("{}", col("→ loading course files…", CYAN));
        let mut files = Vec::new();
        for section in client.course_contents(course.id).await? {
            for m in section.modules {
                for f in m.contents {
                    if f.kind.as_deref() == Some("file") {
                        files.push((format!("{} › {}", section.name, m.name), f));
                    }
                }
            }
        }
        for a in assigns.get(&course.id).into_iter().flatten() {
            for f in &a.introattachments {
                files.push((format!("Assignment › {}", a.name), f.clone()));
            }
        }
        if files.is_empty() {
            println!("{}", col("  no files in this course", DIM));
        } else {
            let downloaded = db.last_downloads()?;
            // Indices picked last time, so returning here keeps the selection.
            let mut selected: Vec<usize> = Vec::new();
            loop {
                let items: Vec<Item<RemoteFile>> = files
                    .iter()
                    .cloned()
                    .map(|(prefix, f)| file_item(prefix, f, &downloaded))
                    .collect();
                let picked = MultiSelect::new("Files:", items)
                    .with_scorer(&contains_all)
                    .with_formatter(&|sel| format!("{} file(s)", sel.len()))
                    .with_page_size(15)
                    .with_default(&selected)
                    .with_help_message(
                        "space to toggle, → all, ← none, type to filter, enter to save, esc to go back",
                    )
                    .raw_prompt();
                match picked {
                    Ok(sel) if !sel.is_empty() => {
                        selected = sel.iter().map(|o| o.index).collect();
                        let files: Vec<RemoteFile> =
                            sel.into_iter().map(|o| o.value.value).collect();
                        match save(client, db, &course, &files, &now).await? {
                            Outcome::Done => break,
                            Outcome::Back => continue,
                            Outcome::Quit => return Ok(()),
                        }
                    }
                    Ok(_) => break,
                    Err(InquireError::OperationCanceled) => continue 'courses,
                    Err(e) if aborted(&e) => return Ok(()),
                    Err(e) => return Err(e.into()),
                }
            }
        }

        match Confirm::new("Download other files?")
            .with_default(false)
            .prompt()
        {
            Ok(true) => {}
            Ok(false) => return Ok(()),
            Err(e) if aborted(&e) => return Ok(()),
            Err(e) => return Err(e.into()),
        }
    }
}

fn file_item(
    prefix: String,
    f: FileEntry,
    downloaded: &HashMap<String, LastDownload>,
) -> Item<RemoteFile> {
    let path = f.filepath.as_deref().unwrap_or("/").trim_start_matches('/');
    let mut plain = format!("{prefix} › {path}{}", f.filename);
    if let Some(s) = f.filesize {
        plain.push_str(&format!(" ({})", size(s)));
    }
    let timemodified = f.timemodified.unwrap_or(0);
    let tag = match downloaded.get(&f.fileurl) {
        Some(d) if timemodified > d.timemodified => col("(updated)", YELLOW),
        Some(d) => col(
            &format!(
                "(downloaded {} → {})",
                day_month(d.downloaded_at),
                tilde(&d.saved_path)
            ),
            GREEN,
        ),
        None => String::new(),
    };
    let display = format!(
        "{} › {}{}",
        col(&prefix, DIM),
        &plain[prefix.len() + " › ".len()..],
        if tag.is_empty() {
            tag
        } else {
            format!(" {tag}")
        }
    );
    Item {
        plain,
        display,
        value: RemoteFile {
            url: f.fileurl,
            filename: f.filename,
            timemodified,
        },
    }
}

/// What to do after `save`.
enum Outcome {
    /// Files were downloaded (or attempted).
    Done,
    /// Nothing saved; show the file list again.
    Back,
    /// The user pressed Ctrl-C.
    Quit,
}

async fn save(
    client: &Client,
    db: &Db,
    course: &Course,
    files: &[RemoteFile],
    now: &impl Fn() -> i64,
) -> Result<Outcome> {
    println!("{}", col("→ choose a folder to save into…", CYAN));
    let Some(dir) = portal::pick_folder(files.len()).await? else {
        println!("  {}", col("cancelled, nothing saved", DIM));
        return Ok(Outcome::Back);
    };
    let names: Vec<&str> = files.iter().map(|f| f.filename.as_str()).collect();
    let targets = target_paths(&dir, &names);

    let existing: Vec<&PathBuf> = targets.iter().filter(|p| p.exists()).collect();
    if !existing.is_empty() {
        println!(
            "{}",
            col(
                &format!(
                    "! {} file(s) already exist and will be overwritten:",
                    existing.len()
                ),
                YELLOW
            )
        );
        for p in &existing {
            println!("  {}", tilde(&p.to_string_lossy()));
        }
        match Confirm::new("Overwrite them?").with_default(false).prompt() {
            Ok(true) => {}
            Ok(false) | Err(InquireError::OperationCanceled) => return Ok(Outcome::Back),
            Err(e) if aborted(&e) => return Ok(Outcome::Quit),
            Err(e) => return Err(e.into()),
        }
    }

    for (f, path) in files.iter().zip(targets) {
        let result = async {
            let bytes = client.download(&f.url).await?;
            std::fs::write(&path, bytes)?;
            anyhow::Ok(())
        }
        .await;
        let shown = tilde(&path.to_string_lossy());
        match result {
            Ok(()) => {
                db.record_download(&DownloadRow {
                    course_id: course.id,
                    course_name: course.name().to_string(),
                    file_url: f.url.clone(),
                    filename: f.filename.clone(),
                    timemodified: f.timemodified,
                    saved_path: path.to_string_lossy().into_owned(),
                    downloaded_at: now(),
                })?;
                println!(
                    "  {} {} → {}",
                    col("✓", GREEN),
                    f.filename,
                    col(&shown, DIM)
                );
            }
            Err(e) => println!(
                "  {} {}: {}",
                col("✗", RED),
                f.filename,
                col(&format!("{e:#}"), RED)
            ),
        }
    }
    Ok(Outcome::Done)
}

/// Paths inside `dir` for each of `names`. Names are reduced to their last
/// component, and repeats within the batch get a ` (2)`, ` (3)`, … suffix.
/// Existing files on disk are overwritten.
fn target_paths(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
    let mut used = HashSet::new();
    names
        .iter()
        .map(|name| {
            let base = Path::new(name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file");
            let mut candidate = base.to_string();
            let mut n = 2;
            while !used.insert(candidate.clone()) {
                candidate = match base.rsplit_once('.') {
                    Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
                    _ => format!("{base} ({n})"),
                };
                n += 1;
            }
            dir.join(candidate)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_paths_dedupes_and_strips_dirs() {
        let dir = Path::new("/d");
        let got = target_paths(
            dir,
            &[
                "a.pdf", "a.pdf", "../x", ".bashrc", ".bashrc", "..", "a.pdf",
            ],
        );
        let want = [
            "a.pdf",
            "a (2).pdf",
            "x",
            ".bashrc",
            ".bashrc (2)",
            "file",
            "a (3).pdf",
        ];
        assert_eq!(got, want.iter().map(|w| dir.join(w)).collect::<Vec<_>>());
    }
}
