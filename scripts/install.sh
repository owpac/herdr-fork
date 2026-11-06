#!/bin/sh
# Put the herdr-fork binary at bin/herdr-fork, for this checkout.
#
# herdr runs this script as the [[build]] step of `herdr plugin install` (an update is a new
# install). `herdr plugin link` does not run it: run it yourself for a local checkout.
#
# The script downloads the binary of the manifest version from the GitHub release, and checks it
# with the SHA256SUMS file of the release. The tag of a release is its version (YYYY.M.N). On main,
# the version is 0.0.0: no release has this version, so the script builds with cargo.
# If the release has no binary for this machine, or if the download or the check fails, the script
# also builds the binary with cargo (Rust 1.88 or later).
# The same method as the plugin herdr-bar (github.com/jeffarese/herdr-bar).
#
#   HERDR_FORK_BUILD=source   skip the download, and build with cargo
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) || exit 1
cd "$ROOT" || exit 1
REPOSITORY_URL="https://github.com/owpac/herdr-fork"
TARGET="$ROOT/bin/herdr-fork"

say() { printf 'herdr-fork: %s\n' "$*" >&2; }

version=$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' herdr-plugin.toml | head -n 1)
if [ -z "$version" ]; then
  say "cannot read the version in herdr-plugin.toml"
  exit 1
fi

# The name of the release binary for this machine: herdr-fork-<os>-<arch>.
asset_name() {
  case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    *) return 1 ;;
  esac
  arch=$(uname -m)
  # A shell under Rosetta shows x86_64 on Apple silicon: use the native binary.
  if [ "$os" = macos ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ]; then
    arch=arm64
  fi
  case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) return 1 ;;
  esac
  printf 'herdr-fork-%s-%s\n' "$os" "$arch"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

download() {
  asset=$(asset_name) || { say "no release binary for this machine"; return 1; }
  command -v curl >/dev/null 2>&1 || { say "curl is missing"; return 1; }
  base="$REPOSITORY_URL/releases/download/$version"
  work=$(mktemp -d) || return 1
  if ! curl -fsSL -o "$work/$asset" "$base/$asset" || ! curl -fsSL -o "$work/SHA256SUMS" "$base/SHA256SUMS"; then
    say "cannot download $asset of $version"
    rm -rf "$work"
    return 1
  fi
  expected=$(awk -v name="$asset" '$2 == name { print $1 }' "$work/SHA256SUMS")
  if [ -z "$expected" ] || [ "$expected" != "$(sha256_of "$work/$asset")" ]; then
    say "the checksum of $asset does not match SHA256SUMS"
    rm -rf "$work"
    return 1
  fi
  mkdir -p bin && mv "$work/$asset" "$TARGET" && chmod +x "$TARGET"
  rm -rf "$work"
  say "installed the release binary $asset of $version"
}

build_from_source() {
  command -v cargo >/dev/null 2>&1 || { say "cargo is missing: install Rust 1.88 or later"; return 1; }
  cargo build --release --locked || return 1
  mkdir -p bin && cp target/release/herdr-fork "$TARGET" && chmod +x "$TARGET"
  say "built herdr-fork $version with cargo"
}

if [ "${HERDR_FORK_BUILD:-}" != source ] && [ "$version" != 0.0.0 ] && download; then
  exit 0
fi
build_from_source
