#!/usr/bin/env bash
# Generates pkg-config metadata for the WebKitGTK stack from the already-installed
# runtime libraries, so Tauri can be built without the -dev packages.
#
# Prefer installing the real packages when you have root:
#   sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
#
# Usage: eval "$(scripts/dev-webkit-pkgconfig.sh)"
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pkgdir="$root/.build/pkgconfig"
libdir="$root/.build/linkstubs"
syslib="/usr/lib/$(uname -m)-linux-gnu"

mkdir -p "$pkgdir" "$libdir"

# system-deps only reads versions and link flags, but rustc still needs an
# unversioned .so to link against; the -dev packages are what normally provide it.
emit() {
  local name="$1" version="$2" lib="$3" requires="$4"
  local sofile
  sofile="$(ls "$syslib/lib$lib.so."* 2>/dev/null | head -n1)" || true
  if [[ -z "$sofile" ]]; then
    echo "missing runtime library lib$lib.so in $syslib" >&2
    exit 1
  fi
  ln -sf "$sofile" "$libdir/lib$lib.so"
  cat >"$pkgdir/$name.pc" <<EOF
libdir=$libdir
Name: $name
Description: local development shim for $name
Version: $version
Requires: $requires
Libs: -L\${libdir} -l$lib
EOF
}

webkit_version="$(dpkg-query -f '${Version}' -W libwebkit2gtk-4.1-0 2>/dev/null | sed 's/-.*//')"
soup_version="$(dpkg-query -f '${Version}' -W libsoup-3.0-0 2>/dev/null | sed 's/-.*//')"

emit javascriptcoregtk-4.1 "${webkit_version:-2.44.0}" javascriptcoregtk-4.1 "glib-2.0, gobject-2.0"
emit libsoup-3.0 "${soup_version:-3.4.4}" soup-3.0 "glib-2.0, gio-2.0, gobject-2.0"
emit webkit2gtk-4.1 "${webkit_version:-2.44.0}" webkit2gtk-4.1 \
  "glib-2.0, gobject-2.0, gtk+-3.0, libsoup-3.0, javascriptcoregtk-4.1"

echo "export PKG_CONFIG_PATH=\"$pkgdir:\${PKG_CONFIG_PATH:-}\""
