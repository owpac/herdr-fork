# herdr-fork

A [herdr](https://herdr.dev) plugin that forks the Claude Code or Codex session of the focused pane into a new split or a new tab.

The fork gets a new session ID. The original conversation stays in its pane, unchanged. Use a fork to try another approach, or to ask a side question, without a change to the main conversation.

## Actions

| Action | Result |
|--------|--------|
| `herdr-fork.right` | Fork into a new split on the right of the pane |
| `herdr-fork.down` | Fork into a new split below the pane |
| `herdr-fork.tab` | Fork into a new tab of the workspace |

The fork starts in the folder of the agent, with the herdr agent name `fork-<start of the session ID>`. If this name is taken, the plugin adds the first free suffix: `-2`, `-3`...

| Agent | Fork command |
|-------|--------------|
| Claude Code | `claude --resume <session ID> --fork-session --name <fork name>` |
| Codex | `codex fork <session ID>` |

herdr types the command in the shell of the new pane, so your shell aliases of `claude` and `codex` apply. If the plugin cannot fork, a herdr notification gives the reason.

## Requirements

- herdr 0.9.0 or later, on macOS or Linux.
- The herdr integration of each agent: `herdr integration install claude` and `herdr integration install codex`. The integration gives the session ID of the agent to herdr. A new session has an ID only after its first message.

## Installation

```sh
herdr plugin install owpac/herdr-fork
```

The install downloads the release binary for your machine, and checks it with the `SHA256SUMS` file of the release. If the release has no binary for your machine, the install builds the binary with cargo (Rust 1.88 or later).

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
HERDR_FORK_BUILD=source sh scripts/install.sh   # build bin/herdr-fork with cargo
herdr plugin link "$PWD"
```

`herdr plugin link` does not run the build step: run `scripts/install.sh` again after each change. The checks of the CI:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

## Release

1. Set the same version in `herdr-plugin.toml` and `Cargo.toml`, then push to `main`.
2. Push the tag `v<version>`.

The workflow `release` builds the binaries for Linux and macOS (`x86_64` and `aarch64`), and publishes them with `SHA256SUMS` in the GitHub release of the tag. `scripts/install.sh` downloads the binary of the manifest version.

The build and release method comes from [herdr-bar](https://github.com/jeffarese/herdr-bar) (MIT).

## License

[MIT](LICENSE)
