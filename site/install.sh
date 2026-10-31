#!/bin/sh
# Source of truth for the install script. scripts/site/build.mjs embeds it
# in the site worker and fills in the latest release version.
set -eu

REPO="erickochen/purple"
BINARY="purple"

main() {
    printf "\n  \033[1mpurple.\033[0m installer\n\n"

    # Detect OS (before dependency checks so unsupported OS gets a clear message)
    os="$(uname -s)"
    case "$os" in
        Darwin) os_suffix="apple-darwin" ;;
        Linux)  os_suffix="unknown-linux-gnu" ;;
        *)
            printf "  \033[1m!\033[0m Unsupported OS: %s\n" "$os"
            printf "  Install via cargo instead:\n\n"
            printf "    cargo install purple-ssh\n\n"
            exit 1
            ;;
    esac

    # Check dependencies (after OS detection so unsupported OS exits with a clear message)
    need_cmd curl
    need_cmd tar
    case "$os" in
        Darwin) need_cmd shasum ;;
        *)      need_cmd sha256sum ;;
    esac

    # Detect architecture
    arch="$(uname -m)"
    case "$arch" in
        arm64|aarch64) target="aarch64-${os_suffix}" ;;
        x86_64)        target="x86_64-${os_suffix}" ;;
        *)
            printf "  \033[1m!\033[0m Unsupported architecture: %s\n" "$arch"
            exit 1
            ;;
    esac

    # The site build fills in the latest release, so there is no lookup.
    # PURPLE_VERSION picks another release instead (1.2.3 or v1.2.3).
    version="${PURPLE_VERSION:-}"
    if [ -n "$version" ]; then
        version="${version#v}"
        if ! is_version "$version"; then
            printf "  \033[1m!\033[0m Invalid PURPLE_VERSION: %s (expected e.g. 1.2.3)\n" "$PURPLE_VERSION"
            exit 1
        fi
        printf "  Using requested version v%s\n" "$version"
    else
        version="__VERSION__"
        if ! is_version "$version"; then
            printf "  \033[1m!\033[0m This copy of the installer has no release version.\n"
            printf "  Install with curl -fsSL getpurple.sh | sh or set PURPLE_VERSION.\n\n"
            exit 1
        fi
    fi

    printf "  Found v%s for %s\n" "$version" "$target"

    # Set up temp directory
    tmp="$(mktemp -d)"
    staged=""
    trap 'rm -rf "$tmp"; [ -n "$staged" ] && rm -f "$staged"' EXIT INT TERM HUP

    tarball="purple-${version}-${target}.tar.gz"
    url="https://github.com/${REPO}/releases/download/v${version}/${tarball}"
    sha_url="${url}.sha256"

    # Download tarball and checksum
    printf "  Downloading...\n"
    if ! curl -fsSL "$url" -o "${tmp}/${tarball}"; then
        printf "  \033[1m!\033[0m Failed to download v%s for %s.\n" "$version" "$target"
        printf "  Check that the release exists: https://github.com/%s/releases\n\n" "$REPO"
        exit 1
    fi
    curl -fsSL "$sha_url" -o "${tmp}/${tarball}.sha256"

    # Verify checksum
    printf "  Verifying checksum...\n"
    expected="$(awk '{print $1}' "${tmp}/${tarball}.sha256")"
    case "$os" in
        Darwin) actual="$(shasum -a 256 "${tmp}/${tarball}" | awk '{print $1}')" ;;
        *)      actual="$(sha256sum "${tmp}/${tarball}" | awk '{print $1}')" ;;
    esac

    if [ "$expected" != "$actual" ]; then
        printf "  \033[1m!\033[0m Checksum mismatch.\n"
        printf "    Expected: %s\n" "$expected"
        printf "    Got:      %s\n" "$actual"
        exit 1
    fi

    # Extract
    tar -xzf "${tmp}/${tarball}" -C "$tmp"

    # Install
    install_dir="/usr/local/bin"
    if [ ! -w "$install_dir" ]; then
        install_dir="${HOME}/.local/bin"
        mkdir -p "$install_dir"
    fi

    # Stage in target dir then atomic rename (prevents corrupted binary on interrupt)
    staged="${install_dir}/.${BINARY}_new_$$"
    cp "${tmp}/${BINARY}" "$staged"
    chmod 755 "$staged"
    mv -f "$staged" "${install_dir}/${BINARY}"
    staged=""

    printf "\n  \033[1;35mpurple v%s\033[0m installed to %s/%s\n\n" \
        "$version" "$install_dir" "$BINARY"

    printf "  To update later, run: purple update\n\n"

    # Check PATH
    case ":${PATH}:" in
        *":${install_dir}:"*) ;;
        *)
            printf "  Add %s to your PATH:\n\n" "$install_dir"
            printf "    export PATH=\"%s:\$PATH\"\n\n" "$install_dir"
            ;;
    esac
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        printf "  \033[1m!\033[0m Required command not found: %s\n" "$1"
        exit 1
    fi
}

is_version() {
    printf '%s' "$1" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'
}

main "$@"
