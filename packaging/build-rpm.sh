#!/bin/bash
# Build an RPM package for MyPowerToys using rpmbuild
set -euo pipefail

VERSION="${1:-0.1.0}"

echo "==> Building release binaries..."
# Whole workspace with default features, so every module's GUI binary is built.
cargo build --release --workspace

BINARIES=(
    mpt-daemon mpt-ctl mpt-settings
    mpt-app-launcher mpt-bulk-rename mpt-color-picker mpt-command-not-found
    mpt-command-palette mpt-crop-and-lock mpt-environment-variables
    mpt-fancy-zones mpt-file-locksmith mpt-hosts-editor mpt-image-resizer
    mpt-new-plus mpt-paste-plain mpt-peek mpt-power-display mpt-quick-accent
    mpt-screen-ruler mpt-shortcut-guide mpt-text-extractor mpt-workspaces
    mpt-zoom-it
)
INSTALL_LINES=""
FILES_LINES=""
for bin in "${BINARIES[@]}"; do
    INSTALL_LINES+="install -Dm755 %{_builddir}/../../target/release/${bin} %{buildroot}%{_bindir}/${bin}"$'\n'
    FILES_LINES+="%{_bindir}/${bin}"$'\n'
done

RPMBUILD_DIR="${HOME}/rpmbuild"
mkdir -p "${RPMBUILD_DIR}"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

cat > "${RPMBUILD_DIR}/SPECS/my-power-toys.spec" << EOF
Name:           my-power-toys
Version:        ${VERSION}
Release:        1%{?dist}
Summary:        System utilities for Linux inspired by Microsoft PowerToys

License:        GPL-3.0-only
URL:            https://github.com/pedrokarim/my-power-toys

Requires:       dbus
Recommends:     tesseract, wl-clipboard, xclip, xdotool, ddcutil, brightnessctl, polkit, ffmpeg

%description
MyPowerToys provides a suite of tools including window tiling,
app launcher, color picker, keyboard remapping, bulk rename,
image resizer, screen ruler, OCR text extraction, and more.

%install
${INSTALL_LINES}install -Dm644 %{_builddir}/../../crates/key-manager/assets/99-mpt-uinput.rules %{buildroot}%{_udevrulesdir}/99-mpt-uinput.rules
install -Dm644 %{_builddir}/../../assets/my-power-toys.desktop %{buildroot}%{_datadir}/applications/my-power-toys.desktop
install -Dm644 %{_builddir}/../../assets/my-power-toys-daemon.desktop %{buildroot}%{_datadir}/applications/my-power-toys-daemon.desktop
install -Dm644 %{_builddir}/../../assets/my-power-toys.service %{buildroot}%{_userunitdir}/my-power-toys.service
install -Dm644 %{_builddir}/../../assets/icons/icon-128.png %{buildroot}%{_datadir}/icons/hicolor/128x128/apps/my-power-toys.png

%files
${FILES_LINES}%{_udevrulesdir}/99-mpt-uinput.rules
%{_datadir}/applications/my-power-toys.desktop
%{_datadir}/applications/my-power-toys-daemon.desktop
%{_userunitdir}/my-power-toys.service
%{_datadir}/icons/hicolor/128x128/apps/my-power-toys.png

%post
echo "MyPowerToys installed. Enable the daemon with:"
echo "  systemctl --user enable --now my-power-toys.service"
echo "Key Manager: add yourself to the input group (sudo usermod -aG input \$USER)."
echo "Power Display: add yourself to the i2c group for ddcutil (sudo usermod -aG i2c \$USER)."
udevadm control --reload-rules >/dev/null 2>&1 || :
EOF

echo "==> Building RPM..."
rpmbuild -bb "${RPMBUILD_DIR}/SPECS/my-power-toys.spec"
echo "==> Done. Check ${RPMBUILD_DIR}/RPMS/ for the package."
