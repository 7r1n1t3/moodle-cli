//! Command-line client for TU Darmstadt Moodle.

mod api;
mod db;
mod download;
mod fmt;
mod portal;
mod status;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "TU Darmstadt Moodle from the terminal")]
struct Cli {
    /// Only use favourited courses (default: all in-progress courses)
    #[arg(long, global = true)]
    favourite: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show open assignments, quizzes and other due items
    Status {
        /// Look ahead this many days
        #[arg(long, default_value_t = 21)]
        days: i64,
    },
    /// Pick course files and save them
    Download,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn read_token() -> Result<String> {
    let path = dirs::config_dir()
        .context("cannot find the config directory")?
        .join("moodle/token");
    let token = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "cannot read the Moodle token from {} (create it with your web service token)",
            path.display()
        )
    })?;
    Ok(token.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    inquire::set_global_render_config(download::render_config());

    let client = api::Client::new(read_token()?)?;
    let mut db = db::Db::open_default()?;
    let classification = if cli.favourite {
        "favourites"
    } else {
        "inprogress"
    };
    let courses = client.courses(classification).await?;

    match cli.command {
        Command::Status { days } => status::run(&client, &mut db, &courses, days, now()).await,
        Command::Download => download::run(&client, &db, courses, now).await,
    }
}
