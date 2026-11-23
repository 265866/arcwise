#!/usr/bin/env bash
# Stages the files Arcwise's installer installs for one target in $STAGING, and writes the zip of
# the same files, for running without installing, into $DIST. The release build then builds the
# installer from release/installer.iss into $DIST itself. It runs this with these set:
#   TARGET    the target triple, such as x86_64-pc-windows-msvc
#   VERSION   the version being released, such as 0.11.0
#   BINARY    the built arcwise.exe
#   NOTICES   the license notices of the crates it is built with (THIRD-PARTY-LICENSES.html)
#   STAGING   the empty folder of files the installer installs, all of them
#   DIST      the folder the release files go in, which must end up holding exactly the names
#             the `release` answer's `files` lists for TARGET, the installer included
# It runs from the repository's root.
set -euo pipefail

: "${TARGET:?}" "${VERSION:?}" "${BINARY:?}" "${NOTICES:?}" "${STAGING:?}" "${DIST:?}"

case "$TARGET" in
  # The names users, links and the in-app updater know: `x64`, not the target triple.
  x86_64-pc-windows-msvc) arch=x64 ;;
  *)
    echo "::error::release/package.sh has no release files for $TARGET" >&2
    exit 1
    ;;
esac

# What the installer installs and the zip holds, side by side at the top level.
cp "$BINARY" "$STAGING/arcwise.exe"
cp "$NOTICES" "$STAGING/THIRD-PARTY-LICENSES.html"
cp LICENSE-MIT "$STAGING/LICENSE-MIT.txt"
cp LICENSE-APACHE "$STAGING/LICENSE-APACHE.txt"

mkdir -p "$DIST"
# Compress-Archive, as the releases before the move to trellis did.
pwsh -NoProfile -NonInteractive -Command \
  "Compress-Archive -Path '$(cygpath -w "$STAGING")\\*' -DestinationPath '$(cygpath -w "$DIST")\\arcwise-$VERSION-$arch.zip'"
