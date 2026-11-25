# AppNest

A native Linux desktop application for managing, integrating and launching AppImages. Written in Rust with GTK4 and Libadwaita.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

## Features

* **Import** AppImages with the file picker, by drag and drop (several at once), or by opening them from a file manager. Files are either copied into AppNest's library or referenced where they are.
* **Validation**: every file is checked before use (ELF header, supported architecture, AppImage signature and embedded filesystem). A file is never accepted just because of its name.
* **Desktop integration**: a menu entry (`appnest-<id>.desktop`) and, if a desktop folder exists, a desktop shortcut, with the icon extracted from the AppImage.
* **Launch, close and Run Once** from the window. Running applications are detected, including ones started outside AppNest.
* **Adaptive launching**: see [Launch methods](#launch-methods).
* **SHA-256**: compute and copy the checksum of an AppImage. The stored value is discarded automatically when the file changes.
* Search, light/dark/system theme and keyboard shortcuts (`Ctrl+O` add, `Ctrl+R`/`F5` refresh, `Ctrl+F` search, `Ctrl+,` preferences, `Ctrl+Q`/`Ctrl+W` close).

### Launch methods

| Method | What it does |
|---|---|
| Auto (default) | Runs the AppImage directly. If, and only if, it fails right away with a FUSE, mount or extraction error, it retries with *extract and run*, then with the managed extracted AppDir. A normal application failure is reported and never retried. |
| Direct | Runs the AppImage directly. |
| Extract and run | Runs `--appimage-extract-and-run`. Needs no FUSE. |
| Extracted AppRun | Extracts the AppDir once into AppNest's data folder and runs its `AppRun` with `APPDIR` and `APPIMAGE` set. The extraction is redone if the AppImage file changes. |

A manually selected method is never silently replaced. Startup failures are shown with the program's own error output (also kept in `$XDG_CACHE_HOME/appnest/launch-<id>.log`).

## Security model

AppImages are **untrusted third-party programs**. Launching one runs it with your user's permissions. **AppNest is not a sandbox.**

What AppNest does to limit the risk of *managing* them:

* Metadata and icons are read by extracting only the needed files, never by running the AppImage.
* Extraction happens in a private staging directory and is checked before it is used: unsafe paths, symlinks that leave the AppDir, special files, set-id bits, and size/file-count limits. A failed or unsafe extraction is never left behind.
* Names taken from an AppImage are never used as file names. Desktop entries are escaped according to the Desktop Entry Specification, namespaced, and never overwrite files AppNest did not create.
* AppNest never disables an application's sandbox (for example it does not add `--no-sandbox`).
* The library database is written atomically, under a lock, keeps a backup, and a damaged file is set aside rather than deleted.
* AppNest does not touch your default-application settings, does not use the network, and sends no telemetry.

A calculated SHA-256 only tells you the file's hash. It is not a statement that the application is safe.

## Not implemented

* Showing AppImages discovered in `~/Applications` or the download folder. The scanner exists (`AppImageScanner`) but is not wired into the interface.
* Updating AppImages (a new version is added as a separate entry; there is no update checker, replacement or rollback workflow).
* A command-line interface for managing AppImages (`appnest [FILE...]` only opens files in the window).
* Separate switches for the menu entry and the desktop shortcut: they are created and removed together.

## Installation

| Distribution | Status |
|---|---|
| Fedora | Submitted for review to the official repositories; **not yet available through `dnf` from Fedora**. A Copr project (`kailaz/appnest`) is used for test builds. |
| Arch Linux | AUR package `appnest` (`packaging/aur`). |
| Others | Build from source. |

### Requirements

* Rust 1.92 or newer (build only)
* GTK 4.6+ and libadwaita 1.5+
* `7z` (the `7zip` package) for reading metadata, icons and extracting AppDirs
* Optional: `update-desktop-database` (`desktop-file-utils`) to refresh the desktop database right away

### Build and run

```bash
# Fedora
sudo dnf install cargo gtk4-devel libadwaita-devel 7zip
# Debian/Ubuntu 24.04
sudo apt install cargo libgtk-4-dev libadwaita-1-dev p7zip-full

cargo build --release --locked
./target/release/appnest
```

### Tests

```bash
cargo test --locked
```

Tests keep all AppNest data, settings and desktop files in temporary directories. Tests that need real SquashFS images are skipped when `mksquashfs` (`squashfs-tools`) or `7z` is missing.

### Where data lives

```text
$XDG_DATA_HOME/appnest/          (default ~/.local/share/appnest/)
├── applications/                # AppImages copied into the library
├── extracted/                   # Extracted AppDirs
├── icons/
└── apps.toml                    # Library database (+ .bak backup)
$XDG_DATA_HOME/applications/     # Menu entries: appnest-<id>.desktop
$XDG_CONFIG_HOME/appnest/        # config.toml
$XDG_CACHE_HOME/appnest/         # Launch logs
```

Desktop shortcuts go to the XDG desktop directory.

## Troubleshooting

* **"7-Zip is not installed"**: install the `7zip` package (`p7zip-full` on Debian/Ubuntu).
* **An AppImage will not start**: the error message shows the program's output. If it mentions FUSE, choose *Extract and run* or *Extracted AppRun* for that application.
* **"File missing"** on an entry: the AppImage was moved or deleted. Restore the file and refresh, or remove the entry.
* **"Cannot use ...: Not an AppImage"**: the file lacks the AppImage signature or embedded filesystem, or is for an unsupported CPU architecture.

## Architecture

```text
src/domain/          models and the repository trait
src/application/     commands (import, launch, close, remove, update, migrate) and queries
src/services/        validator, extractor, importer, launcher, scanner, desktop entries
src/infrastructure/  filesystem, hashing, process handling, TOML repository, settings
src/ui/              GTK4 / Libadwaita widgets
```

Long-running work (listing, scanning for running applications, launching, closing, hashing, extraction) runs on worker threads and reports back to the main loop. `src/ui/window.rs` still contains a good deal of orchestration code that would be better placed in the application layer.

## Packaging

The Fedora spec (`packaging/rpm/appnest.spec`) follows the Fedora Rust guidelines and builds offline against Fedora's packaged crates, with no vendored dependencies. Validate the metadata with:

```bash
desktop-file-validate packaging/io.github._1kailas.AppNest.desktop
appstreamcli validate --no-net resources/io.github._1kailas.AppNest.metainfo.xml
```

## Contributing

Issues and pull requests are welcome. Before sending a change, run `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings` and `cargo test --locked`.

## Reporting security issues

Please report vulnerabilities privately, through GitHub's "Report a vulnerability" option on the repository's Security tab or by email to the maintainer, rather than in a public issue.

## License

GNU General Public License v3.0 or later (GPL-3.0-or-later). See [LICENSE](LICENSE).
