#!/usr/bin/env bash
# Builds azura_<version>_amd64.deb from an already-compiled release binary.
#
# Usage:
#   packaging/linux/build-deb.sh <version> <binary> <out-dir>
#
# Example:
#   packaging/linux/build-deb.sh 1.0.2 target/release/azura dist
#
# The layout is the standard Debian one, assembled with plain `ar` and `tar`
# so a release needs no packaging tooling beyond what every Debian and Ubuntu
# machine already has.

set -euo pipefail

VERSION="${1:?usage: build-deb.sh <version> <binary> <out-dir>}"
BINARY="${2:?usage: build-deb.sh <version> <binary> <out-dir>}"
OUT_DIR="${3:?usage: build-deb.sh <version> <binary> <out-dir>}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARCH="amd64"
DEB="azura_${VERSION}_${ARCH}"

if [ ! -x "$BINARY" ]; then
  echo "not an executable: $BINARY" >&2
  exit 1
fi

STAGE="$(mktemp -d)"
cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

# --------------------------------------------------------------- install tree
install -Dm755 "$BINARY" "$STAGE/usr/bin/azura"
install -Dm644 "$REPO_ROOT/packaging/linux/azura.desktop" \
  "$STAGE/usr/share/applications/azura.desktop"
install -Dm644 "$REPO_ROOT/crates/azura/assets/icons/azura.svg" \
  "$STAGE/usr/share/icons/hicolor/scalable/apps/azura.svg"
install -Dm644 "$REPO_ROOT/README.md" "$STAGE/usr/share/doc/azura/README.md"
install -Dm644 "$REPO_ROOT/LICENSE" "$STAGE/usr/share/doc/azura/LICENSE"

# Raster sizes too, because not every panel asks the theme for a scalable
# icon. The master render is downscaled rather than committed as binaries, so
# the artwork stays editable in one place.
if command -v python3 >/dev/null 2>&1 && python3 -c 'import PIL' 2>/dev/null; then
  for size in 16 32 48 64 128 256; do
    dir="$STAGE/usr/share/icons/hicolor/${size}x${size}/apps"
    install -d "$dir"
    python3 - "$REPO_ROOT/crates/azura/assets/logo/azura.png" \
      "$dir/azura.png" "$size" <<'PY'
import sys
from PIL import Image
src, dst, size = sys.argv[1], sys.argv[2], int(sys.argv[3])
Image.open(src).convert("RGBA").resize((size, size), Image.LANCZOS).save(dst)
PY
  done
else
  echo "note: python3 with Pillow not found; shipping the scalable icon only" >&2
fi

# --------------------------------------------------------------- control file
install -d "$STAGE/DEBIAN"
INSTALLED_KB="$(du -ks "$STAGE" | cut -f1)"

# The shared objects the binary needs, read from the file rather than guessed
# so this cannot drift away from the build. A Debian `Depends` line wants
# package names, so each SONAME is mapped to the package that ships it.
package_for_soname() {
  case "$1" in
    libc.so.6|libm.so.6)                echo libc6 ;;
    libgcc_s.so.1)                      echo libgcc-s1 ;;
    libXau.so.6)                        echo libxau6 ;;
    libXdmcp.so.6)                      echo libxdmcp6 ;;
    libxcb.so.1)                        echo libxcb1 ;;
    libxcb-xkb.so.1)                    echo libxcb-xkb1 ;;
    libxkbcommon-x11.so.0)              echo libxkbcommon-x11-0 ;;
    libxkbcommon.so.0)                  echo libxkbcommon0 ;;
    *)                                  echo "" ;;   # unknown: leave it out
  esac
}

depends=""
while read -r soname; do
  case "$soname" in
    ld-linux*) continue ;;   # the interpreter, not a package
  esac
  package="$(package_for_soname "$soname")"
  if [ -z "$package" ]; then
    # An unmapped library means the binary grew a new dependency. Guessing
    # would leave a package that installs on a machine which cannot run it.
    echo "no Debian package known for $soname; refusing to guess" >&2
    exit 1
  fi
  case ",$depends," in
    *",$package,"*) continue ;;   # already listed
  esac
  depends="${depends:+$depends, }$package"
done <<EOF
$(objdump -p "$STAGE/usr/bin/azura" | awk '/NEEDED/ {print $2}' | sort -u)
EOF

if [ -z "$depends" ]; then
  echo "could not work out the dependencies; refusing to guess" >&2
  exit 1
fi

cat > "$STAGE/DEBIAN/control" <<EOF
Package: azura
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: Azura contributors
Installed-Size: $INSTALLED_KB
Depends: $depends
Description: Launch your apps and commands together, in order
 Azura is a small desktop launcher for Linux. A session is an ordered list of
 real applications, shell commands and links; pressing Run opens them one by
 one, with an optional pause after each one.
 .
 No accounts, no telemetry, no analytics. The only network request it makes is
 a daily check for a new version.
EOF

# The user's sessions live in ~/.config/Azura and are never ours to delete.
cat > "$STAGE/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
exit 0
EOF
chmod 755 "$STAGE/DEBIAN/postrm"

# --------------------------------------------------------------------- build
mkdir -p "$OUT_DIR"
OUT="$OUT_DIR/$DEB.deb"

if command -v dpkg-deb >/dev/null 2>&1; then
  dpkg-deb --root-owner-group --build "$STAGE" "$OUT" >/dev/null
else
  # Hand-assemble the `ar` archive. dpkg-deb is not present everywhere, and a
  # release must not depend on it.
  echo "dpkg-deb not found; assembling the archive by hand" >&2
  WORK="$STAGE/.ar"
  mkdir -p "$WORK"
  tar -C "$STAGE" -czf "$WORK/control.tar.gz" --owner=0 --group=0 --transform 's,^,./,' DEBIAN
  tar -C "$STAGE" -czf "$WORK/data.tar.gz" --owner=0 --group=0 --exclude=./DEBIAN .
  printf '2.0\n' > "$WORK/debian-binary"

  NOW="$(date -u '+%Y-%m-%d %H:%M:%S UTC')"
  ar_member() {
    # ar_member <name> <file>: one fixed-width header line, then the bytes.
    local name="$1" file="$2" size mtime uid gid mode
    size="$(stat -c%s "$file")"
    mtime="$(date -u -d "$NOW" +%s)"
    uid=0; gid=0; mode=100644
    printf '%-16s%-12s%-6s%-6s%-6s%-8s%-10s`\n' \
      "$name" "$NOW" "$uid" "$gid" "$mode" "$size" "$(basename "$file")"
    cat "$file"
  }
  {
    printf '!<arch>\n'
    ar_member "debian-binary" "$WORK/debian-binary"
    ar_member "control.tar.gz" "$WORK/control.tar.gz"
    ar_member "data.tar.gz" "$WORK/data.tar.gz"
  } > "$OUT"
fi

echo "built $OUT"