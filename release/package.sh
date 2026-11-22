#!/usr/bin/env bash
# Builds Arcwise's release files for one target into $DIST: the installer and a zip with the same
# files, for running without installing. The release build runs it with these set:
#   TARGET    the target triple, such as x86_64-pc-windows-msvc
#   VERSION   the version being released, such as 0.11.0
#   BINARY    the built arcwise.exe
#   NOTICES   the license notices of the crates it is built with (THIRD-PARTY-LICENSES.html)
#   DIST      the folder the release files go in, which must end up holding exactly the names
#             the `release` answer's `files` lists for TARGET
# It runs from the repository's root. ISCC names Inno Setup's compiler, which the Windows runners
# have on the path; set it to run this elsewhere.
set -euo pipefail

: "${TARGET:?}" "${VERSION:?}" "${BINARY:?}" "${NOTICES:?}" "${DIST:?}"

case "$TARGET" in
  # The names users, links and the in-app updater know: `x64`, not the target triple.
  x86_64-pc-windows-msvc) arch=x64 ;;
  *)
    echo "::error::release/package.sh has no release files for $TARGET" >&2
    exit 1
    ;;
esac

staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT

# What the installer installs and the zip holds, side by side at the top level.
cp "$BINARY" "$staging/arcwise.exe"
cp "$NOTICES" "$staging/THIRD-PARTY-LICENSES.html"
cp LICENSE-MIT "$staging/LICENSE-MIT.txt"
cp LICENSE-APACHE "$staging/LICENSE-APACHE.txt"

mkdir -p "$DIST"
# Git Bash would rewrite ISCC's `/` options as paths; Windows paths go in as defines.
MSYS_NO_PATHCONV=1 "${ISCC:-iscc}" /Qp \
  "/DAppVersion=$VERSION" \
  "/DStaging=$(cygpath -w "$staging")" \
  "/DOutputDir=$(cygpath -w "$DIST")" \
  "$(cygpath -w release/arcwise.iss)"

# Compress-Archive, as the releases before the move to trellis did.
pwsh -NoProfile -NonInteractive -Command \
  "Compress-Archive -Path '$(cygpath -w "$staging")\\*' -DestinationPath '$(cygpath -w "$DIST")\\arcwise-$VERSION-$arch.zip'"
