#!/usr/bin/env bash
# Runs inside the cibuildwheel build container / runner once per matrix
# entry. Installs rustup with the right target, compiles the sqlitegis
# cdylib with sqlite-extension + bundled-sqlite, and copies the produced
# binary into bindings/python/sqlitegis/_bin/ where hatchling picks it up
# at wheel-build time.
#
# The cargo target triple is passed as $1. Linux containers (manylinux,
# musllinux) leave it empty and use the host triple (cibuildwheel runs
# inside a per-arch container so the host triple is already correct).
# macOS and Windows pass the explicit triple so we can build the right
# arch when the runner does not match the wheel's target.

set -euo pipefail

CARGO_TARGET="${1:-}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "${REPO_ROOT}"

# Install Rust if not already present (manylinux / musllinux containers
# do not ship rustup). Use rustup-init's standalone installer so the
# install works the same way on every platform.
if ! command -v cargo >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain stable --profile minimal
    export PATH="${HOME}/.cargo/bin:${PATH}"
fi

if [ -n "${CARGO_TARGET}" ]; then
    rustup target add "${CARGO_TARGET}"
    BUILD_ARGS=(--target "${CARGO_TARGET}")
    TARGET_DIR="target/${CARGO_TARGET}/release"
else
    BUILD_ARGS=()
    TARGET_DIR="target/release"
fi

cargo build --release \
    --features sqlite-extension,bundled-sqlite \
    "${BUILD_ARGS[@]}"

# Locate the produced cdylib. cargo emits libsqlitegis.so on Linux,
# libsqlitegis.dylib on macOS, sqlitegis.dll on Windows.
DEST="bindings/python/sqlitegis/_bin"
mkdir -p "${DEST}"

shopt -s nullglob
COPIED=0
for src in \
    "${TARGET_DIR}/libsqlitegis.so" \
    "${TARGET_DIR}/libsqlitegis.dylib" \
    "${TARGET_DIR}/sqlitegis.dll"; do
    if [ -f "${src}" ]; then
        echo "Copying ${src} -> ${DEST}/"
        cp "${src}" "${DEST}/"
        COPIED=1
    fi
done

if [ "${COPIED}" -eq 0 ]; then
    echo "ERROR: no cdylib was produced under ${TARGET_DIR}/" >&2
    ls -la "${TARGET_DIR}" >&2 || true
    exit 1
fi

ls -la "${DEST}"
