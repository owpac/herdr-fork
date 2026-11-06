# herdr-fork

A [herdr](https://herdr.dev) plugin that forks the Claude Code or Codex session of the focused pane into a new split or a new tab.

The fork gets a new session ID. The original conversation stays in its pane, unchanged. Use a fork to try another approach, or to ask a side question, without a change to the main conversation.

## Actions

| Action | Result |
|--------|--------|
| `herdr-fork.right` | Fork into a new split on the right of the pane |
| `herdr-fork.down` | Fork into a new split below the pane |
| `herdr-fork.tab` | Fork into a new tab of the workspace |

The fork starts in the folder of the agent. It gets 2 names:

| Name | Example | Where |
|------|---------|-------|
| Title: `[F<n>] <title of the conversation>` | `[F1] Parser fix` | The session name of Claude Code (prompt box, `/resume`, terminal title), and the label of a fork in a tab |
| herdr agent name: `f<n>-<title in lowercase>` | `f1-parser-fix` | `herdr agent prompt`, `wait` and `read`. herdr accepts only `[a-z][a-z0-9_-]`, 32 characters or less |

The forks of one conversation get the numbers 1, 2, 3...: the first number that no live agent uses. A fork of `[F1] Parser fix` is `[F2] Parser fix`: it belongs to the same conversation.

A number becomes free when its fork closes: the next fork can get it again. Thus 2 closed conversations can have the same title in `/resume`.

| Agent | Fork command |
|-------|--------------|
| Claude Code | `claude --resume <session ID> --fork-session --name <fork title>` |
| Codex | `codex fork --no-daemon <session ID>` |

herdr types the command in the shell of the new pane, so your shell aliases of `claude` and `codex` apply. If the plugin cannot fork, a herdr notification gives the reason.

### Codex and its shared daemon

Codex 0.166 runs its sessions on a shared background server, the daemon. The daemon runs the hooks of Codex with the environment of the pane that started the daemon. Thus the herdr hook gives the session ID to this other pane, and herdr knows no session in the pane of Codex.

Start Codex with `codex --no-daemon` in a herdr pane. For example, add this line to `~/.zshrc`:

```zsh
[[ -n "$HERDR_ENV" ]] && alias codex='codex --no-daemon'
```

The plugin starts each Codex fork with `--no-daemon`, so a fork can fork again. Without the daemon, `codex agents` does not show the sessions of herdr, and a session stops when its pane closes.

## Life of a fork

A fork is an independent conversation. The plugin keeps no link and no state between the fork and the original conversation.

- To close a fork, close its pane. The conversation stays on the disk: `claude --resume "[F1] Parser fix"` opens it again.
- After a restart of herdr, the herdr integration resumes the fork with its own session ID.
- The agent deletes its old conversations itself (for Claude Code: the setting `cleanupPeriodDays`).

## Requirements

- herdr 0.9.0 or later, on macOS or Linux.
- The herdr integration of each agent: `herdr integration install claude` and `herdr integration install codex`. The integration gives the session ID of the agent to herdr. A new session has an ID only after its first message. For Codex, see [Codex and its shared daemon](#codex-and-its-shared-daemon).

## Installation

Install a release: give its tag, from the [releases](https://github.com/owpac/herdr-fork/releases) page.

```sh
herdr plugin install owpac/herdr-fork --ref 2026.11.1
```

The install downloads the release binary for your machine, and checks it with the `SHA256SUMS` file of the release. If the release has no binary for your machine, the install builds the binary with cargo (Rust 1.88 or later).

Without `--ref`, herdr installs the last commit of `main`. Its version is `0.0.0`, so the install always builds with cargo.

To update, install the tag of the new release. `herdr plugin update` fetches the same tag again.

## Keybindings

Add `[[keys.command]]` entries to `~/.config/herdr/config.toml`, then run `herdr server reload-config`:

```toml
[[keys.command]]
key = "prefix+ctrl+d"
type = "plugin_action"
command = "herdr-fork.right"
description = "fork the agent into a split on the right"

[[keys.command]]
key = "prefix+ctrl+shift+d"
type = "plugin_action"
command = "herdr-fork.down"
description = "fork the agent into a split below"

[[keys.command]]
key = "prefix+ctrl+t"
type = "plugin_action"
command = "herdr-fork.tab"
description = "fork the agent into a new tab"
```

From a shell, `herdr plugin action invoke right --plugin herdr-fork` forks the focused pane. The binary also takes a pane: `bin/herdr-fork right --pane w1:p2`.

## Development

```sh
git clone https://github.com/owpac/herdr-fork
cd herdr-fork
mise install                                    # Rust and shellcheck of mise.toml
HERDR_FORK_BUILD=source sh scripts/install.sh   # build bin/herdr-fork with cargo
herdr plugin link "$PWD"
```

`herdr plugin link` does not run the build step: run `scripts/install.sh` again after each change. The checks of the CI:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
shellcheck scripts/*.sh
```

## Release

Each push on `main` makes a release, like the tools [pac](https://github.com/owpac/pac), kompose and craft of owpac. The version is `YYYY.M.N`: the month of the push in UTC, and the number of the release in this month (for example `2026.11.1`). The month has no leading zero, because Cargo and herdr read the version as SemVer.

On `main`, `herdr-plugin.toml` and `Cargo.toml` keep the version `0.0.0`. For each push, the workflow `release`:

1. Writes the version in `herdr-plugin.toml`, `Cargo.toml` and `Cargo.lock`, in a release commit on top of the commit of `main`.
2. Pushes the tag of the version, on this release commit. `main` does not change.
3. Builds the binaries for Linux and macOS (`x86_64` and `aarch64`), and publishes them with `SHA256SUMS` in the GitHub release of the tag.

Install a tag, not a commit of `main`: `herdr plugin install owpac/herdr-fork --ref 2026.11.1`. On `main`, the version `0.0.0` has no release, so `scripts/install.sh` builds with cargo.

The build and release method comes from [herdr-bar](https://github.com/jeffarese/herdr-bar) (MIT).

## License

[MIT](LICENSE)
