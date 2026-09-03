#!/bin/sh
# Copies docs/CHANGELOG.md into the web bundle (web/src/lib/changelog.ts).
cd "$(dirname "$0")/.." && python3 - <<'PY'
import json
md=open('docs/CHANGELOG.md').read()
open('web/src/lib/changelog.ts','w').write('// generated from docs/CHANGELOG.md by scripts/sync-changelog.sh — do not edit by hand\nexport const CHANGELOG = ' + json.dumps(md) + ';\nexport const LATEST_VERSION = "' + md.split('\n## ')[1].split(' ')[0] + '";\n')
PY
