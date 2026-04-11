<p align="left">
  <img src="kingi.svg" alt="kingi" width="320"/>
</p>

[![Rust](https://github.com/acerv/kingi/actions/workflows/rust.yml/badge.svg)](https://github.com/acerv/kingi/actions)

A fast, Vim-inspired terminal email client for Maildir folders, written in Rust.

![demo](demo.gif)

## Installation

```sh
cargo install --path .
```

## Features

- Threaded email list with tree indentation
- Diff/patch syntax highlighting in email bodies
- Reply, reply-all with quoting, forward, compose in `$VISUAL`/`$EDITOR`
- Drafts and Trash mailboxes (auto-created if not configured)
- Read/unread tracking, flagging, live search, unread-only filter
- Multiple mailboxes with sidebar, tab-based email viewing
- Move emails or entire threads between mailboxes
- SMTP sending via STARTTLS
- Periodic background sync via a configurable shell command (e.g. `mbsync`)

## Configuration

Create `~/.config/kingi/config.toml`:

```toml
[smtp]
host     = "smtp.gmail.com"
port     = 587
name     = "Name Surname"
username = "you@example.com"
password = "app-password"

[[mailbox]]
label = "INBOX"
path  = "/home/you/Mail/INBOX/"

# "Drafts" and "Trash" labels are treated specially.
# If omitted, defaults are created under ~/.config/kingi/.
[[mailbox]]
label = "Drafts"
path  = "/home/you/Mail/Drafts/"

[[mailbox]]
label = "Trash"
path  = "/home/you/Mail/Trash/"

# Optional: run a shell command every N seconds to sync mail.
[sync]
command  = "mbsync -a"
interval = 60
```

A plain-text `~/.config/kingi/signature` file, if present, is appended to every draft.

## Key bindings

### Thread list

| Key               | Action                                   |
| ----------------- | ---------------------------------------- |
| `j`/`k`/`↑`/`↓`   | Move selection                           |
| `Ctrl+D`/`Ctrl+U` | Page down / up                           |
| `g` / `G`         | First / last email                       |
| `J` / `K`         | Next / previous mailbox                  |
| `Enter`           | Open email (in Drafts: reopen in editor) |
| `r` / `R`         | Reply-all / reply-all quoted             |
| `f`               | Forward                                  |
| `C`               | Compose new email                        |
| `v`               | Toggle read / unread                     |
| `Space`           | Toggle flagged                           |
| `m` / `M`         | Move email / move entire thread          |
| `D`               | Delete email                             |
| `s`               | Toggle sort order                        |
| `N`               | Toggle unread-only filter                |
| `/` / `Esc`       | Search by subject / clear search         |
| `Ctrl+S`          | Force sync                               |
| `Q`               | Quit                                     |

### Email tab

| Key               | Action                   |
| ----------------- | ------------------------ |
| `j`/`k`/`↑`/`↓`   | Scroll line              |
| `Ctrl+D`/`Ctrl+U` | Page down / up           |
| `g` / `G`         | Top / bottom             |
| `J` / `K`         | Next / previous email    |
| `Y`               | Copy body to clipboard   |
| `r` / `R`         | Reply / reply quoted     |
| `f`               | Forward                  |
| `m` / `M`         | Move email / move thread |
| `D`               | Delete and close tab     |
| `q`               | Close tab                |

### Compose

| Key      | Action                                |
| -------- | ------------------------------------- |
| `Ctrl+Q` | Send dialog                           |
| `j`/`k`  | Navigate dialog options               |
| `Enter`  | Confirm (Send / Save draft / Discard) |
| `Esc`    | Back to editor                        |

### Global

| Key                 | Action              |
| ------------------- | ------------------- |
| `Ctrl+N` / `Ctrl+P` | Next / previous tab |
