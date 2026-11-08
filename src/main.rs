//! Command-line client for Moodle.

mod api;
mod config;
mod db;
mod download;
mod fmt;
mod portal;
mod status;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Moodle from the terminal")]
struct Cli {
    /// Only use favourited courses (default: all in-progress courses)
    #[arg(long, global = true)]
    favourite: bool,

    /// Moodle site address, e.g. https://moodle.example.edu (overrides config.toml)
    #[arg(long, global = true, value_name = "URL")]
    url: Option<String>,

    /// Moodle web service token (overrides config.toml; visible in shell history)
    #[arg(long, global = true, value_name = "TOKEN")]
    token: Option<String>,

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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    inquire::set_global_render_config(download::render_config());

    let Some(config) = config::load(cli.url, cli.token)? else {
        std::process::exit(1);
    };
    let client = api::Client::new(config.url, config.token)?;
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
