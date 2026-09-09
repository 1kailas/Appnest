#!/usr/bin/env bash
set -euo pipefail

VERSION="0.1.0"
NAME="appnest"

echo "==> Packaging AppNest v${VERSION} source tarball..."
mkdir -p "${HOME}/rpmbuild/SOURCES"
git archive --format=tar.gz --prefix="${NAME}-${VERSION}/" -o "${HOME}/rpmbuild/SOURCES/${NAME}-${VERSION}.tar.gz" HEAD 2>/dev/null || \
tar --exclude-vcs --exclude='target' --transform "s,^\.,${NAME}-${VERSION}," -czf "${HOME}/rpmbuild/SOURCES/${NAME}-${VERSION}.tar.gz" .

echo "==> Created source tarball at ${HOME}/rpmbuild/SOURCES/${NAME}-${VERSION}.tar.gz"
echo "==> To create the Source RPM (SRPM):"
echo "    rpmbuild -bs packaging/rpm/appnest.spec"
echo "==> To test with fedora-review:"
echo "    fedora-review -n appnest"
