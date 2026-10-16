#!/usr/bin/env bash
set -euo pipefail

VERSION="0.2.0"
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

mkdir -p "${HOME}/rpmbuild/SPECS"
cp "packaging/rpm/${NAME}.spec" "${HOME}/rpmbuild/SPECS/${NAME}.spec"

echo "==> Building Source RPM (SRPM)..."
rpmbuild -bs "${HOME}/rpmbuild/SPECS/${NAME}.spec"

SRPM_FILE=$(find "${HOME}/rpmbuild/SRPMS" -name "${NAME}-${VERSION}-*.src.rpm" | head -n 1)
echo "✅ SRPM built: ${SRPM_FILE}"
echo ""
echo "To upload to Fedora Copr:"
echo "  copr-cli build kailaz/appnest \"${SRPM_FILE}\""

