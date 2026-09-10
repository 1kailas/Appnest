# 🪺 AppNest

A clean, modern Linux desktop application for managing, organizing, integrating, and launching AppImages. Built in **Rust** with **GTK4** and **Libadwaita** following **Clean Architecture**.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Platform: Linux](https://img.shields.io/badge/Platform-Linux-orange.svg)]()
[![Desktop: GNOME / Libadwaita](https://img.shields.io/badge/UI-GTK4%20%2F%20Libadwaita-brightgreen.svg)]()

---

## 🏗️ Architecture & Philosophy

The application strictly separates user interface code from domain and filesystem logic:

```text
                    ┌─────────────────────────┐
                    │  GTK4 / Libadwaita UI   │
                    │  (Views & Controllers)  │
                    └────────────┬────────────┘
                                 │
                                 ▼
                    ┌─────────────────────────┐
                    │    Application Layer    │
                    │  (Commands & Queries)   │
                    └────────────┬────────────┘
                                 │
                                 ▼
                    ┌─────────────────────────┐
                    │      Domain Layer       │
                    │  (Pure Models & Traits) │
                    └────────────┬────────────┘
                                 │
                                 ▼
                    ┌─────────────────────────┐
                    │  Infrastructure Layer   │
                    │ (Filesystem, Processes) │
                    └─────────────────────────┘
```

* **Zero Business Logic in UI**: GTK widgets only emit user actions and render state updates.
* **Non-blocking Operations**: Background workers handle heavy tasks (SquashFS extraction, SHA-256 computation, downloads) without freezing the UI.
* **Testable Without GTK**: Domain, services, repository, and validation logic are unit-tested in headless CI environments.

---

## 🚀 Key Features

* **🛡️ Adaptive Runtime Manager**:
  Handles execution across diverse Linux distributions and system setups:
  1. **Direct Native**: Direct execution of AppImage binary.
  2. **FUSE / Extract-and-run**: Runs with `--appimage-extract-and-run` if FUSE is unavailable or unprivileged.
  3. **Extracted AppRun Fallback**: Extracts SquashFS contents and executes `AppRun` directly. This circumvents broken `binfmt_misc` kernel interception (such as orphaned `AppImageLauncher` handlers) and missing FUSE modules.
* **🖱️ Drag & Drop**: Drop one or multiple `.AppImage` files anywhere onto the window to automatically validate, import, extract icons, and integrate them in the background.
* **⚡ Non-Blocking Background Operations**: Long-running operations like SquashFS extraction, file importing, and SHA-256 checksum computation run on background worker threads with `glib::spawn_future_local`, keeping the UI responsive at all times.
* **🔒 SHA-256 Integrity Verification**: On-demand calculation of SHA-256 file hashes with one-click copy to clipboard and persistent database caching.
* **⌨️ Global Keyboard Accelerators**:
  - `Ctrl+O` / `Ctrl+N`: Add / Import AppImage
  - `Ctrl+R` / `F5`: Refresh application library
  - `Ctrl+F`: Focus search bar
  - `Ctrl+,`: Open Preferences window
  - `Ctrl+Q` / `Ctrl+W`: Close application
* **🎨 Modern Libadwaita Theming**: System theme following, Force Light, and Force Dark color schemes via Libadwaita `StyleManager`.
* **🔍 Automatic Scanner**: Discovers unmanaged AppImages across standard locations (`~/Applications`, `~/Downloads`, etc.) with one-click non-blocking import.
* **🖥️ Desktop Integration**: Generates freedesktop `.desktop` entries in `~/.local/share/applications/` and refreshes the desktop application database so installed applications immediately appear in the GNOME Dash, app grid, and application launchers.
* **🖼️ Multi-Tier Icon Extraction**: Resolves embedded `.DirIcon` symlinks and extracts high-resolution icons (512x512 PNG, SVG) into user icon directories with automatic theme fallbacks.
* **📂 XDG Compliant**: Adheres to Linux freedesktop specifications for configuration, cache, and data directories.

---

## 📁 Directory Layout

Adheres to standard XDG user directory specifications:

```text
~/.local/share/appnest/
├── applications/             # Managed AppImage binaries
├── extracted/                # Extracted AppDirs for fallback execution
├── icons/                    # Application icons
└── apps.toml                 # Application database & metadata registry

~/.local/share/applications/  # Freedesktop .desktop entries
~/.config/appnest/            # Application settings (config.toml)
~/.cache/appnest/             # Temporary cache and logs
```

---

## 🛠️ Building & Running

### Requirements
* **Rust** 1.80+ (`cargo`, `rustc`)
* **GTK4** (4.10+)
* **libadwaita** (1.4+)
* **7-Zip** (`7z`) or `squashfs-tools` (for SquashFS extraction)
* `desktop-file-utils`

### Build & Run
```bash
# Build release binary
cargo build --release

# Run
./target/release/appnest
```

### Run Tests
```bash
cargo test
```

---

## 📦 Packaging

### Arch Linux (AUR)

Install with your preferred AUR helper:
```bash
yay -S appnest
# or
paru -S appnest
```

Or build and install manually with `makepkg`:
```bash
cd packaging/aur
makepkg -si
```

### Fedora / DNF / Copr

AppNest includes spec and metadata files strictly adhering to Fedora Packaging Guidelines:
* Spec file: `packaging/rpm/appnest.spec`
* AppStream metainfo: `resources/io.github._1kailas.AppNest.metainfo.xml`
* Desktop entry: `packaging/io.github._1kailas.AppNest.desktop`
* Icon: `resources/icons/io.github._1kailas.AppNest.svg`

### Validate Packaging Standards
```bash
desktop-file-validate packaging/io.github._1kailas.AppNest.desktop
appstreamcli validate --no-net resources/io.github._1kailas.AppNest.metainfo.xml
```

---

## 📄 License

This project is licensed under the **GNU General Public License v3.0 (GPL-3.0-or-later)** - see the [LICENSE](LICENSE) file for details.
