#!/usr/bin/env bash
set -euo pipefail

VERSION="0.1.0"
NAME="appnest"

echo "==> Packaging AppNest v${VERSION} source tarball..."

mkdir -p "${HOME}/rpmbuild/SOURCES"

rm -f "${HOME}/rpmbuild/SOURCES/${NAME}-${VERSION}.tar.gz"

tar \
    --exclude-vcs \
    --exclude='./target' \
    --exclude='./rpmbuild' \
    --transform "s,^\.,${NAME}-${VERSION}," \
    -czf "${HOME}/rpmbuild/SOURCES/${NAME}-${VERSION}.tar.gz" \
    .

echo "==> Created source tarball at ${HOME}/rpmbuild/SOURCES/${NAME}-${VERSION}.tar.gz"
