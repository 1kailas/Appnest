#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$(dirname "$(dirname "$SCRIPT_DIR")")"

NAME="appnest"
VERSION="$(awk '/^Version:/ {print $2; exit}' "packaging/rpm/${NAME}.spec")"

echo "==> Packaging AppNest v${VERSION} source tarball..."

mkdir -p "${HOME}/rpmbuild/SOURCES" "${HOME}/rpmbuild/SPECS"

# GitHub names the top-level directory after the repository (Appnest-VERSION)
git archive --format=tar.gz --prefix="Appnest-${VERSION}/" \
    -o "${HOME}/rpmbuild/SOURCES/${NAME}-${VERSION}.tar.gz" HEAD

echo "==> Created source tarball at ${HOME}/rpmbuild/SOURCES/${NAME}-${VERSION}.tar.gz"

cp "packaging/rpm/${NAME}.spec" "${HOME}/rpmbuild/SPECS/${NAME}.spec"

echo "==> Building Source RPM (SRPM)..."
rpmbuild -bs "${HOME}/rpmbuild/SPECS/${NAME}.spec"

SRPM_FILE=$(find "${HOME}/rpmbuild/SRPMS" -name "${NAME}-${VERSION}-*.src.rpm" | head -n 1)
echo "SRPM built: ${SRPM_FILE}"
echo ""
echo "To upload to Fedora Copr:"
echo "  copr-cli build kailaz/appnest \"${SRPM_FILE}\""
