#!/usr/bin/env bash
# Fetch the EEST corpus and extract exactly the fixtures scripts/corpus.sh names.
#
# The full archive is ~500 MB compressed and several GB expanded, so only the
# files the campaign reads are unpacked. Widen the corpus, not this script.
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/corpus.sh

ARCHIVE=bench/fixtures/fixtures_zkevm.tar.gz
URL="https://github.com/ethereum/execution-specs/releases/download/tests-zkevm@v0.6.2/fixtures_zkevm.tar.gz"
ROOT=fixtures/blockchain_tests/for_amsterdam

mkdir -p bench/fixtures/eest
[ -f "$ARCHIVE" ] || curl -sSL -o "$ARCHIVE" "$URL"

# corpus() emits `path::case`; the archive wants paths relative to its own root.
list=$(mktemp)
corpus "$ROOT" | sed 's|::.*||' > "$list"
tar -xzf "$ARCHIVE" -C bench/fixtures/eest -T "$list"
rm -f "$list"

find bench/fixtures/eest -name '*.json' | sort
