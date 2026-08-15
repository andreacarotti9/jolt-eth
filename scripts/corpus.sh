# shellcheck shell=bash
# Sourced by the campaign scripts, so it has no shebang of its own.
# The corpus: one workload per line, as `path::case-substring`.
#
# The case matters. An EEST file holds many cases and the first is often
# degenerate - ecpairing/valid.json opens with the *empty* input (no pairing at
# all) and p256verify/wycheproof_valid.json opens with an invalid public key -
# so each workload names the case it actually means.
corpus() {
  local root="$1"
  cat <<LIST
$root/amsterdam/eip2780_reduce_intrinsic_tx_gas/value_moving_transactions/value_moving_transactions.json::non-zero_value-recipient_type_RecipientType.EOA
$root/ported_static/vmTests/sha3/sha3.json::sha3_1024_1
$root/frontier/precompiles/ecrecover/precompiles.json::R_eq_2G_low_s
$root/byzantium/eip197_ec_pairing/ecpairing/valid.json::three_point_match_1
$root/byzantium/eip198_modexp_precompile/modexp/modexp.json::EIP-198-case1-mod-even-declared-length-128-bytes
$root/osaka/eip7951_p256verify_precompiles/p256verify/wycheproof_valid.json::EcdsaP1363Verify SHA-256 #1:
$root/ported_static/stInitCodeTest/transaction_create_random_init_code/transaction_create_random_init_code.json::
LIST
}
