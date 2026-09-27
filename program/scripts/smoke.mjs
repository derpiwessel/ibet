#!/usr/bin/env node
// End-to-end check against a deployed program: creates a bet, takes it from a
// second wallet, creates and cancels a third, and then watches the keeper
// settle a bet early on a touch. Lamports are asserted exactly.
//
//   node scripts/smoke.mjs [--cluster devnet] [--keypair PATH] [--stake 0.01]
//                          [--skip-early] [--wait 300]
//
// The funding wallet pays for a throwaway taker, so this needs roughly
// stake * 3 + 0.08 SOL to run. The taker-wins path is not covered here: a bet
// cannot expire faster than its shortest timeframe of one day, so that one is
// proven by the Rust suite and by letting a 1-day bet run.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  Connection, Keypair, PublicKey, SystemProgram, Transaction, TransactionInstruction,
} from '@solana/web3.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const IDL = JSON.parse(fs.readFileSync(path.join(here, '..', 'target', 'idl', 'ibet_escrow.json'), 'utf8'));
const PROGRAM_ID = new PublicKey(IDL.address);
const LAMPORTS = 1_000_000_000;
const DAY = 86_400;

const CLUSTERS = {
  devnet: 'https://api.devnet.solana.com',
  testnet: 'https://api.testnet.solana.com',
  localnet: 'http://127.0.0.1:8899',
};

const [, , ...rest] = process.argv;
const flags = {};
for (let i = 0; i < rest.length; i += 1) {
  if (!rest[i].startsWith('--')) continue;
  const key = rest[i].slice(2);
  const next = rest[i + 1];
  flags[key] = next && !next.startsWith('--') ? (i += 1, next) : 'true';
}
const cluster = flags.cluster ?? 'devnet';
const url = flags.url ?? CLUSTERS[cluster] ?? cluster;
const stake = Math.round(Number(flags.stake ?? 0.01) * LAMPORTS);

const die = (m) => { console.error(`\n  ${m}\n`); process.exit(1); };

class Buf {
  constructor() { this.b = []; }
  disc(name) {
    const ix = IDL.instructions.find((i) => i.name === name);
    this.b.push(...ix.discriminator);
    return this;
  }
  u8(v) { this.b.push(v & 0xff); return this; }
  u64(v) { let n = BigInt(v); for (let i = 0; i < 8; i += 1) { this.b.push(Number(n & 0xffn)); n >>= 8n; } return this; }
  i64(v) { return this.u64(v); }
  key(k) { this.b.push(...new PublicKey(k).toBytes()); return this; }
  out() { return Buffer.from(this.b); }
}

const configPda = () => PublicKey.findProgramAddressSync([Buffer.from('config')], PROGRAM_ID)[0];
const betPda = (creator, betId) => PublicKey.findProgramAddressSync(
  [Buffer.from('bet'), new PublicKey(creator).toBytes(), new Buf().u64(betId).out()],
  PROGRAM_ID,
)[0];

const meta = (pubkey, isSigner, isWritable) => ({ pubkey: new PublicKey(pubkey), isSigner, isWritable });

function ixCreate(creator, betId, args) {
  const data = new Buf().disc('create_bet')
    .u64(betId).key(args.mint).u8(args.direction)
    .u64(args.target).u64(args.start).u64(args.stake).i64(args.duration)
    .out();
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      meta(creator, true, true),
      meta(configPda(), false, false),
      meta(betPda(creator, betId), false, true),
      meta(SystemProgram.programId, false, false),
    ],
    data,
  });
}
const ixTake = (taker, bet) => new TransactionInstruction({
  programId: PROGRAM_ID,
  keys: [
    meta(taker, true, true),
    meta(configPda(), false, false),
    meta(bet, false, true),
    meta(SystemProgram.programId, false, false),
  ],
  data: new Buf().disc('take_bet').out(),
});
const ixCancel = (creator, bet) => new TransactionInstruction({
  programId: PROGRAM_ID,
  keys: [meta(creator, true, true), meta(bet, false, true)],
  data: new Buf().disc('cancel_bet').out(),
});

class Rd {
  constructor(b) { this.a = b; this.o = 0; }
  skip(n) { this.o += n; return this; }
  u8() { return this.a[this.o++]; }
  u64() { const v = this.a.readBigUInt64LE(this.o); this.o += 8; return Number(v); }
  i64() { const v = this.a.readBigInt64LE(this.o); this.o += 8; return Number(v); }
  key() { const v = new PublicKey(this.a.subarray(this.o, this.o + 32)); this.o += 32; return v; }
  optKey() { return this.u8() ? this.key() : null; }
}
const STATUS = ['Open', 'Matched', 'Settled', 'Cancelled', 'Refunded'];

async function readBet(connection, pda) {
  const info = await connection.getAccountInfo(pda);
  if (!info) return null;
  const r = new Rd(info.data).skip(8);
  const creator = r.key();
  const taker = r.optKey();
  const mint = r.key();
  const direction = r.u8();
  const target = r.u64();
  r.u64(); // start mcap
  const betStake = r.u64();
  r.i64(); // duration
  r.i64(); // created
  r.i64(); // taken
  const expiresAt = r.i64();
  const status = STATUS[r.u8()];
  return { creator, taker, mint, direction, target, stake: betStake, expiresAt, status, lamports: info.lamports };
}

async function send(connection, signers, ix, label) {
  const tx = new Transaction().add(ix);
  tx.feePayer = signers[0].publicKey;
  tx.recentBlockhash = (await connection.getLatestBlockhash('confirmed')).blockhash;
  tx.sign(...signers);
  const sig = await connection.sendRawTransaction(tx.serialize());
  await connection.confirmTransaction(sig, 'confirmed');
  console.log(`  ${label.padEnd(14)} ${sig}`);
  return sig;
}

const pass = (ok, msg) => {
  console.log(`  ${ok ? '✓' : '✗'} ${msg}`);
  if (!ok) process.exitCode = 1;
};

async function main() {
  const connection = new Connection(url, 'confirmed');
  const keypairPath = (flags.keypair ?? '~/.config/solana/ibet-devnet-deployer.json').replace(/^~/, process.env.HOME);
  if (!fs.existsSync(keypairPath)) die(`No keypair at ${keypairPath}`);
  const creator = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(keypairPath, 'utf8'))));

  const cfg = await connection.getAccountInfo(configPda());
  if (!cfg) die('No config account — run `config.mjs init` first.');

  console.log(`\n  program  ${PROGRAM_ID.toBase58()}\n  cluster  ${cluster}\n  creator  ${creator.publicKey.toBase58()}`);
  console.log(`  stake    ${stake / LAMPORTS} SOL\n`);

  // A throwaway taker, funded just enough for one stake plus fees.
  const taker = Keypair.generate();
  await send(connection, [creator], SystemProgram.transfer({
    fromPubkey: creator.publicKey, toPubkey: taker.publicKey, lamports: stake + 20_000_000,
  }), 'fund taker');

  const mint = Keypair.generate().publicKey; // stands in for a coin address
  const rentFor = (n) => connection.getMinimumBalanceForRentExemption(n);
  const rent = await rentFor(221);

  // ── create and take ──────────────────────────────────────────────────────
  const idA = Date.now();
  const betA = betPda(creator.publicKey, idA);
  await send(connection, [creator], ixCreate(creator.publicKey, idA, {
    mint, direction: 0, target: 5_000_000, start: 1_000_000, stake, duration: 7 * DAY,
  }), 'create A');

  let a = await readBet(connection, betA);
  pass(a?.status === 'Open', `bet A is Open (${a?.status})`);
  pass(a?.lamports === rent + stake, `bet A holds rent + one stake (${a?.lamports} vs ${rent + stake})`);

  const takerBefore = await connection.getBalance(taker.publicKey);
  await send(connection, [taker], ixTake(taker.publicKey, betA), 'take A');

  a = await readBet(connection, betA);
  const takerAfter = await connection.getBalance(taker.publicKey);
  pass(a?.status === 'Matched', `bet A is Matched (${a?.status})`);
  pass(a?.taker?.equals(taker.publicKey) === true, 'bet A records the taker');
  pass(a?.lamports === rent + stake * 2, `bet A holds both stakes (${a?.lamports} vs ${rent + stake * 2})`);
  pass(a?.expiresAt > 0, `bet A clock started (expires ${new Date(a.expiresAt * 1000).toISOString()})`);
  // The taker paid one stake plus a 5000 lamport signature fee.
  pass(takerBefore - takerAfter === stake + 5_000, `taker paid exactly one stake + fee (${takerBefore - takerAfter})`);

  // ── create and cancel ────────────────────────────────────────────────────
  const idB = Date.now() + 1;
  const betB = betPda(creator.publicKey, idB);
  const beforeB = await connection.getBalance(creator.publicKey);
  await send(connection, [creator], ixCreate(creator.publicKey, idB, {
    mint, direction: 1, target: 400_000, start: 1_000_000, stake, duration: 1 * DAY,
  }), 'create B');
  await send(connection, [creator], ixCancel(creator.publicKey, betB), 'cancel B');

  const afterB = await connection.getBalance(creator.publicKey);
  pass((await readBet(connection, betB)) === null, 'bet B account is closed');
  // Two signature fees, and the stake and rent both came back.
  pass(beforeB - afterB === 10_000, `cancel returned the stake and the rent (net ${beforeB - afterB} lamports of fees)`);

  // ── the keeper settles a touch early ─────────────────────────────────────
  if (flags['skip-early']) {
    console.log('\n  Skipping the early-win check.\n');
    return;
  }

  const CA = flags.coin ?? 'GTBxUiw6wJdmmkCGZgRHLyYxqu1vG4KtRpeox6yDpump';
  const coin = new PublicKey(CA);
  const dex = await (await fetch(`https://api.dexscreener.com/latest/dex/tokens/${CA}`)).json();
  const top = (dex.pairs ?? []).filter((p) => p.chainId === 'solana')
    .sort((a, b) => (b.liquidity?.usd ?? 0) - (a.liquidity?.usd ?? 0))[0];
  if (!top) die(`No pool found for ${CA}`);
  const liveMcap = Number(top.fdv);
  console.log(`\n  Live mcap for ${top.baseToken?.symbol ?? CA}: $${Math.round(liveMcap).toLocaleString()}`);

  // A target just under the live market cap, so the very next candle touches it.
  const idC = Date.now() + 2;
  const betC = betPda(creator.publicKey, idC);
  await send(connection, [creator], ixCreate(creator.publicKey, idC, {
    mint: coin,
    direction: 0,
    target: Math.round(liveMcap * 0.99),
    start: Math.round(liveMcap * 0.98),
    stake,
    duration: 7 * DAY,
  }), 'create C');
  await send(connection, [taker], ixTake(taker.publicKey, betC), 'take C');

  const creatorBefore = await connection.getBalance(creator.publicKey);
  const waitSecs = Number(flags.wait ?? 300);
  console.log(`  waiting up to ${waitSecs}s for the keeper to settle it early…`);

  const started = Date.now();
  let settled = false;
  while ((Date.now() - started) / 1000 < waitSecs) {
    await new Promise((r) => setTimeout(r, 10_000));
    if (!(await readBet(connection, betC))) { settled = true; break; }
    process.stdout.write('.');
  }
  console.log('');

  pass(settled, `bet C was settled by the keeper without anyone signing (${Math.round((Date.now() - started) / 1000)}s)`);
  if (settled) {
    const creatorAfter = await connection.getBalance(creator.publicKey);
    // Creator wins: back comes the rent plus the pot less the platform fee.
    const fee = Math.floor(stake * 2 * 200 / 10_000);
    pass(
      creatorAfter - creatorBefore === rent + stake * 2 - fee,
      `creator received the pot less the 2% fee (${creatorAfter - creatorBefore} lamports)`,
    );
    console.log(`\n  https://explorer.solana.com/address/${betC.toBase58()}?cluster=${cluster}`);
  }

  console.log(`\n  Bet A is still live on chain:`);
  console.log(`  https://explorer.solana.com/address/${betA.toBase58()}?cluster=${cluster}\n`);
}

main().catch((e) => die(e.stack || String(e)));
