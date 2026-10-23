#!/bin/sh
# Fail unless every manifest carries the version of the release tag (vX.Y.Z).
set -eu
tag="${1#v}"
root="$(dirname "$0")/.."
found="package.json:$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$root/package.json")
src-tauri/tauri.conf.json:$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$root/src-tauri/tauri.conf.json")
src-tauri/Cargo.toml:$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/src-tauri/Cargo.toml" | head -1)
packaging/arch-native/PKGBUILD:$(sed -n 's/^pkgver=//p' "$root/packaging/arch-native/PKGBUILD")"
status=0
for entry in $found; do
  if [ "${entry#*:}" != "$tag" ]; then
    echo "${entry%%:*} tiene ${entry#*:}, el tag pide $tag" >&2
    status=1
  fi
done
[ "$status" -eq 0 ] && echo "Versión $tag coherente en todos los manifiestos"
exit "$status"
