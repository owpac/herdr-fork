#!/bin/sh
# Update the pull request "chore: release <version>", like the release of jdx/mise.
#
# The workflow release-pr runs this script on main, after each push to main and each day. The
# script finds the commits of main after the last release. If there are commits, it writes the
# next version in herdr-plugin.toml, Cargo.toml and Cargo.lock, and the section of this version
# in CHANGELOG.md (git-cliff, cliff.toml). Then it pushes the branch release, and opens or updates
# its pull request. The merge of this pull request puts the version on main: the workflow release
# then tags this commit of main and publishes the release.
#
# The version is YYYY.M.N: the month in UTC, and the number of the release in this month. The
# month has no leading zero: Cargo and herdr read the version as SemVer.
#
# A pull request of GITHUB_TOKEN starts no workflow. Thus the script starts the workflow ci on
# the branch release itself: the ruleset of main needs its checks.
#
#   RELEASE_PR_DRY_RUN=1   print the version and the changelog, and change nothing
set -eu

say() { printf 'release-pr: %s\n' "$*" >&2; }

manifest_version() {
  sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' herdr-plugin.toml | head -n 1
}

# Write the version $1 in the first version line of herdr-plugin.toml and Cargo.toml, and in the
# package herdr-fork of Cargo.lock.
write_version() {
  for file in herdr-plugin.toml Cargo.toml; do
    awk -v version="$1" '
      !done && /^version = "/ { $0 = "version = \"" version "\""; done = 1 }
      { print }
    ' "$file" > "$file.new"
    mv "$file.new" "$file"
  done
  awk -v version="$1" '
    previous == "name = \"herdr-fork\"" && /^version = / { $0 = "version = \"" version "\"" }
    { print; previous = $0 }
  ' Cargo.lock > Cargo.lock.new
  mv Cargo.lock.new Cargo.lock
}

current=$(manifest_version)
# The merge of the release pull request: the workflow release publishes this version now.
if [ "$current" != 0.0.0 ] && [ -z "$(git tag --list "$current")" ]; then
  say "the release $current has no tag yet: the workflow release publishes it"
  exit 0
fi

# The last release: the highest tag YYYY.M.N.
last=$(git tag --list '[0-9][0-9][0-9][0-9].*.*' | sort -t . -k1,1n -k2,2n -k3,3n | tail -n 1)
if [ -n "$last" ]; then
  set -- "$last..HEAD"
else
  set --
fi

# The commits to release: the commits of main after the last release, except the release commits.
if [ -z "$(git log --format=%s "${1:-HEAD}" | grep -v '^chore: release ' || true)" ]; then
  say "no commit after the release ${last:-(none)}: no release pull request"
  exit 0
fi

month=$(date -u +%Y.%-m)
number=$(git tag --list "$month.*" | sed "s/^$month\.//" | sort -n | tail -n 1)
version="$month.$((${number:-0} + 1))"
notes=$(git cliff "$@" --tag "$version" --unreleased --strip all)

if [ "${RELEASE_PR_DRY_RUN:-}" = 1 ]; then
  say "the next release is $version, after ${last:-(none)}"
  printf '%s\n' "$notes"
  exit 0
fi

write_version "$version"
git cliff "$@" --tag "$version" --unreleased --prepend CHANGELOG.md
# git-cliff puts no blank line between 2 releases: add one before each title of a release.
awk 'previous != "" && /^## / { print "" } { print; previous = $0 }' CHANGELOG.md > CHANGELOG.md.new
mv CHANGELOG.md.new CHANGELOG.md

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git checkout --quiet -B release
git add herdr-plugin.toml Cargo.toml Cargo.lock CHANGELOG.md
git commit --quiet -m "chore: release $version"
# The branch release belongs to this script: each run writes it again from main.
git push --quiet --force origin release

title="chore: release $version"
body=$(printf 'Merge this pull request to release %s. The workflow release-pr updates it after each push to main, and each day.\n\n%s\n' "$version" "$notes")
pr=$(gh pr list --head release --base main --state open --json number --jq '.[0].number // empty')
if [ -z "$pr" ]; then
  gh pr create --base main --head release --title "$title" --body "$body"
else
  gh pr edit "$pr" --title "$title" --body "$body"
fi
gh workflow run ci.yml --ref release
say "the pull request releases $version"
