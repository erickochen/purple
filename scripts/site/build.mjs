#!/usr/bin/env node
// Generate site/worker.generated.ts for deploy.
//
// The content lives in site/page.html, site/install.sh and llms.txt (which
// carry `__VERSION__`, `__DATE__` and `__DATE_HUMAN__` placeholders). This
// script injects the version + build date, then embeds the three documents
// into the worker template (site/worker.ts, which holds the server logic and
// empty constants). The committed worker.ts is never deployed;
// worker.generated.ts is. Run from CI (site.yml) before the Bunny deploy, or
// locally to preview.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const siteDir = join(here, "..", "..", "site");
const repoRoot = join(here, "..", "..");

// The deploy passes the latest published release in PURPLE_RELEASE_VERSION,
// so the installer never points at a release that is still building.
const RELEASE_VERSION_PATTERN = /^\d+\.\d+\.\d+$/;

function resolveVersion() {
    const fromRelease = process.env.PURPLE_RELEASE_VERSION;
    if (fromRelease !== undefined) {
        if (!RELEASE_VERSION_PATTERN.test(fromRelease)) {
            throw new Error(`PURPLE_RELEASE_VERSION must look like 1.2.3, got "${fromRelease}"`);
        }
        return { version: fromRelease, source: "latest release" };
    }
    // Local previews and PR checks fall back to the [package] version line,
    // the only line-leading `version = "..."` in Cargo.toml.
    const cargo = readFileSync(join(repoRoot, "Cargo.toml"), "utf8");
    const versionMatch = cargo.match(/^version = "([^"]+)"/m);
    if (!versionMatch) {
        throw new Error("could not find package version in Cargo.toml");
    }
    return { version: versionMatch[1], source: "Cargo.toml" };
}

const { version, source: versionSource } = resolveVersion();

// Build date in UTC. `dateModified` reflects the last deploy; the human form
// ("May 2026") feeds the llms.txt status line.
const now = new Date();
const isoDate = now.toISOString().slice(0, 10);
const humanDate = now.toLocaleString("en-US", {
    month: "long",
    year: "numeric",
    timeZone: "UTC",
});

// The hero animation is a committed artifact generated from the live TUI
// (`purple gen-assets --hero-out assets/hero.svg`). Inlining it means no
// extra request and the SVG text renders in the page's own webfonts.
const heroPath = join(repoRoot, "assets", "hero.svg");
if (!existsSync(heroPath)) {
    throw new Error(
        `assets/hero.svg is missing. Regenerate it with: purple gen-assets --hero-out assets/hero.svg (looked in ${heroPath})`,
    );
}
const heroSvg = readFileSync(heroPath, "utf8").trim();
if (!heroSvg.startsWith("<svg") || !heroSvg.endsWith("</svg>")) {
    throw new Error("assets/hero.svg is not a complete SVG document");
}

function inject(text) {
    return text
        .replaceAll("__HERO_SVG__", heroSvg)
        .replaceAll("__VERSION__", version)
        .replaceAll("__DATE_HUMAN__", humanDate)
        .replaceAll("__DATE__", isoDate);
}

// Order matters: backslash first, otherwise we double-escape our own escapes.
function escapeForTemplate(raw) {
    return raw
        .replace(/\\/g, "\\\\")
        .replace(/`/g, "\\`")
        .replace(/\$\{/g, "\\${");
}

function replaceConstant(worker, constant, body) {
    const startMarker = `const ${constant} = \``;
    const start = worker.indexOf(startMarker);
    if (start === -1) {
        throw new Error(`${constant} not found in worker.ts`);
    }
    const bodyStart = start + startMarker.length;
    const end = worker.indexOf("`;", bodyStart);
    if (end === -1) {
        throw new Error(`closing backtick for ${constant} not found`);
    }
    return worker.slice(0, bodyStart) + body + worker.slice(end);
}

const sources = [
    { constant: "INSTALL_SCRIPT", file: join(siteDir, "install.sh") },
    { constant: "LANDING_PAGE", file: join(siteDir, "page.html") },
    { constant: "LLMS_TXT", file: join(repoRoot, "llms.txt") },
];

let worker = readFileSync(join(siteDir, "worker.ts"), "utf8");
for (const { constant, file } of sources) {
    const body = escapeForTemplate(inject(readFileSync(file, "utf8")));
    worker = replaceConstant(worker, constant, body);
}

// Safety net: fail loudly rather than deploy a broken site if a placeholder
// survived (source typo) or a document came back empty (missing source file).
for (const placeholder of ["__VERSION__", "__DATE__", "__DATE_HUMAN__", "__HERO_SVG__"]) {
    if (worker.includes(placeholder)) {
        throw new Error(`unresolved placeholder ${placeholder} in generated worker`);
    }
}
for (const { constant } of sources) {
    if (worker.includes(`const ${constant} = \`\`;`)) {
        throw new Error(`${constant} is empty in generated worker (source file missing?)`);
    }
}

const outPath = join(siteDir, "worker.generated.ts");
writeFileSync(outPath, worker);
process.stdout.write(
    `worker.generated.ts written (version ${version} from ${versionSource}, ${isoDate}).\n`,
);
