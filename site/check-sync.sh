#!/bin/sh
# Verify that source files match their embedded copies in worker.ts.
# Run this before committing changes to site/ or llms.txt.
# This is the same check that runs in CI (.github/workflows/site.yml).

set -e
cd "$(dirname "$0")"

fail=0

check() {
  name="$1"
  const="$2"
  file="$3"

  node -e "
    const fs = require('fs');
    const src = fs.readFileSync('worker.ts', 'utf8');
    const start = src.indexOf('const $const = \`');
    if (start === -1) { console.error('$const not found in worker.ts'); process.exit(1); }
    const bodyStart = src.indexOf('\`', start) + 1;
    const bodyEnd = src.indexOf('\`;', bodyStart);
    if (bodyEnd === -1) { console.error('Closing backtick not found'); process.exit(1); }
    const raw = src.substring(bodyStart, bodyEnd);
    let embedded = '';
    for (let i = 0; i < raw.length; i++) {
      if (raw[i] === '\\\\' && i + 1 < raw.length) {
        const next = raw[i + 1];
        if (next === '\\\\' || next === '\$' || next === '\`') { embedded += next; i++; continue; }
      }
      embedded += raw[i];
    }
    const standalone = fs.readFileSync('$file', 'utf8');
    if (embedded === standalone) {
      console.log('  $name: ok');
    } else {
      console.error('  $name: DRIFTED');
      process.exit(1);
    }
  " || fail=1
}

echo "Checking worker.ts embedded content sync..."
check "install.sh  <-> INSTALL_SCRIPT" "INSTALL_SCRIPT" "install.sh"
check "page.html    <-> LANDING_PAGE"  "LANDING_PAGE"   "page.html"
check "../llms.txt  <-> LLMS_TXT"      "LLMS_TXT"       "../llms.txt"

if [ "$fail" = "1" ]; then
  echo ""
  echo "FAILED: embedded content in worker.ts is out of sync."
  echo "Update worker.ts to match the source files before committing."
  exit 1
fi

echo "All checks passed."
