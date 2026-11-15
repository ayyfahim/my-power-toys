#!/bin/bash
# Build a .deb package for MyPowerToys
set -euo pipefail

VERSION="${1:-0.1.0}"
ARCH="amd64"
PKG_NAME="my-power-toys"
BUILD_DIR="target/release"
DEB_DIR="target/deb"
PKG_DIR="${DEB_DIR}/${PKG_NAME}_${VERSION}_${ARCH}"

echo "==> Building release binaries..."
# Whole workspace with default features, so every module's GUI binary is built.
cargo build --release --workspace

echo "==> Assembling .deb package..."
rm -rf "${PKG_DIR}"
mkdir -p "${PKG_DIR}/DEBIAN"
mkdir -p "${PKG_DIR}/usr/bin"
mkdir -p "${PKG_DIR}/usr/share/applications"
mkdir -p "${PKG_DIR}/usr/lib/systemd/user"
mkdir -p "${PKG_DIR}/usr/share/icons/hicolor/48x48/apps"
mkdir -p "${PKG_DIR}/usr/share/icons/hicolor/128x128/apps"
mkdir -p "${PKG_DIR}/usr/share/icons/hicolor/256x256/apps"

# Binaries
BINARIES=(
    mpt-daemon mpt-ctl mpt-settings
    mpt-app-launcher mpt-bulk-rename mpt-color-picker mpt-command-not-found
    mpt-command-palette mpt-crop-and-lock mpt-environment-variables
    mpt-fancy-zones mpt-file-locksmith mpt-hosts-editor mpt-image-resizer
    mpt-new-plus mpt-paste-plain mpt-peek mpt-power-display mpt-quick-accent
    mpt-screen-ruler mpt-shortcut-guide mpt-text-extractor mpt-workspaces
    mpt-zoom-it
)
for bin in "${BINARIES[@]}"; do
    cp "${BUILD_DIR}/${bin}" "${PKG_DIR}/usr/bin/"
done

# Key Manager udev rule (input group access to /dev/uinput)
mkdir -p "${PKG_DIR}/usr/lib/udev/rules.d"
cp crates/key-manager/assets/99-mpt-uinput.rules "${PKG_DIR}/usr/lib/udev/rules.d/"

# Desktop files
cp assets/my-power-toys.desktop "${PKG_DIR}/usr/share/applications/"
cp assets/my-power-toys-daemon.desktop "${PKG_DIR}/usr/share/applications/"

# Systemd user service
cp assets/my-power-toys.service "${PKG_DIR}/usr/lib/systemd/user/"

# Icons
cp assets/icons/icon-48.png "${PKG_DIR}/usr/share/icons/hicolor/48x48/apps/my-power-toys.png"
cp assets/icons/icon-128.png "${PKG_DIR}/usr/share/icons/hicolor/128x128/apps/my-power-toys.png"
cp assets/icons/icon-256.png "${PKG_DIR}/usr/share/icons/hicolor/256x256/apps/my-power-toys.png"

# Control file
cat > "${PKG_DIR}/DEBIAN/control" << EOF
Package: ${PKG_NAME}
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Depends: libdbus-1-3
Recommends: tesseract-ocr, wl-clipboard, xclip, xdotool, imagemagick, ddcutil, brightnessctl, pkexec, ffmpeg
Maintainer: Ahmed Karim <karimisma6464@gmail.com>
Description: System utilities for Linux inspired by Microsoft PowerToys
 MyPowerToys provides a suite of tools including window tiling,
 app launcher, color picker, keyboard remapping, bulk rename,
 image resizer, screen ruler, OCR text extraction, and more.
Homepage: https://github.com/pedrokarim/my-power-toys
EOF

# Post-install script
cat > "${PKG_DIR}/DEBIAN/postinst" << 'EOF'
#!/bin/bash
set -e
echo "MyPowerToys installed. Run 'mpt-daemon' or enable the systemd service:"
echo "  systemctl --user enable --now my-power-toys.service"
echo "Open settings with 'mpt-settings'"
echo "Key Manager: add yourself to the input group (sudo usermod -aG input \$USER)."
echo "Power Display: add yourself to the i2c group for ddcutil (sudo usermod -aG i2c \$USER)."
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
    udevadm trigger --subsystem-match=misc --sysname-match=uinput || true
fi
EOF
chmod 755 "${PKG_DIR}/DEBIAN/postinst"

# Build the deb
dpkg-deb --build "${PKG_DIR}"
echo "==> Package built: ${PKG_DIR}.deb"
