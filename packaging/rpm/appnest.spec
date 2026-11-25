%global app_id io.github._1kailas.AppNest

Name:           appnest
Version:        0.4.0
Release:        1%{?dist}
Summary:        Manage, integrate, and launch AppImages

# Output of %%{cargo_license_summary}:
# Apache-2.0 OR MIT
# Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT
# GPL-3.0-or-later
# MIT
# MIT OR Apache-2.0
# MPL-2.0
# Unlicense OR MIT
# LICENSE.dependencies contains a full license breakdown
License:        %{shrink:
    GPL-3.0-or-later AND
    MIT AND
    MPL-2.0 AND
    (Apache-2.0 OR MIT) AND
    (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT) AND
    (Unlicense OR MIT)
}
URL:            https://github.com/1kailas/Appnest
Source:         %{url}/archive/v%{version}/%{name}-%{version}.tar.gz

ExclusiveArch:  %{rust_arches}

BuildRequires:  cargo-rpm-macros >= 24
BuildRequires:  pkgconfig(gtk4) >= 4.10
BuildRequires:  pkgconfig(libadwaita-1) >= 1.5
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib

Requires:       hicolor-icon-theme
Requires:       /usr/bin/7z
Requires:       curl
Recommends:     desktop-file-utils

%description
AppNest is a desktop application for managing, organizing, integrating, and
launching AppImages. It imports AppImages into a managed library, creates
desktop entries and icons for them, discovers AppImages that are not managed
yet, checks for updates published inside the AppImages, and can verify their
SHA-256 hashes. Every feature is also available on the command line.

An adaptive runtime (direct execution, extract-and-run, or running the
extracted AppRun) lets AppImages start even when FUSE is unavailable.

%prep
%autosetup -n Appnest-%{version} -p1
%cargo_prep

%generate_buildrequires
%cargo_generate_buildrequires -t

%build
%cargo_build
%{cargo_license_summary}
%{cargo_license} > LICENSE.dependencies

%install
install -Dpm 0755 target/rpm/%{name} -t %{buildroot}%{_bindir}
desktop-file-install --dir=%{buildroot}%{_datadir}/applications packaging/%{app_id}.desktop
install -Dpm 0644 resources/icons/%{app_id}.svg -t %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
install -Dpm 0644 resources/%{app_id}.metainfo.xml -t %{buildroot}%{_metainfodir}
install -Dpm 0644 packaging/%{name}.1 -t %{buildroot}%{_mandir}/man1

%check
%cargo_test
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml

%files
%license LICENSE
%license LICENSE.dependencies
%doc README.md
%{_bindir}/%{name}
%{_mandir}/man1/%{name}.1*
%{_datadir}/applications/%{app_id}.desktop
%{_datadir}/icons/hicolor/scalable/apps/%{app_id}.svg
%{_metainfodir}/%{app_id}.metainfo.xml

%changelog
* Wed Nov 25 2026 Kailasnath T <00kailas000@gmail.com> - 0.4.0-1
- Update checks and downloads using the AppImage update information (zsync)
- Command-line interface: list, info, import, launch, run, remove, scan,
  check-update, update, replace
- Discovery: unmanaged AppImages found in configurable scan folders are
  shown in the window
- Menu entries and desktop shortcuts are now independent per application
- Menu entries launch through `appnest launch` so all entry points share
  failure detection and runtime fallbacks
- Replace an application with a newer AppImage file

* Wed Nov 25 2026 Kailasnath T <00kailas000@gmail.com> - 0.3.0-1
- Initial package for Fedora
