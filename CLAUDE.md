# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

`moodle-cli` is a Rust CLI for any Moodle site with the mobile web service enabled. It has two subcommands:

- `status`: lists open work (assignments, quizzes, other actionable calendar events). Items not seen before are marked with a red `(new!)`.
- `download`: an interactive picker. Choose a course, then multi-select its files. One xdg-desktop-portal FileChooser picks a folder and all files are saved there, then the tool asks "Download other files?".

Both subcommands cover in-progress courses by default. The global `--favourite` flag limits them to favourited courses.

## Commands

```sh
cargo run -- status [--days N] [--favourite]   # N defaults to 21
cargo run -- download [--favourite]
cargo test                                      # unit tests, no network
prek run --all-files                            # fmt, clippy, rust-analyzer, tests
```

- Site URL and token come from `~/.config/moodle-cli/config.toml`; the global `--url`/`--token` flags override it (`src/config.rs`). If the file is missing, a template (mode 0600) and token instructions are printed and the program exits with status 1, unless both flags were given. For manual tests, point `XDG_CONFIG_HOME` and `XDG_DATA_HOME` at a temp dir so the real config and database stay untouched.
- Every run makes live API calls; there is no mock mode. `download` needs a TTY and a running portal.
- Colours are only emitted when stdout is a TTY (`fmt::COLOR`). Interactive prompts use `inquire`, styled in `download::render_config`.

## Layout

- `src/config.rs`: loads `config.toml`, merges the flags, validates the URL (http/https, trailing `/` removed), and writes the first-run template.
- `src/api.rs`: `Client::call` POSTs to `<url>/webservice/rest/server.php`. `Params` flattens lists into `name[i]`; a response with an `exception` key becomes an error. It also holds the typed wrappers and response structs. `download()` adds `token=` to file URLs and rewrites `/pluginfile.php` to `/webservice/pluginfile.php`.
- `src/status.rs`: a port of the original Python script. Course names are shortened to the part before the first `" - "`. Assignments due in `[now - 30d, now + days]` get `mod_assign_get_submission_status` (one request each). Calendar action events are merged in, skipping `assign` events already covered (the `seen` set holds assignment `id` and `cmid`) and non-actionable ones.
- `src/download.rs`: the picker flow. Files come from `core_course_get_contents` (modules with `contents[type == "file"]`) and from assignment `introattachments`. Already-downloaded files are tagged `(downloaded …)`, or `(updated)` if Moodle's `timemodified` is newer.
- `src/portal.rs`: `pick_folder` opens `OpenFile` with `directory=true`. Cancelling saves nothing. `download::target_paths` keeps only the last component of each filename, overwrites existing files, and adds ` (2)`, ` (3)`, … when names repeat within a batch.
- `src/db.rs`: SQLite in `~/.local/share/moodle-cli/moodle-cli.db`. Migrations are keyed on `PRAGMA user_version`; append new ones to `MIGRATIONS`, never edit old ones.
  - `todo_items`: key `assign:<id>` / `event:<id>`, with `first_seen` and `last_seen`.
  - `downloads`: one row per saved file (URL, timemodified, path, time).
- `src/fmt.rs`: ANSI colours, relative dates and sizes.

## Conventions

- Commits follow Conventional Commits. The `commit-msg` hook runs `cog verify`, and `cog bump --auto` bumps the version and writes the changelog (see `cog.toml`).
- Hooks are configured in `.pre-commit-config.yaml` and run by `prek`. `scripts/rust-analyzer-check` fails on rust-analyzer errors and warnings.
