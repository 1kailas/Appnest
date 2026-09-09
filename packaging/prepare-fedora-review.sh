#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$ROOT_DIR"

VERSION="0.1.0"
NAME="appnest"

echo "=== 1. Checking required tools ==="
MISSING_TOOLS=()
for tool in rpmbuild rpmlint fedora-review rpmdev-setuptree; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        MISSING_TOOLS+=("$tool")
    fi
done

if [ ${#MISSING_TOOLS[@]} -ne 0 ]; then
    echo "⚠️  Missing required Fedora review tools: ${MISSING_TOOLS[*]}"
    echo "Please run the following command in your terminal with your password:"
    echo ""
    echo "  sudo dnf install -y fedora-packager rpmdevtools rpmlint mock fedora-review"
    echo ""
    exit 1
fi

echo "=== 2. Setting up RPM build tree ==="
rpmdev-setuptree

echo "=== 3. Placing Source Tarball & Spec File ==="
if [ ! -f "dist/${NAME}-${VERSION}.tar.gz" ]; then
    mkdir -p dist
    git archive --format=tar.gz --prefix="${NAME}-${VERSION}/" -o "dist/${NAME}-${VERSION}.tar.gz" HEAD
fi

cp "dist/${NAME}-${VERSION}.tar.gz" "${HOME}/rpmbuild/SOURCES/${NAME}-${VERSION}.tar.gz"
cp "packaging/rpm/${NAME}.spec" "${HOME}/rpmbuild/SPECS/${NAME}.spec"

echo "=== 4. Building Source RPM (SRPM) ==="
rpmbuild -bs "${HOME}/rpmbuild/SPECS/${NAME}.spec"

SRPM_FILE=$(find "${HOME}/rpmbuild/SRPMS" -name "${NAME}-${VERSION}-*.src.rpm" | head -n 1)
echo "✅ SRPM built: ${SRPM_FILE}"
cp "${SRPM_FILE}" "dist/"

echo "=== 5. Running rpmlint on Spec and SRPM ==="
rpmlint "${HOME}/rpmbuild/SPECS/${NAME}.spec"
rpmlint "${SRPM_FILE}"

echo "=== 6. Running fedora-review ==="
echo "Running: fedora-review -b ${SRPM_FILE}"
fedora-review -b "${SRPM_FILE}"
