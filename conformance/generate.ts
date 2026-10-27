// Generates conformance/vectors.json from the TypeScript core that the BonBagi web app runs
// (@bonbagi/core). The Rust watcher re-implements the parts it needs (canonical charter bytes,
// charter hash, ZIP-215 signature checks, the zatoshi split, the memo and the pay link) and its
// tests replay these vectors, so the two implementations cannot drift apart unnoticed.
//
//   node conformance/generate.ts ../core/src > conformance/vectors.json
//
// The core imports @noble/ed25519, so run it from a checkout where that package is installed
// (a BonBagi/core checkout after `npm ci`). Everything here is deterministic:
// fixed private keys, fixed order IDs, fixed prices.

import { join } from "node:path";
import { pathToFileURL } from "node:url";

const coreDir = process.argv[2];
if (!coreDir) {
  console.error("usage: node conformance/generate.ts <core src dir>");
  process.exit(2);
}
const load = (file: string) => import(pathToFileURL(join(coreDir, file)).href);
const charter = await load("charter.ts");
const split = await load("split.ts");
const memo = await load("memo.ts");
const paylink = await load("paylink.ts");
const request = await load("request.ts");

const keys = await Promise.all(
  ["01", "02", "03"].map(async (b) => {
    const priv = b.repeat(32);
    return { priv, pub: await charter.publicKeyOf(priv) };
  }),
);

// Made-up addresses with the right prefixes and alphabet. They fail a wallet's checksum, so they
// can never receive funds; the watcher (like the web app) only checks prefix and alphabet.
const fake = (hrp: string, seed: string) => {
  const alphabet = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
  let out = "";
  let x = [...seed].reduce((a, c) => (a * 31 + c.charCodeAt(0)) >>> 0, 7);
  for (let i = 0; i < 100; i++) {
    x = (Math.imul(x, 1103515245) + 12345) >>> 0;
    out += alphabet[(x >>> 16) % 32];
  }
  return `${hrp}1${out}`;
};

const charters: Record<string, any> = {
  demo2: {
    bonbagi: 1,
    network: "test",
    title: "Kopi Kita demo",
    minShareZat: "10000",
    members: [
      { label: "Dex", address: fake("utest", "dex"), bps: 6000, pubkey: keys[0]!.pub },
      { label: "Hans", address: fake("utest", "hans"), bps: 4000, pubkey: keys[1]!.pub },
    ],
  },
  // Title in decomposed form (e + combining acute) and a non-ASCII label: canonical bytes are NFC.
  unicode3: {
    bonbagi: 1,
    network: "test",
    title: "Café Bersama",
    minShareZat: "10000",
    members: [
      { label: "Ayu ☕", address: fake("utest", "ayu"), bps: 4000, pubkey: keys[0]!.pub },
      { label: "Bima", address: fake("utest", "bima"), bps: 3000, pubkey: keys[1]!.pub },
      { label: "Citra", address: fake("ztestsapling", "citra"), bps: 3000, pubkey: keys[2]!.pub },
    ],
  },
  main3: {
    bonbagi: 1,
    network: "main",
    title: "Thirds",
    minShareZat: "10000",
    members: [
      { label: "A", address: fake("u", "a"), bps: 3333, pubkey: keys[0]!.pub },
      { label: "B", address: fake("u", "b"), bps: 3333, pubkey: keys[1]!.pub },
      { label: "C", address: fake("zs", "c"), bps: 3334, pubkey: keys[2]!.pub },
    ],
  },
};

async function signAll(c: any, privs: string[]) {
  let file = { charter: c, signatures: [] as any[] };
  for (const p of privs) file = await charter.addSignature(file, p);
  return file;
}

const signed: Record<string, any> = {};
for (const [name, c] of Object.entries(charters)) {
  signed[name] = await signAll(c, keys.slice(0, c.members.length).map((k) => k.priv));
}

const charterVectors = [];
const describe = async (name: string, file: any) => {
  const v = await charter.verifyCharter(file);
  charterVectors.push({
    name,
    file,
    canonical: charter.canonicalJson(file.charter),
    hash: v.hash,
    unsigned: v.unsigned,
  });
};
for (const [name, file] of Object.entries(signed)) await describe(name, file);
// Hans's share edited after signing: both signatures stop verifying.
const tampered = structuredClone(signed.demo2);
tampered.charter.members[0].bps = 5999;
tampered.charter.members[1].bps = 4001;
await describe("demo2-tampered", tampered);
// Only the first member signed.
await describe("demo2-partial", { charter: signed.demo2.charter, signatures: [signed.demo2.signatures[0]] });

const splitCases: [bigint, number[], bigint][] = [
  [300_000n, [6000, 4000], 10_000n],
  [300_001n, [6000, 4000], 10_000n],
  [299_999n, [6000, 4000], 10_000n],
  [1_000_001n, [4000, 3000, 3000], 10_000n],
  [1_000_000n, [3333, 3333, 3334], 10_000n],
  [100n, [3333, 3333, 3334], 1n],
  [2_100_000_000_000_000n, [1, 9999], 10_000n],
  [15_000n, [6000, 4000], 10_000n],
  [300_000n, [6000, 3000], 10_000n],
];
const splits = splitCases.map(([total, bps, min]) => {
  try {
    return { total: String(total), bps, min: String(min), out: split.splitAmount(total, bps, min).map(String) };
  } catch (e) {
    return { total: String(total), bps, min: String(min), error: (e as Error).message };
  }
});

const requests = [];
for (const [name, price, order] of [
  ["demo2", 300_000n, "ord-k7m2qx9p"],
  ["unicode3", 1_000_001n, "ord-t3stq4rm"],
  ["main3", 1_000_000n, "ord-mzn8w2ra"],
] as const) {
  const r = await request.buildPaymentRequest(signed[name], price, order);
  requests.push({
    charter: name,
    price_zat: String(price),
    order,
    uri: r.uri,
    lines: r.lines.map((l: any) => ({ label: l.label, address: l.address, amount_zat: String(l.amountZat), memo: l.memo })),
  });
}

const paylinks = [
  { charter: "demo2", price_zat: "300000" },
  { charter: "unicode3" },
].map(({ charter: name, price_zat }) => {
  const fragment = paylink.encodePayFragment(signed[name], price_zat === undefined ? undefined : BigInt(price_zat));
  const back = paylink.decodePayFragment(fragment);
  if (JSON.stringify(back.signed) !== JSON.stringify(signed[name])) throw new Error("pay link does not round-trip");
  return { charter: name, price_zat: price_zat ?? null, url: `https://bonbagi.vercel.app/pay.html#${fragment}` };
});

const memos = [
  ["ord-k7m2qx9p", "3b25c168a1f0e2d4c5b6a7980112233445566778899aabbccddeeff001122334", 1, 2],
  ["A-1", "f".repeat(64), 50, 50],
].map(([o, h, i, n]) => ({ order: o, hash: h, member: i, of: n, memo: memo.bonbagiMemo(o, h, i, n) }));

process.stdout.write(
  JSON.stringify(
    {
      about: "Conformance vectors from @bonbagi/core (the TypeScript the BonBagi web app runs). Regenerate with conformance/generate.ts; the Rust tests replay them.",
      keys,
      charters: charterVectors,
      splits,
      requests,
      paylinks,
      memos,
    },
    null,
    2,
  ) + "\n",
);
