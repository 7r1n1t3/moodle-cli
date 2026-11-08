//! Settings from `~/.config/moodle-cli/config.toml`, overridable on the command line.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::fmt::{BOLD, CYAN, YELLOW, col};

const TEMPLATE: &str = r#"# moodle-cli settings. `--url` and `--token` on the command line override these.

# Address of your Moodle site, e.g. "https://moodle.example.edu"
url = ""

# Web service token: Moodle → Preferences → Security keys →
# copy the key of the "Moodle mobile web service" (<url>/user/managetoken.php).
token = ""
"#;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    url: String,
    #[serde(default)]
    token: String,
}

#[derive(Debug, PartialEq)]
pub struct Config {
    /// Site address without a trailing slash.
    pub url: String,
    pub token: String,
}

pub fn path() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("cannot find the config directory")?
        .join("moodle-cli/config.toml"))
}

/// Load the config, letting `url`/`token` arguments take precedence.
/// Creates a template on first launch and returns `Ok(None)` when the
/// user has to fill it in first.
pub fn load(url: Option<String>, token: Option<String>) -> Result<Option<Config>> {
    let path = path()?;
    let file = if path.exists() {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
    } else {
        create_template(&path)?;
        print_first_run(&path, url.as_deref());
        if url.is_none() || token.is_none() {
            return Ok(None);
        }
        File::default()
    };
    resolve(file, url, token, &path).map(Some)
}

fn resolve(file: File, url: Option<String>, token: Option<String>, path: &Path) -> Result<Config> {
    let url = url
        .unwrap_or(file.url)
        .trim()
        .trim_end_matches('/')
        .to_string();
    let token = token.unwrap_or(file.token).trim().to_string();
    if url.is_empty() {
        bail!(
            "no Moodle URL set: add `url` to {} or pass --url",
            path.display()
        );
    }
    let parsed = url::Url::parse(&url).with_context(|| format!("invalid Moodle URL: {url}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        bail!("the Moodle URL must start with https:// (got {url})");
    }
    if token.is_empty() {
        bail!(
            "no Moodle token set: add `token` to {} or pass --token \
             (see Moodle → Preferences → Security keys)",
            path.display()
        );
    }
    Ok(Config { url, token })
}

fn create_template(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        // The file will hold a token: keep it private.
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
        .and_then(|mut f| f.write_all(TEMPLATE.as_bytes()))
        .with_context(|| format!("creating {}", path.display()))
}

fn print_first_run(path: &Path, url: Option<&str>) {
    let site = url.unwrap_or("<your Moodle URL>").trim_end_matches('/');
    println!(
        "{}\n\n  {}\n",
        col(
            "Created a config file. Set your Moodle URL and token in:",
            BOLD
        ),
        col(&path.display().to_string(), CYAN),
    );
    println!("{}", col("How to get your Moodle token:", BOLD));
    println!("  1. Log in to Moodle in your browser.");
    println!("  2. Open the user menu (top right) → Preferences → Security keys,");
    println!(
        "     or go to {}",
        col(&format!("{site}/user/managetoken.php"), CYAN)
    );
    println!(
        "  3. Copy the key for the \"Moodle mobile web service\" (create it if there is none)."
    );
    println!("  4. Paste it as `token = \"…\"` in the config file.");
    println!(
        "\n  {}",
        col(
            "No Security keys page? Your site may have it turned off; ask your Moodle admins.",
            YELLOW
        )
    );
    println!(
        "\nYou can also pass both on the command line: {}",
        col("moodle-cli --url <URL> --token <TOKEN> status", CYAN)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(url: &str, token: &str) -> File {
        File {
            url: url.into(),
            token: token.into(),
        }
    }

    #[test]
    fn arguments_override_file() {
        let p = Path::new("c.toml");
        let c = resolve(
            file("https://a.example/", "filetok"),
            Some("https://b.example/".into()),
            None,
            p,
        )
        .unwrap();
        assert_eq!(
            c,
            Config {
                url: "https://b.example".into(),
                token: "filetok".into()
            }
        );
        let c = resolve(
            file("https://a.example", "x"),
            None,
            Some("argtok".into()),
            p,
        )
        .unwrap();
        assert_eq!(c.token, "argtok");
    }

    #[test]
    fn missing_or_bad_values_fail() {
        let p = Path::new("c.toml");
        assert!(resolve(file("", "t"), None, None, p).is_err());
        assert!(resolve(file("https://a.example", ""), None, None, p).is_err());
        assert!(resolve(file("moodle.example", "t"), None, None, p).is_err());
        assert!(resolve(file("ftp://a.example", "t"), None, None, p).is_err());
    }

    #[test]
    fn template_parses_as_empty() {
        let f: File = toml::from_str(TEMPLATE).unwrap();
        assert!(f.url.is_empty() && f.token.is_empty());
    }
}
