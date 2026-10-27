// Checks watcher receipts with @bonbagi/core's canonicalJson and @noble/ed25519, the rules the
// browser uses. Proves a receipt signed in Rust verifies in TypeScript.
//   node conformance/verify-receipt.ts <path to a @bonbagi/core checkout with node_modules> <receipt.json>...
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
const core = process.argv[2];
const { canonicalJson } = await import(pathToFileURL(core + "/src/charter.ts").href);
const ed = await import(pathToFileURL(core + "/node_modules/@noble/ed25519/index.js").href);
for (const f of process.argv.slice(3)) {
  const { receipt, sig } = JSON.parse(readFileSync(f, "utf8"));
  const msg = new TextEncoder().encode("bonbagi-receipt-v1\n" + canonicalJson(receipt));
  console.log(f.split("/").pop(), await ed.verifyAsync(sig, msg, receipt.pubkey));
}
