import * as BunnySDK from "@bunny.net/edgescript-sdk";

// Embedded copy of site/install.sh (source of truth).
// Must stay in sync — CI checks for drift on every PR and push (site.yml).
const INSTALL_SCRIPT = `#!/bin/sh
# Source of truth for the install script.
# Also embedded in worker.ts — keep both in sync.
# CI checks for drift on every PR and push (site.yml).
set -eu

REPO="erickochen/purple"
BINARY="purple"

main() {
    printf "\\n  \\033[1mpurple.\\033[0m installer\\n\\n"

    # Detect OS (before dependency checks so non-macOS gets a clear message)
    os="$(uname -s)"
    case "$os" in
        Darwin) ;;
        Linux)
            printf "  \\033[1m!\\033[0m Pre-built binaries are macOS only for now.\\n"
            printf "  Install via cargo instead:\\n\\n"
            printf "    cargo install purple-ssh\\n\\n"
            exit 1
            ;;
        *)
            printf "  \\033[1m!\\033[0m Unsupported OS: %s\\n" "$os"
            printf "  Install via cargo instead:\\n\\n"
            printf "    cargo install purple-ssh\\n\\n"
            exit 1
            ;;
    esac

    # Check dependencies (after OS detection so non-macOS exits with a clear message)
    need_cmd curl
    need_cmd tar
    need_cmd shasum

    # Detect architecture
    arch="$(uname -m)"
    case "$arch" in
        arm64|aarch64) target="aarch64-apple-darwin" ;;
        x86_64)        target="x86_64-apple-darwin" ;;
        *)
            printf "  \\033[1m!\\033[0m Unsupported architecture: %s\\n" "$arch"
            exit 1
            ;;
    esac

    # Get latest version
    printf "  Fetching latest release...\\n"
    version="$(curl -fsSL "https://api.github.com/repos/\${REPO}/releases/latest" \\
        | grep '"tag_name"' | head -1 | sed 's/.*"v\\(.*\\)".*/\\1/')"

    if [ -z "$version" ] || ! printf '%s' "$version" | grep -qE '^[0-9]+\\.[0-9]+\\.[0-9]+$'; then
        printf "  \\033[1m!\\033[0m Failed to fetch latest version.\\n"
        printf "  GitHub API may be rate-limited. Try again later or install via:\\n\\n"
        printf "    brew install erickochen/purple/purple\\n\\n"
        exit 1
    fi

    printf "  Found v%s for %s\\n" "$version" "$target"

    # Set up temp directory
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    tarball="purple-\${version}-\${target}.tar.gz"
    url="https://github.com/\${REPO}/releases/download/v\${version}/\${tarball}"
    sha_url="\${url}.sha256"

    # Download tarball and checksum
    printf "  Downloading...\\n"
    curl -fsSL "$url" -o "\${tmp}/\${tarball}"
    curl -fsSL "$sha_url" -o "\${tmp}/\${tarball}.sha256"

    # Verify checksum
    printf "  Verifying checksum...\\n"
    expected="$(awk '{print $1}' "\${tmp}/\${tarball}.sha256")"
    actual="$(shasum -a 256 "\${tmp}/\${tarball}" | awk '{print $1}')"

    if [ "$expected" != "$actual" ]; then
        printf "  \\033[1m!\\033[0m Checksum mismatch.\\n"
        printf "    Expected: %s\\n" "$expected"
        printf "    Got:      %s\\n" "$actual"
        exit 1
    fi

    # Extract
    tar -xzf "\${tmp}/\${tarball}" -C "$tmp"

    # Install
    install_dir="/usr/local/bin"
    if [ ! -w "$install_dir" ]; then
        install_dir="\${HOME}/.local/bin"
        mkdir -p "$install_dir"
    fi

    cp "\${tmp}/\${BINARY}" "\${install_dir}/\${BINARY}"
    chmod 755 "\${install_dir}/\${BINARY}"

    printf "\\n  \\033[1;35mpurple v%s\\033[0m installed to %s/%s\\n\\n" \\
        "$version" "$install_dir" "$BINARY"

    printf "  To update later, run: purple update\\n\\n"

    # Check PATH
    case ":\${PATH}:" in
        *":\${install_dir}:"*) ;;
        *)
            printf "  Add %s to your PATH:\\n\\n" "$install_dir"
            printf "    export PATH=\\"%s:\\$PATH\\"\\n\\n" "$install_dir"
            ;;
    esac
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        printf "  \\033[1m!\\033[0m Required command not found: %s\\n" "$1"
        exit 1
    fi
}

main "$@"
`;

const LANDING_PAGE = `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>purple. \u2014 SSH config manager and launcher</title>
<meta name="description" content="Smart, fast SSH launcher for the terminal. TUI host manager with search, tags, tunnels, cloud sync and round-trip fidelity for ~/.ssh/config.">
<meta property="og:title" content="purple. \u2014 SSH config manager and launcher">
<meta property="og:description" content="Smart, fast SSH launcher for the terminal. TUI with search, tags, tunnels, ping and cloud sync.">
<meta property="og:type" content="website">
<meta property="og:url" content="https://getpurple.sh">
<meta property="og:image" content="https://raw.githubusercontent.com/erickochen/purple/master/demo.gif">
<meta name="twitter:card" content="summary_large_image">
<link rel="canonical" href="https://getpurple.sh">
<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  background: #0a0a0a;
  color: #e0e0e0;
  font-family: "SF Mono", "Fira Code", "JetBrains Mono", "Cascadia Code", Menlo, Monaco, "Courier New", monospace;
  line-height: 1.6;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
}
main {
  max-width: 720px;
  width: 100%;
  padding: 80px 24px 60px;
}
h1 {
  font-size: 3rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  margin-bottom: 8px;
}
h1 span { color: #9333ea; }
.tagline {
  color: #888;
  font-size: 1rem;
  margin-bottom: 48px;
}
.install-box {
  background: #161616;
  border: 1px solid #2a2a2a;
  border-radius: 8px;
  padding: 20px 24px;
  margin-bottom: 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}
.install-box code {
  font-size: 1rem;
  color: #fff;
  white-space: nowrap;
}
.install-box code .dim { color: #777; }
.copy-btn {
  background: none;
  border: 1px solid #333;
  border-radius: 6px;
  color: #888;
  padding: 6px 12px;
  font-family: inherit;
  font-size: 0.8rem;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}
.copy-btn:hover { border-color: #9333ea; color: #fff; }
.alt-methods {
  color: #555;
  font-size: 0.85rem;
  margin-bottom: 56px;
  line-height: 1.8;
}
.alt-methods a {
  color: #888;
  text-decoration: none;
  border-bottom: 1px solid #333;
  transition: all 0.15s;
}
.alt-methods a:hover { color: #9333ea; border-color: #9333ea; }
.demo {
  margin-bottom: 56px;
}
.demo img {
  width: 100%;
  border-radius: 8px;
  border: 1px solid #2a2a2a;
}
.features {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px 32px;
  margin-bottom: 56px;
  font-size: 0.9rem;
}
.features div {
  color: #888;
}
.features div strong {
  color: #e0e0e0;
  font-weight: 600;
}
footer {
  color: #444;
  font-size: 0.8rem;
  padding-bottom: 40px;
}
footer a {
  color: #555;
  text-decoration: none;
  border-bottom: 1px solid #333;
  transition: all 0.15s;
}
footer a:hover { color: #9333ea; border-color: #9333ea; }
@media (max-width: 540px) {
  main { padding: 48px 16px 40px; }
  h1 { font-size: 2.2rem; }
  .install-box { padding: 16px; }
  .install-box code { font-size: 0.85rem; }
  .features { grid-template-columns: 1fr; }
}
</style>
</head>
<body>
<main>
  <h1>purple<span>.</span></h1>
  <p class="tagline">SSH config manager and launcher for the terminal</p>

  <div class="install-box">
    <code><span class="dim">$</span> curl -fsSL getpurple.sh | sh</code>
    <button class="copy-btn" onclick="copy(this)">copy</button>
  </div>

  <div class="alt-methods">
    or via <a href="https://github.com/erickochen/homebrew-purple">Homebrew</a>:
    brew install erickochen/purple/purple<br>
    or via <a href="https://crates.io/crates/purple-ssh">cargo</a>:
    cargo install purple-ssh
  </div>

  <div class="demo">
    <img src="https://raw.githubusercontent.com/erickochen/purple/master/demo.gif"
         alt="purple TUI demo" loading="lazy">
  </div>

  <div class="features">
    <div><strong>Search</strong> \u2014 fuzzy find across aliases, hostnames and tags</div>
    <div><strong>Tags</strong> \u2014 organize hosts with #tags and filter instantly</div>
    <div><strong>Tunnels</strong> \u2014 manage SSH port forwards per host</div>
    <div><strong>Ping</strong> \u2014 check host reachability from the TUI</div>
    <div><strong>Cloud sync</strong> \u2014 DigitalOcean, Vultr, Linode, Hetzner, UpCloud, Proxmox VE</div>
    <div><strong>Round-trip</strong> \u2014 preserves comments, formatting and unknown directives</div>
    <div><strong>Self-update</strong> \u2014 run purple update, with startup version check</div>
  </div>

  </main>

<footer>
  <a href="https://github.com/erickochen/purple">GitHub</a>
</footer>

<script>
function copy(btn) {
  navigator.clipboard.writeText("curl -fsSL getpurple.sh | sh").then(function() {
    btn.textContent = "copied";
    setTimeout(function() { btn.textContent = "copy"; }, 2000);
  }).catch(function() {});
}
<\/script>
</body>
</html>`;

BunnySDK.net.http.serve(async (request: Request): Promise<Response> => {
  // Redirect purple-ssh.com → getpurple.sh
  const host = request.headers.get("host") || "";
  if (host === "purple-ssh.com" || host === "www.purple-ssh.com" || host === "www.getpurple.sh") {
    const url = new URL(request.url);
    return Response.redirect(`https://getpurple.sh${url.pathname}${url.search}`, 301);
  }

  const ua = (request.headers.get("user-agent") || "").toLowerCase();
  const isCli =
    ua.startsWith("curl") ||
    ua.startsWith("wget") ||
    ua.startsWith("fetch") ||
    ua.startsWith("httpie");

  if (isCli) {
    return new Response(INSTALL_SCRIPT, {
      headers: {
        "content-type": "text/plain; charset=utf-8",
        "cache-control": "public, max-age=300",
      },
    });
  }

  return new Response(LANDING_PAGE, {
    headers: {
      "content-type": "text/html; charset=utf-8",
      "cache-control": "public, max-age=3600",
    },
  });
});
