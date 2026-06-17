#!/usr/bin/env bash
set -e

echo "🚀 System Monitor - Desktop Integration Installer"
echo "================================================="
echo ""

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

APP_NAME="System Monitor"
BINARY_NAME=$(grep '^name =' Cargo.toml | head -n1 | cut -d '"' -f2)

INSTALL_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
DESKTOP_FILE="$APP_DIR/system-monitor.desktop"

ICON_THEME_DIR="$HOME/.local/share/icons/hicolor/512x512/apps"
ICON_NAME="system-monitor"

DESKTOP_DIR="$HOME/Desktop"

echo -e "${BLUE}Step 1: Checking environment...${NC}"

mkdir -p "$INSTALL_DIR"
mkdir -p "$APP_DIR"
mkdir -p "$ICON_THEME_DIR"

if ! command -v cargo &> /dev/null; then
    echo -e "${YELLOW}Rust not installed. Installing...${NC}"
    curl https://sh.rustup.rs -sSf | sh -s -- -y
    source "$HOME/.cargo/env"
fi

echo -e "${GREEN}Environment OK${NC}"
echo ""

echo -e "${BLUE}Step 2: Building application...${NC}"

if [ ! -f "Cargo.toml" ]; then
    echo -e "${YELLOW}Run this inside your project directory.${NC}"
    exit 1
fi

cargo build --release --features nvidia

echo -e "${GREEN}Build successful${NC}"
echo ""

echo -e "${BLUE}Step 3: Installing binary...${NC}"

cp "target/release/$BINARY_NAME" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/$BINARY_NAME"

echo -e "${GREEN}Installed → $INSTALL_DIR/$BINARY_NAME${NC}"
echo ""

echo -e "${BLUE}Step 4: Icon setup...${NC}"

echo "Enter path to icon (PNG) or press Enter to skip:"
read -r ICON_PATH

if [ -n "$ICON_PATH" ] && [ -f "$ICON_PATH" ]; then
    cp "$ICON_PATH" "$ICON_THEME_DIR/$ICON_NAME.png"
    echo -e "${GREEN}Icon installed${NC}"
else
    echo "Skipping icon"
fi

echo ""

echo -e "${BLUE}Step 5: Creating desktop entry...${NC}"

cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=$APP_NAME
Comment=Real-time system performance monitoring
Exec=$INSTALL_DIR/$BINARY_NAME
Icon=$ICON_NAME
Terminal=false
Categories=System;Monitor;
Keywords=system;monitor;performance;cpu;memory;
StartupNotify=true
EOF

chmod +x "$DESKTOP_FILE"

echo -e "${GREEN}Desktop entry created${NC}"
echo ""

echo -e "${BLUE}Step 6: Updating desktop databases...${NC}"

if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$APP_DIR"
fi

if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor"
fi

echo -e "${GREEN}Caches updated${NC}"
echo ""

echo -e "${BLUE}Step 7: Creating Desktop icon...${NC}"

if [ -d "$DESKTOP_DIR" ]; then
    cp "$DESKTOP_FILE" "$DESKTOP_DIR/"
    chmod +x "$DESKTOP_DIR/system-monitor.desktop"

    if command -v gio &> /dev/null; then
        gio set "$DESKTOP_DIR/system-monitor.desktop" metadata::trusted true || true
    fi

    echo -e "${GREEN}Desktop icon created${NC}"
else
    echo -e "${YELLOW}Desktop folder not found${NC}"
fi

echo ""

echo -e "${BLUE}Step 8: GNOME fix (Fedora default)...${NC}"

if command -v gnome-extensions &> /dev/null; then
    if ! gnome-extensions list | grep -q ding@rastersoft.com; then
        echo "Installing GNOME desktop icons extension..."
        sudo dnf install -y gnome-shell-extension-desktop-icons-ng || true
    fi

    echo "Enabling desktop icons..."
    gnome-extensions enable ding@rastersoft.com || true
fi

echo ""

echo -e "${GREEN}========================================"
echo " Installation Complete!"
echo "========================================${NC}"
echo ""
echo "You can now launch System Monitor:"
echo ""
echo "1) From the app menu"
echo "2) From Desktop icon"
echo "3) Terminal command:"
echo ""
echo "   system-monitor"
echo ""
