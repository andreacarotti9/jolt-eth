#!/usr/bin/env bash
# Clone every upstream repo at the pin recorded in UPSTREAM.md.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p upstream && cd upstream

clone() { # repo sha dir
  [ -d "$3" ] || git clone --filter=blob:none -q "https://github.com/$1.git" "$3"
  git -C "$3" fetch -q origin "$2" 2>/dev/null || true
  git -C "$3" checkout -q "$2"
  echo "$3 @ $(git -C "$3" rev-parse --short HEAD)"
}

clone a16z/jolt                        915faf453f36871249615a7fdf2704d77a88f259 jolt
clone eth-act/ere                      552595938b48884d87cfada24f29d39223695c1e ere
clone eth-act/ere-guests               817fae8d693f72297c6785fff47acaca5855a6ef ere-guests
clone eth-act/zkvm-standards           282cd356c3a0498416bb0619f9c8a347ce9933fb zkvm-standards
clone eth-act/zkevm-benchmark-workload 228a9228b1424c2216c78352763558ce1a7a268c zkevm-benchmark-workload
clone succinctlabs/rsp                 44401e52ecd3df81ee603355c0f09f9d64e030e4 rsp
