# AppNest

A native Linux desktop application for managing, integrating and launching AppImages. Written in Rust with GTK4 and Libadwaita.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

## Features

* **Import** AppImages with the file picker, by drag and drop (several at once), or by opening them from a file manager. Files are either copied into AppNest's library or referenced where they are.
* **Validation**: every file is checked before use (ELF header, supported architecture, AppImage signature and embedded filesystem). A file is never accepted just because of its name.
* **Desktop integration**: an application menu entry and, independently, a desktop shortcut, each controlled per application and by preferences. Menu entries launch through `appnest launch`, so failure detection and the launch fallbacks apply however an application is started.
* **Discovery**: AppImages found in the scan folders but not in the library are listed under "Found on this computer" with an Add button. The folders are configurable in Preferences; Refresh (`Ctrl+R`/`F5`) searches again.
* **Updates**: checks the update information publishers embed in AppImages (`gh-releases-zsync` and `zsync`). Downloads use HTTPS, are checked against the SHA-1 in the zsync control file, and replace the current version while keeping the application's identity and settings. An application can also be replaced manually with any newer file.
* **Command line**: everything is also available without a display: `appnest list`, `info`, `import`, `launch`, `run`, `remove`, `scan`, `check-update`, `update` and `replace`. See `appnest --help` and `man appnest`.
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
* AppNest does not touch your default-application settings. The only network use is update checking and downloading, started by you, over HTTPS through the system `curl`, verified against the checksum published in the update information. There is no telemetry.

A calculated SHA-256 only tells you the file's hash. It is not a statement that the application is safe.

## Limitations

* Updating downloads the complete new AppImage; the zsync delta-transfer method is not implemented. Applications without embedded update information cannot be checked automatically, but can be replaced manually.
* Embedded AppImage signatures (`.sig_key` sections) are not verified. Updates prove the download matches what the publisher's update metadata announced, nothing more.
* Scan folders are searched with Refresh or at startup, not continuously.

## Installation

| Distribution | Status |
|---|---|
| Fedora | Submitted for review to the official repositories; **not yet available through `dnf` from Fedora**. A Copr project (`kailaz/appnest`) is used for test builds. |
| Debian/Ubuntu | Debian packaging in `debian/` builds against the distribution's own Rust crates. **Not yet in the official archives** — see [Packaging](#packaging). |
| Arch Linux | AUR package `appnest` (`packaging/aur`). |
| Others | Build from source. |

### Requirements

* Rust 1.92 or newer (build only)
* GTK 4.10+ and libadwaita 1.5+
* `7z` (the `7zip` package) for reading metadata, icons and extracting AppDirs
* `curl` for update checks and downloads
* Optional: `update-desktop-database` (`desktop-file-utils`) to refresh the desktop database right away

### Build and run

```bash
# Fedora
sudo dnf install cargo gtk4-devel libadwaita-devel 7zip
# Debian/Ubuntu 24.04
sudo apt install cargo libgtk-4-dev libadwaita-1-dev 7zip

cargo build --release --locked
./target/release/appnest
```

### Command-line usage

```
appnest list                     # what's in the library, ready/running/missing
appnest import ./Tool.AppImage   # add it (--no-copy / --no-menu / --no-shortcut)
appnest launch tool              # start it (arguments after -- go to the app)
appnest update -y tool           # download and install a published update
appnest replace tool ./Tool-2.AppImage   # swap in a newer file manually
appnest run ./Other.AppImage     # run once, without adding
appnest scan                     # AppImages found in the scan folders
appnest remove -y tool           # remove (--delete-file drops the copy too)
```

Exit status is 0 on success, 1 on failure, 2 for usage errors, so the commands can be scripted. All commands work without a display server. See the man page for the full reference.

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
$XDG_CONFIG_HOME/appnest/        # config.toml (incl. scan folders)
$XDG_CACHE_HOME/appnest/         # Launch logs, downloaded updates
```

Desktop shortcuts go to the XDG desktop directory.

## Troubleshooting

* **"7-Zip is not installed"**: install the `7zip` package (`7zip` on Debian/Ubuntu).
* **An AppImage will not start**: the error message shows the program's output. If it mentions FUSE, choose *Extract and run* or *Extracted AppRun* for that application.
* **"File missing"** on an entry: the AppImage was moved or deleted. Restore the file and refresh, or remove the entry.
* **"Cannot use ...: Not an AppImage"**: the file lacks the AppImage signature or embedded filesystem, or is for an unsupported CPU architecture.

## Architecture

```text
src/domain/          models and the repository trait
src/application/     commands (import, launch, close, remove, update, migrate) and queries
src/services/        validator, extractor, importer, launcher, scanner, updater, desktop entries
src/cli.rs           command-line interface (no GTK)
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

### Debian / Ubuntu (apt)

Debian packaging lives in `debian/` and builds against the distribution's own
packaged Rust crates (no vendored or downloaded dependencies):

```bash
sudo apt install debhelper dh-cargo rustc cargo libgtk-4-dev libadwaita-1-dev \
  librust-gtk4-dev librust-libadwaita-dev # + the crates in debian/control
dpkg-buildpackage -us -uc -b
```

This currently requires Debian unstable/testing or a release whose gtk-rs crates
are new enough (libadwaita-rs 0.9+). Ubuntu 24.04 LTS is too old; build from
source there instead. AppNest is **not yet in the official Debian or Ubuntu
archives**; inclusion would need a Debian maintainer to adopt the package.

## Contributing

Issues and pull requests are welcome. Before sending a change, run `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings` and `cargo test --locked`.

## Reporting security issues

Please report vulnerabilities privately, through GitHub's "Report a vulnerability" option on the repository's Security tab or by email to the maintainer, rather than in a public issue.

## License

GNU General Public License v3.0 or later (GPL-3.0-or-later). See [LICENSE](LICENSE).
