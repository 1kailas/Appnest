Name:           appnest
Version:        0.1.0
Release:        1%{?dist}
Summary:        Modern AppImage manager built with Rust, GTK4, and Libadwaita

License:        MIT
URL:            https://github.com/1kailas/appnest
Source0:        https://github.com/1kailas/%{name}/archive/v%{version}/%{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.80
BuildRequires:  cargo
BuildRequires:  pkgconfig(gtk4) >= 4.10
BuildRequires:  pkgconfig(libadwaita-1) >= 1.4
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib

Requires:       hicolor-icon-theme
Requires:       p7zip
Recommends:     squashfs-tools

%description
AppNest is a clean, modern Linux desktop application for managing,
organizing, integrating, and launching AppImages. Built in Rust with
GTK4 and Libadwaita following Clean Architecture principles.

Features an adaptive runtime fallback (Direct Native, FUSE, and Extracted
SquashFS AppRun) to eliminate binfmt_misc and FUSE execution failures,
automatic .desktop integration, SHA-256 integrity verification, and
custom per-application environment and launch arguments.

%prep
%autosetup

%build
cargo build --release

%install
rm -rf %{buildroot}
install -Dpm 0755 target/release/%{name} %{buildroot}%{_bindir}/%{name}
install -Dpm 0644 resources/icons/io.github._1kailas.AppNest.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github._1kailas.AppNest.svg
install -Dpm 0644 packaging/io.github._1kailas.AppNest.desktop %{buildroot}%{_datadir}/applications/io.github._1kailas.AppNest.desktop
install -Dpm 0644 resources/io.github._1kailas.AppNest.metainfo.xml %{buildroot}%{_datadir}/metainfo/io.github._1kailas.AppNest.metainfo.xml

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github._1kailas.AppNest.desktop
appstreamcli validate --no-net %{buildroot}%{_datadir}/metainfo/io.github._1kailas.AppNest.metainfo.xml

%files
%license LICENSE
%doc README.md
%{_bindir}/%{name}
%{_datadir}/applications/io.github._1kailas.AppNest.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github._1kailas.AppNest.svg
%{_datadir}/metainfo/io.github._1kailas.AppNest.metainfo.xml

%changelog
* Wed Sep 09 2026 1kailas <1kailas@users.noreply.github.com> - 0.1.0-1
- Initial release of AppNest for Fedora / DNF
