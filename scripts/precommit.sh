#!/usr/bin/env bash
# Single source of truth for the pre-commit gates. Run before every
# commit. Strict mode so the first non-zero exit aborts the run.
#
# Use `cargo +stable` everywhere so rustfmt and clippy versions match
# what CI runs (`dtolnay/rust-toolchain@master` resolves to latest
# stable). Whatever the local default toolchain happens to be drifts
# from CI silently.
#
# Pass `--with-docker` to add a Linux validation gate that runs the
# same toolchain CI uses inside `rust:latest`. Catches regressions in
# `#[cfg(target_os = "linux")]` paths invisible on macOS.
#
# Pass `--with-nix` to add a `nix flake check` gate that validates the
# flake builds the package and `cargo fmt` passes inside the Nix
# sandbox. Requires `nix` on PATH with flakes enabled.

set -euo pipefail

CYAN='\033[0;36m'; GREEN='\033[0;32m'; RED='\033[0;31m'; RESET='\033[0m'
gate_num=0
WITH_DOCKER=false
WITH_NIX=false
for arg in "$@"; do
    case "$arg" in
        --with-docker) WITH_DOCKER=true ;;
        --with-nix)    WITH_NIX=true ;;
    esac
done

step() {
    gate_num=$((gate_num + 1))
    echo -e "${CYAN}[$gate_num] $*${RESET}"
}

pass() {
    echo -e "${GREEN}    ✓ pass${RESET}"
}

# Verify cargo +stable is available; otherwise we are running against
# a non-stable default which will drift from CI.
if ! cargo +stable --version >/dev/null 2>&1; then
    echo -e "${RED}cargo +stable is not installed. Install with: rustup install stable${RESET}"
    exit 1
fi

step "cargo +stable fmt --check"
cargo +stable fmt --check
pass

step "cargo +stable clippy --all-targets -- -D warnings"
cargo +stable clippy --all-targets -- -D warnings
pass

step "cargo build"
cargo build
pass

step "cargo test (release-grade race flush: 3 runs)"
for i in 1 2 3; do
    echo "    run $i ..."
    cargo test --quiet
done
pass

step "cargo deny check"
cargo deny check
pass

step "rustup run 1.88 cargo check --locked (MSRV)"
rustup run 1.88 cargo check --locked
pass

step 'RUSTDOCFLAGS="-D warnings" cargo doc --no-deps'
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
pass

step "smoke tests (TUI, shell exit codes, host key prompt)"
./tests/smoke_tui.sh
python3 tests/ssh_exit_tui_smoke.py target/debug/purple
# The host key test starts a throwaway sshd. On Linux that also needs the
# privilege separation directory, which CI creates before its run.
if [[ -x /usr/sbin/sshd ]] && { [[ "$(uname)" == "Darwin" ]] || [[ -d /run/sshd ]]; }; then
    python3 tests/host_key_askpass_smoke.py target/debug/purple
else
    echo -e "${RED}    NOTE: host key prompt test skipped (needs /usr/sbin/sshd, plus /run/sshd on Linux)${RESET}"
fi
pass

step "./scripts/check-design-system.sh"
./scripts/check-design-system.sh
pass

step "./scripts/check-messages.sh"
./scripts/check-messages.sh
pass

step "./scripts/check-keybindings.sh"
./scripts/check-keybindings.sh
pass

step "./scripts/check-conventions.sh"
./scripts/check-conventions.sh
pass

step "./scripts/check-aur-pkgbuild-drift.sh"
./scripts/check-aur-pkgbuild-drift.sh
pass

step "./scripts/check-alias-caches.sh"
./scripts/check-alias-caches.sh
pass

step "cargo test --lib visual_regression"
cargo test --lib visual_regression
pass

# Asset pipeline: render to a temp dir and validate, so a break in gen-assets,
# rsvg rasterization, the hero SVG or the demo tape is caught before push (the
# assets.yml workflow runs the same render path). Runs automatically when its
# tooling is present (rsvg-convert + vhs + Berkeley Mono in fontconfig); prints
# a notice otherwise so the default run stays portable. Reuses the debug binary.
# Count berkeley faces without a `| grep -q` pipe: grep -q exits on first match
# and SIGPIPEs fc-list, which under `set -o pipefail` would make the condition
# false and skip this gate silently. Read to EOF (grep -ci) and test the count.
have_berkeley="$(fc-list 2>/dev/null | grep -ci berkeley || true)"
if command -v rsvg-convert >/dev/null 2>&1 && command -v vhs >/dev/null 2>&1 \
    && [[ "${have_berkeley:-0}" -gt 0 ]]; then
    step "asset pipeline (render + validate; matches assets.yml)"
    asset_tmp="$(mktemp -d)"
    PURPLE_BIN="$(pwd)/target/debug/purple" PNG_DIR="$asset_tmp/png" HERO_OUT="$asset_tmp/hero.svg" \
        ./scripts/render-assets.sh >/dev/null
    [[ "$(head -c 64 "$asset_tmp/hero.svg")" == *"<svg"* ]] && grep -q '</svg>' "$asset_tmp/hero.svg" \
        || { echo -e "${RED}    hero.svg is not a complete SVG${RESET}"; exit 1; }
    png_count=0
    for p in "$asset_tmp"/png/*.png; do
        { [[ -s "$p" ]] && [[ "$(file "$p")" == *"PNG image data"* ]]; } \
            || { echo -e "${RED}    invalid PNG: $p${RESET}"; exit 1; }
        png_count=$((png_count + 1))
    done
    [[ "$png_count" -ge 1 ]] || { echo -e "${RED}    no PNGs rendered${RESET}"; exit 1; }
    vhs validate assets/tapes/demo.tape
    rm -rf "$asset_tmp"
    echo "    rendered + validated $png_count PNGs, hero.svg and the demo tape"
    pass
else
    echo -e "${RED}    NOTE: asset pipeline gate skipped (needs rsvg-convert + vhs + Berkeley Mono on PATH)${RESET}"
fi

if [[ "$WITH_DOCKER" == "true" ]]; then
    if ! docker info >/dev/null 2>&1; then
        echo -e "${RED}Docker daemon not running. Skip --with-docker or start Docker.${RESET}"
        exit 1
    fi
    step "docker rust:latest cargo clippy --locked --all-targets -- -D warnings (Linux, CI-matching toolchain)"
    docker run --rm -v "$(pwd)":/work -w /work rust:latest \
        bash -c "cargo clippy --locked --all-targets -- -D warnings"
    pass

    step "docker rust:latest cargo test (Linux, CI-matching toolchain)"
    docker run --rm -v "$(pwd)":/work -w /work rust:latest \
        bash -c "cargo test --quiet"
    pass
fi

if [[ "$WITH_NIX" == "true" ]]; then
    if ! command -v nix >/dev/null 2>&1; then
        echo -e "${RED}nix is not installed. Install via https://nixos.org/download or skip --with-nix.${RESET}"
        exit 1
    fi
    step "nix flake check (validates flake builds purple-ssh and cargo fmt passes)"
    nix flake check
    pass
fi

echo ""
echo -e "${GREEN}All $gate_num pre-commit gates passed.${RESET}"
