// Writes the constructed testnet example in examples/testnet/ from conformance/vectors.json, so
// every charter byte, amount and memo in it comes from @bonbagi/core, not from this repository.
//   node scripts/make-examples.mjs
// The txids are placeholders (SHA-256 of a label): this is not a real wallet. A recorded live
// scan, when there is one, goes in examples/testnet-live/.

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";

const v = JSON.parse(readFileSync(new URL("../conformance/vectors.json", import.meta.url), "utf8"));
const out = (name, text) => writeFileSync(new URL(`../examples/testnet/${name}`, import.meta.url), text);
const charter = (name) => v.charters.find((c) => c.name === name);
const txid = (label) => createHash("sha256").update(`bonbagi-watcher constructed example: ${label}`).digest("hex");

const demo = charter("demo2");
const fp = demo.hash.slice(0, 16);
const paid = v.requests.find((r) => r.charter === "demo2" && r.order === "ord-k7m2qx9p");
const memo = (order, m) => `bonbagi:v1 o=${order} c=${fp} m=${m}/2`;

out("charter.json", JSON.stringify(demo.file, null, 2) + "\n");
out("dex-key.TEST-ONLY.txt", v.keys[0].priv + "\n");
out("paylink.txt", v.paylinks.find((l) => l.charter === "demo2").url + "\n");

const snapshot = {
  bonbagi_snapshot: 1,
  network: "test",
  tip_height: 4540420,
  note: "Constructed example, not a real wallet: what member 1 (Dex) of charter.json would see. Charter, amounts and memos come from @bonbagi/core (conformance/vectors.json); txids are placeholders.",
  outputs: [
    // 0.003 TAZ order paid as the pay page builds it: Dex's 60% line of the ZIP-321 request.
    { txid: txid("ord-k7m2qx9p"), height: 4540210, output_index: 0, pool: "Ironwood", value_zat: Number(paid.lines[0].amount_zat), memo: paid.lines[0].memo },
    // A 0.001 TAZ order: 60% = 0.0006.
    { txid: txid("ord-9xq2ttmh"), height: 4540388, output_index: 1, pool: "Orchard", value_zat: 60000, memo: memo("ord-9xq2ttmh", 1) },
    // Addresses swapped by whoever built the request: Hans's output (member 2/2) came to Dex.
    { txid: txid("ord-swapped1"), height: 4540401, output_index: 0, pool: "Ironwood", value_zat: 120000, memo: memo("ord-swapped1", 2) },
    // Broadcast, not mined yet: 0.0015 TAZ order, 60% = 0.0009.
    { txid: txid("ord-pnd3w8kq"), height: null, output_index: 0, pool: "Ironwood", value_zat: 90000, memo: memo("ord-pnd3w8kq", 1) },
    // Another collective's charter (the three-member one in the vectors): not ours, left out.
    { txid: txid("other-charter"), height: 4540100, output_index: 0, pool: "Orchard", value_zat: 40000, memo: `bonbagi:v1 o=ord-c4fe0001 c=${charter("unicode3").hash.slice(0, 16)} m=1/3` },
    // An ordinary payment with a text memo: not BonBagi, ignored.
    { txid: txid("not-bonbagi"), height: 4539990, output_index: 0, pool: "Sapling", value_zat: 500000, memo: "thanks for the coffee" },
  ],
};
out("snapshot.json", JSON.stringify(snapshot, null, 2) + "\n");
