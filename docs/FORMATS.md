# Formats

Everything the watcher reads and writes. Amounts are zatoshi (1 ZEC = 100,000,000 zat); in
receipts they are decimal strings, so JavaScript can read them without losing precision.

## The BonBagi memo

Each member's output in a BonBagi payment carries this UTF-8 text memo, built by
`bonbagiMemo` in `@bonbagi/core`. Only that member's wallet can decrypt it.

```text
bonbagi:v1 o=<order ID> c=<charter fingerprint> m=<member>/<members>
bonbagi:v1 o=ord-k7m2qx9p c=4857f20a3cac5fe5 m=1/2
```

| Field | Rule |
|---|---|
| `o` | order reference, 1–32 letters, digits or dashes; the pay page makes `ord-` + 8 random characters |
| `c` | first 16 hex characters of the charter hash (below) |
| `m` | this output's member number and the number of members, as in the charter's order |

The watcher accepts exactly this shape and nothing else; any other memo is not a BonBagi memo.

## Charter hash and signatures

```text
message = "bonbagi-charter-v1\n" + canonicalJson(charter)
hash    = SHA-256(message), lowercase hex
```

`canonicalJson` sorts object keys at every level, writes no whitespace, NFC-normalises strings,
escapes them as `JSON.stringify` does, and allows only safe integers. Every member signs
`message` with Ed25519; signatures are checked with the ZIP 215 rules (the `@noble/ed25519`
default the web app uses). The watcher refuses a charter that is not signed by every member.
`conformance/vectors.json` has worked examples.

## Snapshot (input of `check --snapshot`, output of `--save-snapshot`)

The outputs one wallet received, nothing else. A live scan writes only received outputs whose
memo starts with `bonbagi:`.

```json
{
  "bonbagi_snapshot": 1,
  "network": "test",
  "tip_height": 4540420,
  "note": "optional free text, shown in the report",
  "outputs": [
    {
      "txid": "95ad566a4093ec8b90bdb835edc903fd70b405d82c9a54c8103e875b87a3d6de",
      "height": 4540210,
      "output_index": 0,
      "pool": "Ironwood",
      "value_zat": 180000,
      "memo": "bonbagi:v1 o=ord-k7m2qx9p c=4857f20a3cac5fe5 m=1/2"
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `network` | `main` or `test`; must match the charter |
| `tip_height` | chain tip the wallet had synced to; confirmations = `tip_height - height + 1` |
| `height` | block height, `null` while not mined |
| `pool` | as the wallet names it: `Sapling`, `Orchard`, `Ironwood`, or `Transparent` (flagged) |
| `memo` | text memo, `null` if none |
| `expired` | optional, `true` for a transaction that was never mined and is past its expiry |

Unknown fields are rejected, so a typo does not silently change the result.

## Receipt (output of `check --receipt`, input of `verify-receipt`)

```json
{
  "receipt": {
    "amount_zat": "180000",
    "bonbagi_receipt": 1,
    "charter": "4857f20a3cac5fe5ef24baef43630b9bda10c4cf3ce7ba199d612e5fc3b68d1b",
    "checked_at": "2026-10-27T05:40:00Z",
    "confirmations": 211,
    "expected_zat": "180000",
    "height": 4540210,
    "label": "Rani",
    "member": 1,
    "network": "test",
    "of": 2,
    "order": "ord-k7m2qx9p",
    "output_index": 0,
    "pool": "Ironwood",
    "pubkey": "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c",
    "source": "snapshot",
    "title": "Kopi Kita demo",
    "txid": "95ad566a4093ec8b90bdb835edc903fd70b405d82c9a54c8103e875b87a3d6de"
  },
  "sig": "<128 hex: Ed25519 signature>"
}
```

```text
signed bytes = "bonbagi-receipt-v1\n" + canonicalJson(receipt)
```

`pubkey` is the member's charter key; the watcher only signs when the key file belongs to the
member the receipt is about. `expected_zat` is present when a price was known. `source` is
`live` or `snapshot`. A receipt is issued only for a PAID result of one `--order`.

`verify-receipt` checks the signature, that `amount_zat` equals `expected_zat` when the receipt
states one, and, given the charter, that the charter hash matches, that `pubkey` is member
`member` of `of`, the network, and that the amount is a possible share. Without the charter it
answers NOT VERIFIED (exit 1): anyone can sign with a fresh key, only the charter says whose
key is a member's.

### Checking a receipt in TypeScript

The same canonical form as the charter, so `@bonbagi/core` and `@noble/ed25519` are enough:

```ts
import * as ed from "@noble/ed25519";
import { canonicalJson } from "@bonbagi/core";

const { receipt, sig } = JSON.parse(text);
const msg = new TextEncoder().encode("bonbagi-receipt-v1\n" + canonicalJson(receipt));
const signatureOk = await ed.verifyAsync(sig, msg, receipt.pubkey);
// then: receipt.charter === hash of your charter, and receipt.pubkey is member receipt.member
```

[`conformance/verify-receipt.ts`](../conformance/verify-receipt.ts) runs exactly this against
the golden receipt: `true` for `examples/testnet/expected/receipt.json`, `false` once any field
is changed.

## Report (`check --json`)

`examples/testnet/expected/order-paid.json` is a full example. Top level: `charter` (title,
network, hash, fingerprint, members, fully_signed), `member` (index, of, label, bps, pubkey),
`source` (kind `snapshot` or `live`, with path or wallet folder, server, connection,
elapsed_ms, tip_height), `order`, `price_zat`, `price_from` (`flag` or `pay_link`),
`min_conf`, `payments[]` (order, status, amount_zat, expected_zat, implied_total_zat
{min, max}, txid, output_index, pool, height, confirmations, problems[], notes[]),
`other_charters`, `other_orders`, `verdict` (`paid`, `pending`, `not_found`, `mismatch`),
`summary`, `receipt`.
