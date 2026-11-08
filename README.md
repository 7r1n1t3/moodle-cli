# moodle-cli

[Moodle](https://moodle.org) from the terminal:

- **`status`**: open assignments, quizzes and other due items for your courses, with submission state. Items you haven't seen before are marked with a red `(new!)`.
- **`download`**: pick a course, select files from it, and save them through your desktop's file chooser. The tool remembers what you downloaded and where.

```
$ moodle-cli status
Software Engineering  (https://moodle.tu-darmstadt.de/course/view.php?id=48710)
  ✓ submitted              Abgabe 02: Anwendungsfallmodellierung  — due Fri 06.11 10:00 (2d ago)
  ○ to do                  Abgabe 03: Domänenmodellierung  — due Fri 13.11 10:00 (in 5d) (new!)

1 open item(s) in the next 21 days
```

## Requirements

- Linux with [xdg-desktop-portal](https://flatpak.github.io/xdg-desktop-portal/) and a FileChooser backend (GTK, KDE, termfilechooser, …). Only `download` needs it.
- A Rust toolchain (edition 2024) to build.
- A Moodle site (3.6 or newer) with the mobile web service enabled. If the Moodle mobile app works for your site, this tool will too.
- A web service token for that site.

## Install

```sh
cargo install --git https://github.com/7r1n1t3/moodle-cli
```

Or from a clone: `cargo install --path .`

## Setup

On first launch, `moodle-cli` creates `~/.config/moodle-cli/config.toml`, explains how to get a token, and exits. Fill in both values:

```toml
url = "https://moodle.example.edu"
token = "0123456789abcdef0123456789abcdef"
```

To get the token:

1. Log in to Moodle in your browser.
2. Open the user menu → *Preferences* → *Security keys*, or go to `<url>/user/managetoken.php`.
3. Copy the key for the *Moodle mobile web service*. Create it if there is none.

If there is no *Security keys* page, your site has it turned off; ask your Moodle admins.

You can also pass `--url` and `--token` on the command line. They override the config file, so you don't need one. A token passed this way ends up in your shell history.

## Usage

```sh
moodle-cli status                # look 21 days ahead (overdue items: 30 days back)
moodle-cli status --days 7       # custom look-ahead window
moodle-cli download              # interactive file picker
moodle-cli --favourite status    # only favourited courses (works for both commands)
moodle-cli --url https://moodle.example.edu --token TOKEN status
```

By default both commands cover all in-progress courses.

### `status`

For each course, it lists:

- assignments, tagged `✓ submitted` (with grade if available), `○ to do` or `✗ OVERDUE`
- other actionable calendar events, such as quizzes

The first run marks everything `(new!)`. After that, only new items get the marker.

### `download`

1. Choose a course. Type to filter.
2. Select files with space (→ selects all, ← selects none). The list contains course files and the files attached to assignments.
3. Choose where to save in the file chooser. For several files the tool asks the portal for all locations at once; if your backend can't do that, it asks once per file.
4. Answer whether you want to download other files.

Files you already downloaded show `(downloaded DD.MM → path)`. If the file changed on Moodle since then, they show `(updated)` instead.

## Data

Everything is stored in a SQLite database at `~/.local/share/moodle-cli/moodle-cli.db`:

- `todo_items`: every due item seen by `status`, with first and last seen times.
- `downloads`: every saved file, with its URL, save path and download time.

```sh
sqlite3 ~/.local/share/moodle-cli/moodle-cli.db 'select filename, saved_path from downloads'
```

## Development

```sh
cargo test                 # unit tests (no network)
prek install               # install pre-commit and commit-msg hooks
prek run --all-files       # rustfmt, clippy, rust-analyzer diagnostics, tests
```

Commits follow [Conventional Commits](https://www.conventionalcommits.org/). The commit-msg hook checks this with [cocogitto](https://github.com/cocogitto/cocogitto). Releases are made with `cog bump --auto`, which updates `Cargo.toml` and `CHANGELOG.md` and creates a tag.
