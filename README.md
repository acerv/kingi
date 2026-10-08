<p align="left">
  <img src="kingi.svg" alt="kingi" width="320"/>
</p>

[![Rust](https://github.com/acerv/kingi/actions/workflows/rust.yml/badge.svg)](https://github.com/acerv/kingi/actions)

A fast, Vim-inspired terminal email client for Maildir folders,
written in Rust. Heavily inspired by [aerc](https://aerc-mail.org/).

![demo](demo.gif)

## Installation

```sh
cargo install --path .
```

## Features

- Threaded email list with tree indentation
- Diff/patch syntax highlighting in email bodies
- Reply, reply-all with quoting, forward, built-in Vim-style compose editor
- Drafts and Trash mailboxes (auto-created if not configured)
- Read/unread tracking, flagging, live search, unread-only filter
- Multiple mailboxes with sidebar, tab-based email viewing
- Move emails or entire threads between mailboxes
- SMTP sending via STARTTLS
- Periodic background sync via a configurable shell command (e.g. `mbsync`)
- Quick reply templates bound to number keys (`1`-`9`, `0`)
- GPG decryption and signature verification (PGP/MIME and inline PGP)
- List received attachments and save them to disk

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
markers = true  # Enables patch status tracking (merged, reviewed, superseded)

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

# Optional: Case-insensitive regular expressions that mark a patch as merged.
# If omitted, kingi uses the default lore-cli regexes.
[status]
merged_markers = [
    # Line begins with the action verb, followed by "thanks", a target tree
    # ("to <tree>"), or end-of-statement — e.g. "Applied, thanks",
    # "Merged.", "Pushed to for-next".
    "\\b(applied|merged|pushed)(,?\\s+thanks|\\s+to\\s+\\S+|[.!]|\\s*$)",
    # "thanks, applied" / "thanks merged".
    "\\bthanks,?\\s+(applied|merged|pushed)\\b",
    # "patchset applied", "series merged" as a line-leading statement.
    "\\b(patch(set|es)?|series)\\s+(applied|merged)(,?\\s+thanks|\\s+to\\s+\\S+|[.!]?\\s*$)",
    # "Thanks ... merged" / "Thanks ... pushed"
    "\\b(T|t)hanks.*(merged|applied|pushed)",
]
```

A plain-text `~/.config/kingi/signature` file, if present, is appended to
every draft.

### Quick replies

Place template files named `reply-1` through `reply-9` and `reply-0` in the
config directory (`~/.config/kingi/`). Pressing the corresponding number key
on an email opens a reply pre-filled with that template's content.

For example, create `~/.config/kingi/reply-1`:

```
Thanks for the patch, applied!
```

Then press `1` on any email to reply with that text.

### GPG

Kingi automatically detects and decrypts PGP-encrypted emails and verifies
PGP-signed emails when opened. Both PGP/MIME (RFC 3156) and inline PGP
formats are supported. Encrypted emails are marked with a `⚷` icon in the
thread list.

A working `gpg-agent` is required for passphrase handling. To override the
GPG binary path:

```toml
[gpg]
binary = "gpg2"
```

### Received attachments

In an email tab, press `a` to list attachments with their names and sizes.
Use `j`/`k` or the arrow keys to select a file, then press `Enter` or `s`.
Edit the destination path and press `Enter` to save. The default path is
the attachment filename in the current working directory. Existing files
are never overwritten. Press `Esc` to cancel or close the list.

Attachments inside PGP/MIME encrypted emails are available after decryption.

## Key bindings

### Thread list

| Key               | Action                                   |
| ----------------- | ---------------------------------------- |
| `r` / `R`         | Reply-all / reply-all quoted             |
| `1`-`9`, `0`      | Quick reply with template                |
| `f`               | Forward                                  |
| `C`               | Compose new email                        |
| `/` / `Esc`       | Search by subject / clear                |
| `\` / `Esc`       | Search by sender / clear                 |
| `m` / `M`         | Move email / move entire thread          |
| `D`               | Delete email                             |
| `Enter`           | Open email (in Drafts: reopen in editor) |
| `v`               | Toggle read / unread                     |
| `Ctrl+f`          | Toggle flagged                           |
| `Ctrl+x`          | Toggle merged                            |
| `Ctrl+A`          | Mark all emails as read                  |
| `j`/`k`/`↑`/`↓`   | Move selection                           |
| `Ctrl+D`/`Ctrl+U` | Page down / up                           |
| `g` / `G`         | First / last email                       |
| `J` / `K`         | Next / previous mailbox                  |
| `s`               | Toggle sort order                        |
| `V`               | Toggle unread-only filter                |
| `F`               | Toggle flagged-only filter               |
| `Ctrl+S`          | Force sync                               |
| `?`               | Help                                     |
| `Q`               | Quit                                     |

### Email tab

| Key               | Action                    |
| ----------------- | ------------------------- |
| `r` / `R`         | Reply / reply quoted      |
| `1`-`9`, `0`      | Quick reply with template |
| `f`               | Forward                   |
| `Y`               | Copy body to clipboard    |
| `a`               | List / save attachments   |
| `m` / `M`         | Move email / move thread  |
| `Ctrl+f`          | Toggle flagged            |
| `Ctrl+x`          | Toggle merged             |
| `D`               | Delete and close tab      |
| `j`/`k`/`↑`/`↓`   | Scroll line               |
| `Ctrl+D`/`Ctrl+U` | Page down / up            |
| `g` / `G`         | Top / bottom              |
| `J` / `K`         | Next / previous email     |
| `?`               | Help                      |
| `q`               | Close tab                 |

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

## History

This project started with the aim of learning more about the Rust
programming language. Over the 2025 Christmas holidays, I set myself a
challenge: how to design an algorithm to load emails in Maildir format with
a maximum execution time of `O(n)`.

That challenge led to learning Rust's shared memory management, fighting
with the borrow checker, and achieving C-like performance. It was a lot
of fun, even if the borrow-checker struggle is very real.

Around March–April 2026, the emergence of more capable LLMs, such as
Opus 4.6, made it possible to speed up development and tackle a task I
hadn't been able to finish due to work and personal commitments: writing
a proper UI around that algorithm.

It worked, but with a caveat: LLMs produced tons of bad code. I had to
work through that, learning how to use these new tools effectively while
adding all the features I had always wanted in a terminal email client:
keybindings similar to `aerc`, fast loading speeds, and a workflow
tailored for mailing-list patch reviews (LTP and the Linux kernel in
particular).

In the end, it worked out. `kingi` became my primary terminal email
client: fast, easy to use, shaped around my workflow, and with unique
features no other client offered. This is the best email client I have ever
used, and I want to share it with everyone who is willing to try it out.
