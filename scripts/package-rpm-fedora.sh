#!/usr/bin/env bash
# Build a Fedora RPM for pot from the binary compiled on a Fedora build host.
#
#   scripts/package-rpm-fedora.sh                       # auto-detects the binary
#   scripts/package-rpm-fedora.sh --target x86_64-unknown-linux-gnu
#   BIN=path/to/pot scripts/package-rpm-fedora.sh
#
# Why not let the Tauri 1 bundler produce the .rpm? Its dependency table in
# src-tauri/tauri.conf.json is written with Debian names (libxdo-dev, libxcb1,
# libxrandr2, tesseract-ocr) that no Fedora package provides, so dnf refuses to
# install the result. Here the runtime dependencies are read back from the
# binary's own DT_NEEDED entries and resolved to Fedora package names with
# `rpm -qf`, so the generated RPM always names packages that actually exist on
# Fedora (webkit2gtk4.1, libsoup3, gtk3, ...).
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."
root=$PWD

target=""
while [[ $# -gt 0 ]]; do
  case $1 in
    --target) target=$2; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ $(uname -s) == Linux ]] || { echo 'Build this package on Linux.' >&2; exit 1; }
command -v rpmbuild >/dev/null || { echo 'Install rpm-build.' >&2; exit 1; }

version=$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['package']['version'])")
arch=$(uname -m)

if [[ -n ${BIN:-} ]]; then
  binary=$BIN
else
  candidates=()
  [[ -n $target ]] && candidates+=("src-tauri/target/$target/release/pot")
  candidates+=("src-tauri/target/release/pot")
  for c in "${candidates[@]}"; do
    [[ -x $c ]] && { binary=$c; break; }
  done
fi
[[ -n ${binary:-} && -x ${binary:-} ]] || { echo "missing pot binary; build first" >&2; exit 1; }
echo "packaging $binary ($version, $arch)"

# --- dependency table -------------------------------------------------------
# Every DT_NEEDED soname is mapped to the Fedora package that owns it. glibc
# and libgcc are covered by rpm's automatic dependency generator, so they are
# skipped to keep the Requires: line readable.
declare -A deps=()
sonames=$(readelf -d "$binary" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p')
for soname in $sonames; do
  # No `exit` in the awk: closing the pipe early makes ldconfig die of
  # SIGPIPE, which `set -o pipefail` would turn into a fatal 141.
  path=$(ldconfig -p | awk -v s="$soname" '$1 == s && !seen++ {print $NF}')
  [[ -n $path ]] || { echo "warning: $soname is not installed on this host" >&2; continue; }
  owner=$(rpm -qf --qf '%{NAME}\n' "$(readlink -f "$path")" 2>/dev/null |
    sed -n '1p' || true)
  case $owner in
    ''|glibc|libgcc|libstdc++) continue ;;
    *) deps[$owner]=1 ;;
  esac
done
# Runtime pieces that are not in DT_NEEDED:
#   * libappindicator-sys dlopen()s one of libayatana-appindicator3.so.1 /
#     libappindicator3.so.1 for the tray icon, so the linker never sees it;
#   * OCR shells out to the `tesseract` binary.
deps[libayatana-appindicator-gtk3]=1
deps[tesseract]=1

requires=$(printf '%s\n' "${!deps[@]}" | sort | tr '\n' ' ' | sed 's/ $//')
echo "Requires: $requires"

# --- staging ----------------------------------------------------------------
topdir=$(mktemp -d)
trap 'rm -rf "$topdir"' EXIT
mkdir -p "$topdir"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
stage=$topdir/stage
install -d "$stage/usr/bin" "$stage/usr/share/applications" \
           "$stage/usr/share/icons/hicolor/512x512/apps" \
           "$stage/usr/share/doc/pot" "$stage/usr/share/metainfo" \
           "$stage/usr/share/licenses/pot"

install -m755 "$binary" "$stage/usr/bin/pot"
strip --strip-all "$stage/usr/bin/pot" 2>/dev/null || true
install -m644 src-tauri/icons/icon.png \
  "$stage/usr/share/icons/hicolor/512x512/apps/pot.png"
install -m644 LICENSE "$stage/usr/share/licenses/pot/LICENSE"
files='%{_bindir}/pot
/usr/share/applications/pot.desktop
/usr/share/icons/hicolor/512x512/apps/pot.png
/usr/share/licenses/pot/LICENSE'
if [[ -f com.pot_app.pot.metainfo.xml ]]; then
  install -m644 com.pot_app.pot.metainfo.xml \
    "$stage/usr/share/metainfo/com.pot_app.pot.metainfo.xml"
  files+=$'\n/usr/share/metainfo/com.pot_app.pot.metainfo.xml'
fi

cat > "$stage/usr/share/applications/pot.desktop" <<'DESK'
[Desktop Entry]
Type=Application
Name=Pot
Comment=A cross-platform text translation and OCR app
Comment[zh_CN]=跨平台划词翻译和 OCR 软件
Exec=pot
Icon=pot
Terminal=false
Categories=Utility;Translation;
Keywords=translate;ocr;dictionary;
StartupWMClass=pot
DESK

cp -a "$stage/." "$topdir/SOURCES/payload"

# --- spec -------------------------------------------------------------------
cat > "$topdir/SPECS/pot.spec" <<SPEC
Name:       pot
Version:    $version
Release:    1%{?dist}
Summary:    Cross-platform text translation and OCR app
License:    GPL-3.0-only
URL:        https://pot-app.com
Source0:    %{name}-payload.tar.gz
BuildArch:  $arch
AutoReqProv: no

Requires:   $requires
Recommends: tesseract-langpack-eng
Recommends: tesseract-langpack-chi_sim
# No CJK font is recommended on purpose: every desktop that shows this app
# already ships a Chinese font, and google-noto-sans-cjk-fonts alone would add
# ~125 MiB to the transaction. Users who want it can install it themselves.

%description
Pot is a cross-platform translation and OCR tool built on Tauri. This build
links WebKitGTK 4.1 (libsoup3), so it runs on Fedora 40 and newer.

%prep

%build

%install
mkdir -p %{buildroot}
cp -a %{_sourcedir}/payload/. %{buildroot}/

%files
$files

%changelog
* $(LC_ALL=C date '+%a %b %d %Y') pot-app contributors - $version-1
- Fedora build: WebKitGTK 4.1 runtime dependencies derived from DT_NEEDED
SPEC

tar -czf "$topdir/SOURCES/pot-payload.tar.gz" -C "$topdir/SOURCES" payload

rpmbuild --define "_topdir $topdir" --define "_sourcedir $topdir/SOURCES" \
         --define "debug_package %{nil}" --define "_build_id_links none" \
         -bb "$topdir/SPECS/pot.spec" > "$topdir/rpmbuild.log" 2>&1 || {
  tail -n 30 "$topdir/rpmbuild.log"; exit 1; }

outdir=$root/target/rpm
mkdir -p "$outdir"
# Only this architecture's package: a leftover from another matrix leg would
# otherwise be picked up by the glob below.
find "$outdir" -maxdepth 1 -name "pot-*-1*.$arch.rpm" -delete
find "$topdir/RPMS" -name "*.rpm" -exec cp {} "$outdir/" \;
rpmfile=$(find "$outdir" -maxdepth 1 -name "*.rpm" | head -1)
# An unreadable file here means the copy above went wrong; stop before the
# checksum records it as a real package.
rpm -qp --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' "$rpmfile" >/dev/null
(cd "$outdir" && sha256sum "$(basename "$rpmfile")" > "$(basename "$rpmfile").sha256")
rpm -qp --requires "$rpmfile"
echo "$rpmfile"
