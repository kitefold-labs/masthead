#!/usr/bin/env bash
# Copyright 2026 Kitefold Labs
# SPDX-License-Identifier: Apache-2.0
#
# Fail if a tracked file that can carry a comment lacks the copyright header.
# Files that cannot (images, fonts, JSON, Cargo.lock, NOTICE) are covered by
# REUSE.toml instead.
set -euo pipefail
cd "$(dirname "$0")/.."

missing=$(git ls-files -z -- . \
    ':!:*.png' ':!:*.ttf' ':!:*.json' ':!:Cargo.lock' ':!:NOTICE' ':!:REUSE.toml' \
    ':!:LICENSE' ':!:LICENSES/*' ':!:crates/masthead-window/fonts/OFL-*.txt' |
    xargs -0 grep -L -e 'Copyright [0-9]\{4\} Kitefold Labs' || true)
missing_spdx=$(git ls-files -z -- . \
    ':!:*.png' ':!:*.ttf' ':!:*.json' ':!:Cargo.lock' ':!:NOTICE' ':!:REUSE.toml' \
    ':!:LICENSE' ':!:LICENSES/*' ':!:crates/masthead-window/fonts/OFL-*.txt' |
    xargs -0 grep -L -e 'SPDX-License-Identifier: Apache-2.0' || true)

if [ -n "$missing$missing_spdx" ]; then
    echo "Files missing the header:"
    printf '%s\n%s\n' "$missing" "$missing_spdx" | sort -u | sed '/^$/d; s/^/  /'
    echo "Add these two lines in the file's comment syntax:"
    echo "  Copyright $(date +%Y) Kitefold Labs"
    echo "  SPDX-License-Identifier: Apache-2.0"
    exit 1
fi
echo "Every file carries the copyright header."
