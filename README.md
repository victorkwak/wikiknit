# wikiknit

A linter for the `[[wikilinks]]` in a Markdown vault (Obsidian and others) that AI agents keep as
their memory.

[![CI](https://github.com/victorkwak/wikiknit/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/victorkwak/wikiknit/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/wikiknit.svg)](https://crates.io/crates/wikiknit)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/wikiknit.svg)](#license)

## Why

A vault misleads the agent that reads it when its links are wrong: a link to a note that moved, a
resolved issue still listed as open, a note that names another note but does not link it.
wikiknit finds these, and gives the fix for each. It reads the files directly (Obsidian does not
need to run) and never edits notes: the agent does, with the fix wikiknit gives.

An example vault. `INDEX.md`:

```markdown
# Index

- [[Projects/gatehouse/overveiw]]

## Open issues

- [[Issues/resolved/login-timeout]]
```

`Projects/gatehouse/overview.md` says "The login timeout was fixed in March." and has no link to
the issue. With a rule that a resolved issue must not be listed as open:

```console
$ wikiknit check
# note	line	severity	rule	message	fix
INDEX	3	error	unresolved-link	[[Projects/gatehouse/overveiw]] points to no note or file	did you mean [[Projects/gatehouse/overview]]
INDEX	7	error	forbidden-link	a resolved issue is listed as open	remove the link to [[Issues/resolved/login-timeout]], or move it where it belongs
Projects/gatehouse/overview	1	warn	orphan	no note links here	link it from a related note, or add its folder to `orphan_exempt`

$ wikiknit mentions
# note	line	term	target	count	context	fix
Projects/gatehouse/overview	4	login timeout	Issues/resolved/login-timeout	1	The login timeout was fixed in March.	if it means [[Issues/resolved/login-timeout]]: on line 4, write `[[Issues/resolved/login-timeout|login timeout]]` in place of `login timeout`
```

After the agent applies the fixes, both commands print nothing.

## Features

- **Broken links**, with the closest existing notes as the fix.
- **Link style**: full paths or shortest names, as the vault uses.
- **Link rules** you write: links a note must have, links a note must get, links that must not
  be written.
- **Unlinked mentions**: text that names another note (by its name or an alias) without a link.
- **Stale notes**: a date field older than a limit you set.
- **Drift**: a note that describes a git repository, and the commits made there since the note
  was checked.
- **Other defects**: orphans, notes with no outgoing link, links hidden in code or HTML, unquoted
  links in frontmatter, aliases two notes share.
- **Only what a change adds**: `check --changed` and `check --staged` compare with the last
  commit. A git pre-commit hook blocks a commit only when it adds an error.
- **Output for agents**: tab-separated records, or JSON with `--format json`. A clean check
  prints nothing.
- **Deterministic**: each check finds a real defect or stays quiet. No search, no guesses.

## Install

Prebuilt binary for Linux (x86_64, aarch64; static) or macOS (arm64, x86_64), with a SHA-256
check, into `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/victorkwak/wikiknit/main/install.sh | sh
```

`WIKIKNIT_VERSION=v0.8.5` pins a release; `WIKIKNIT_INSTALL_DIR` picks the folder.

With Rust:

```bash
cargo install --locked wikiknit     # build from crates.io
cargo binstall wikiknit             # the release binary, via cargo-binstall
```

Windows (`.zip`), and every other release archive: the
[releases page](https://github.com/victorkwak/wikiknit/releases).

## Quick start

Ask your agent: "Set up wikiknit for this vault, following
https://github.com/victorkwak/wikiknit/blob/main/docs/agent-install.md"

Or do it yourself, from the project that holds the vault:

```bash
wikiknit init --dry-run              # what it would do
wikiknit init                        # wikiknit.toml + git hooks; a second run changes nothing
wikiknit snippet --into AGENTS.md    # or CLAUDE.md: the instructions for agents
wikiknit check                       # every note; exit 1 on errors
```

`init` writes a starter `wikiknit.toml` with only what the notes show. A minimal one:

```toml
# wikiknit.toml
vault = "notes"            # the vault folder, relative to this file (default ".")
link_style = "full-path"   # or "shortest"
hubs = ["INDEX"]           # index notes
ignore = ["Templates/"]    # folders not checked
```

Then check your edits before you commit:

```bash
wikiknit check --changed      # only what the uncommitted changes add
wikiknit mentions --changed   # unlinked mentions in the changed notes
```

## Configure

`init` writes no rules: rules are decisions about the vault. Add them to `wikiknit.toml`. For
example, the rule from the example above:

```toml
[[forbid_link]]
target = "^Issues/resolved/"
near = "(?i)\\bopen\\b"
message = "a resolved issue is listed as open"
```

Every key and rule: [Configure](docs/reference.md#configure) in the reference.

## Claude Code plugin

This repository is also a Claude Code plugin marketplace. The plugin is optional: the
`wikiknit snippet` block in the project's `CLAUDE.md` and the git hook do the checking. It adds one
skill, `link`: how to link the notes an agent writes (by meaning, both ways, with aliases), and to
run `wikiknit check --changed` before committing. It adds no hooks.

```bash
claude plugin marketplace add victorkwak/wikiknit   # or a local checkout path
claude plugin install wikiknit@wikiknit
```

## Documentation

- [Reference](docs/reference.md): every command and flag, the output format, the git hook and
  CI, every config key, the rules, and how links resolve.
- [Agent setup](docs/agent-install.md): the setup steps, written for an agent.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at
your option.
