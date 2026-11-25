#!/usr/bin/env bash
# Prepare a Fedora build host for Tauri 1.8, which still asks pkg-config for
# WebKitGTK 4.0 (removed on Fedora 40+).
#
# Fedora ships WebKitGTK 4.1 only, and its libdir is /usr/lib64. Two lookups
# have to be redirected:
#   * pkg-config names webkit2gtk-4.0 / javascriptcoregtk-4.0, provided as .pc
#     shims in a private directory added to PKG_CONFIG_PATH;
#   * the unversioned dev link names -lwebkit2gtk-4.0 / -ljavascriptcoregtk-4.0
#     that webkit2gtk-sys hard-codes, provided as symlinks to the 4.1 sonames.
#     The soname of the target stays libwebkit2gtk-4.1.so.0, so the produced
#     binary depends on the Fedora 4.1 runtime package, not on 4.0.
#
# Needs root only for the symlinks in the system libdir.
set -euo pipefail
export LC_ALL=C

shim_dir=${SHIM_DIR:-$PWD/.webkit41-shim}
libdir=$(pkg-config --variable=libdir webkit2gtk-4.1 2>/dev/null || true)
[[ -n $libdir ]] || libdir=/usr/lib64
includedir=$(pkg-config --variable=includedir webkit2gtk-4.1 2>/dev/null || true)
[[ -n $includedir ]] || includedir=/usr/include

webkit=$(pkg-config --modversion webkit2gtk-4.1)
jsc=$(pkg-config --modversion javascriptcoregtk-4.1)

mkdir -p "$shim_dir"

cat > "$shim_dir/webkit2gtk-4.0.pc" <<PC
prefix=/usr
exec_prefix=\${prefix}
libdir=$libdir
includedir=$includedir

Name: WebKit2GTK
Description: WebKitGTK 4.1 standing in for the removed 4.0 package
Version: $webkit
Requires: webkit2gtk-4.1
Libs: -L\${libdir} -lwebkit2gtk-4.1
Cflags: -I\${includedir}/webkitgtk-4.1
PC

cat > "$shim_dir/javascriptcoregtk-4.0.pc" <<PC
prefix=/usr
exec_prefix=\${prefix}
libdir=$libdir
includedir=$includedir

Name: JavaScriptCoreGTK
Description: JavaScriptCore 4.1 standing in for the removed 4.0 package
Version: $jsc
Requires: javascriptcoregtk-4.1
Libs: -L\${libdir} -ljavascriptcoregtk-4.1
Cflags: -I\${includedir}/webkitgtk-4.1
PC

for pair in "libwebkit2gtk-4.1.so.0 libwebkit2gtk-4.0.so" \
            "libjavascriptcoregtk-4.1.so.0 libjavascriptcoregtk-4.0.so"; do
  read -r target_name link_name <<< "$pair"
  target=$libdir/$target_name
  link=$libdir/$link_name
  [[ -e $target ]] || { echo "missing $target" >&2; exit 1; }
  if [[ -e $link || -L $link ]]; then
    echo "kept existing $link"
  else
    if [[ $(id -u) -eq 0 ]]; then
      ln -s "$target_name" "$link"
    else
      sudo ln -s "$target_name" "$link"
    fi
    echo "linked $link -> $target_name"
  fi
done

PKG_CONFIG_PATH="$shim_dir${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
  pkg-config --modversion webkit2gtk-4.0 javascriptcoregtk-4.0
echo "shim ready in $shim_dir (libdir=$libdir)"
