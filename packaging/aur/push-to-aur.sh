#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKGNAME="appnest"
AUR_REPO="ssh://aur@aur.archlinux.org/${PKGNAME}.git"

echo "=== Checking SSH authentication to aur.archlinux.org ==="
AUTH_OUTPUT="$(ssh -T -o BatchMode=yes -o StrictHostKeyChecking=accept-new aur@aur.archlinux.org 2>&1 || true)"
if ! echo "$AUTH_OUTPUT" | grep -q "Welcome to AUR"; then
    echo "ERROR: Unable to authenticate to aur.archlinux.org via SSH."
    echo "$AUTH_OUTPUT"
    echo ""
    echo "To push to the AUR, please ensure:"
    echo "1. You have registered an account at https://aur.archlinux.org/register"
    echo "2. Your SSH public key (e.g. ~/.ssh/id_ed25519.pub) is added to your account at https://aur.archlinux.org/account/"
    echo "3. Run this script again: ./packaging/aur/push-to-aur.sh"
    exit 1
fi

echo "$AUTH_OUTPUT"

TMPDIR="$(mktemp -d /tmp/aur-${PKGNAME}-XXXXXX)"
trap 'rm -rf "${TMPDIR}"' EXIT

echo "=== Setting up AUR repository for ${PKGNAME} ==="
cd "${TMPDIR}"
git init -b master
git config user.name "$(git -C "${SCRIPT_DIR}" config user.name || echo '1kailas')"
git config user.email "$(git -C "${SCRIPT_DIR}" config user.email || echo '00kailas000@gmail.com')"

git remote add origin "${AUR_REPO}"
if git ls-remote origin &>/dev/null; then
    echo "Existing package repository found on AUR. Fetching..."
    git pull origin master --allow-unrelated-histories || true
else
    echo "Publishing new package '${PKGNAME}' to AUR."
fi

cp "${SCRIPT_DIR}/PKGBUILD" .
cp "${SCRIPT_DIR}/.SRCINFO" .
cp "${SCRIPT_DIR}/.gitignore" .

git add PKGBUILD .SRCINFO .gitignore
if git diff --cached --quiet; then
    echo "No changes to commit. AUR package is already up to date."
    exit 0
fi

VERSION="$(grep -m1 '^pkgver=' PKGBUILD | cut -d= -f2)-$(grep -m1 '^pkgrel=' PKGBUILD | cut -d= -f2)"
git commit -m "Update ${PKGNAME} to ${VERSION}"

echo "=== Pushing to AUR (master branch) ==="
git push origin master

echo ""
echo "=== Successfully published ${PKGNAME} to AUR! ==="
echo "Package URL: https://aur.archlinux.org/packages/${PKGNAME}"
