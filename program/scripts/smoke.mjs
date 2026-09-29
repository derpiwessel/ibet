#!/usr/bin/env node
// End-to-end check against a deployed program. Three paths, in order:
//
//   1. create -> cancel                      stake and rent come back, no fee
//   2. create -> take -> touch               creator paid early, 2% fee taken
//   3. create -> take -> expire (5 min)      taker paid after the deadline
//
// Lamports are asserted exactly, including the fee wallet's share, and no bet
// is left open at the end — which matters on mainnet, where a dangling matched
// bet would sit in escrow until the 48-hour refund.
//
//   node scripts/smoke.mjs [--cluster devnet|mainnet-beta] [--keypair PATH]
//                          [--stake 0.01] [--wait 300] [--expire-wait 900]
//                          [--only 1,2,3] [--coin CA]
//
// The funding wallet also pays for a throwaway taker and sweeps whatever is
// left in it back at the end, so only one wallet needs funding: about
// stake * 3 + 0.05 SOL, nearly all of which returns.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  Connection, Keypair, PublicKey, SystemProgram, Transaction, TransactionInstruction,
} from '@solana/web3.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const IDL = JSON.parse(fs.readFileSync(path.join(here, '..', 'target', 'idl', 'ibet_escrow.json'), 'utf8'));
const LAMPORTS = 1_000_000_000;
const DAY = 86_400;
const MINUTE = 60;

const CLUSTERS = {
  devnet: 'https://api.devnet.solana.com',
  testnet: 'https://api.testnet.solana.com',
  'mainnet-beta': 'https://api.mainnet-beta.solana.com',
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
// The IDL names the mainnet program; devnet's older deployment is reachable
// through --program, so this script can be rehearsed for free before it is
// pointed at real money.
const PROGRAM_ID = new PublicKey(flags.program ?? IDL.address);
const stake = Math.round(Number(flags.stake ?? 0.01) * LAMPORTS);

const die = (m) => { console.error(`\n  ${m}\n`); process.exit(1); };
// Explorer defaults to mainnet, and ?cluster=mainnet-beta is not a thing.
const explorer = (kind, id) =>
  `https://explorer.solana.com/${kind}/${id}` + (cluster === 'mainnet-beta' ? '' : `?cluster=${cluster}`);
const only = (flags.only ?? '1,2,3').split(',').map((n) => n.trim());

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

const bal = (connection, key) => tolerant('read balance', () => connection.getBalance(key));

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

/// Retries anything that talks to an RPC. A failed request means "could not
/// ask", never "the answer is no" — reading a dropped connection as a closed
/// account would report a settlement that never happened.
/// Patient on purpose: name resolution here drops out for tens of seconds at a
/// time, and giving up in the middle of a run with money in escrow is worse
/// than waiting. Roughly a minute of attempts before it admits defeat.
async function tolerant(label, fn, tries = 12) {
  let last;
  for (let i = 0; i < tries; i += 1) {
    try { return await fn(); } catch (e) { last = e; }
    process.stdout.write('!');
    await new Promise((r) => setTimeout(r, Math.min(2_000 * (i + 1), 6_000)));
  }
  throw new Error(`${label} failed ${tries} times: ${last?.message ?? last}`);
}

async function readBet(connection, pda) {
  const info = await tolerant('read bet', () => connection.getAccountInfo(pda));
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
  tx.recentBlockhash = (await tolerant('blockhash',
    () => connection.getLatestBlockhash('confirmed'))).blockhash;
  tx.sign(...signers);
  const sig = await tolerant('send', () => connection.sendRawTransaction(tx.serialize()));
  await tolerant('confirm', () => connection.confirmTransaction(sig, 'confirmed'));
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

  const cfg = await tolerant('read config', () => connection.getAccountInfo(configPda()));
  if (!cfg) die('No config account — run `config.mjs init` first.');

  console.log(`\n  program  ${PROGRAM_ID.toBase58()}\n  cluster  ${cluster}\n  creator  ${creator.publicKey.toBase58()}`);
  console.log(`  stake    ${stake / LAMPORTS} SOL\n`);

  // A throwaway taker, funded for one stake plus fees. Whatever is left goes
  // back to the funding wallet at the end, so only one wallet ever needs money.
  const taker = Keypair.generate();
  await send(connection, [creator], SystemProgram.transfer({
    fromPubkey: creator.publicKey, toPubkey: taker.publicKey, lamports: stake + 20_000_000,
  }), 'fund taker');

  const rent = await tolerant('rent', () => connection.getMinimumBalanceForRentExemption(221));
  const fee = Math.floor(stake * 2 * 200 / 10_000);
  const feeWallet = new Rd(cfg.data).skip(8).key() && (() => {
    const r = new Rd(cfg.data).skip(8);
    r.key(); r.key();              // admin, resolver
    return r.key();                // fee wallet
  })();
  console.log(`  fee wallet ${feeWallet.toBase58()}\n`);

  // A coin that actually trades, so paths 2 and 3 have candles to be judged on.
  const CA = flags.coin ?? 'GTBxUiw6wJdmmkCGZgRHLyYxqu1vG4KtRpeox6yDpump';
  const dex = await (await fetch(`https://api.dexscreener.com/latest/dex/tokens/${CA}`)).json();
  const top = (dex.pairs ?? []).filter((p) => p.chainId === 'solana')
    .sort((a, b) => (b.liquidity?.usd ?? 0) - (a.liquidity?.usd ?? 0))[0];
  if (!top) die(`No pool found for ${CA}`);
  const liveMcap = Number(top.fdv);
  const symbol = top.baseToken?.symbol ?? CA.slice(0, 6);
  console.log(`  ${symbol} is at $${Math.round(liveMcap).toLocaleString()}\n`);

  /// Polls until the bet account is gone, which is what settlement looks like
  /// from outside: the program closes the account when it pays out.
  async function waitSettled(pda, secs, label) {
    const started = Date.now();
    while ((Date.now() - started) / 1000 < secs) {
      await new Promise((r) => setTimeout(r, 10_000));
      if (!(await readBet(connection, pda))) {
        console.log(`  settled after ${Math.round((Date.now() - started) / 1000)}s`);
        return true;
      }
      process.stdout.write('.');
    }
    console.log('');
    pass(false, `${label} was not settled within ${secs}s`);
    return false;
  }

  // ── 1. create and cancel ─────────────────────────────────────────────────
  if (only.includes('1')) {
    console.log('  1. create -> cancel');
    const id = Date.now();
    const bet = betPda(creator.publicKey, id);
    const before = await bal(connection, creator.publicKey);
    await send(connection, [creator], ixCreate(creator.publicKey, id, {
      mint: new PublicKey(CA), direction: 1, target: Math.round(liveMcap * 0.5),
      start: Math.round(liveMcap), stake, duration: 7 * DAY,
    }), 'create');

    const open = await readBet(connection, bet);
    pass(open?.status === 'Open', `bet is Open (${open?.status})`);
    pass(open?.lamports === rent + stake, `holds rent + one stake (${open?.lamports} vs ${rent + stake})`);

    await send(connection, [creator], ixCancel(creator.publicKey, bet), 'cancel');
    const after = await bal(connection, creator.publicKey);
    pass((await readBet(connection, bet)) === null, 'the account is closed');
    // Two signature fees; the stake and the rent both came back, and no platform fee.
    pass(before - after === 10_000,
      `stake and rent returned, nothing else taken (net ${before - after} lamports of signature fees)`);
    console.log('');
  }

  // ── 2. create, take, and a touch settles it early ────────────────────────
  if (only.includes('2')) {
    console.log('  2. create -> take -> touch');
    const id = Date.now() + 1;
    const bet = betPda(creator.publicKey, id);
    // A target just under the live market cap, so the next candle reaches it.
    await send(connection, [creator], ixCreate(creator.publicKey, id, {
      mint: new PublicKey(CA), direction: 0, target: Math.round(liveMcap * 0.99),
      start: Math.round(liveMcap * 0.98), stake, duration: 7 * DAY,
    }), 'create');

    const takerBefore = await bal(connection, taker.publicKey);
    await send(connection, [taker], ixTake(taker.publicKey, bet), 'take');
    const matched = await readBet(connection, bet);
    const takerAfter = await bal(connection, taker.publicKey);
    pass(matched?.status === 'Matched', `bet is Matched (${matched?.status})`);
    pass(matched?.taker?.equals(taker.publicKey) === true, 'the taker is recorded');
    pass(matched?.lamports === rent + stake * 2,
      `holds both stakes (${matched?.lamports} vs ${rent + stake * 2})`);
    pass(matched?.expiresAt > 0, `the clock started (expires ${new Date(matched.expiresAt * 1000).toISOString()})`);
    pass(takerBefore - takerAfter === stake + 5_000,
      `taker paid exactly one stake + fee (${takerBefore - takerAfter})`);

    const creatorBefore = await bal(connection, creator.publicKey);
    const feeBefore = await bal(connection, feeWallet);
    console.log(`  waiting up to ${Number(flags.wait ?? 300)}s for the keeper…`);
    if (await waitSettled(bet, Number(flags.wait ?? 300), 'the touch bet')) {
      const creatorAfter = await bal(connection, creator.publicKey);
      const feeAfter = await bal(connection, feeWallet);
      pass(creatorAfter - creatorBefore === rent + stake * 2 - fee,
        `creator got the pot less the 2% fee and the rent back (${creatorAfter - creatorBefore})`);
      pass(feeAfter - feeBefore === fee, `the fee wallet got exactly ${fee} lamports (${feeAfter - feeBefore})`);
      console.log(`  ${explorer('address', bet.toBase58())}`);
    }
    console.log('');
  }

  // ── 3. create, take, and let it expire ───────────────────────────────────
  if (only.includes('3')) {
    console.log('  3. create -> take -> expire');
    const id = Date.now() + 2;
    const bet = betPda(creator.publicKey, id);
    // Five minutes, with a target far out of reach, so the only honest outcome
    // is a taker win once the candles cover the whole window.
    await send(connection, [creator], ixCreate(creator.publicKey, id, {
      mint: new PublicKey(CA), direction: 0, target: Math.round(liveMcap * 3),
      start: Math.round(liveMcap), stake, duration: 5 * MINUTE,
    }), 'create');
    await send(connection, [taker], ixTake(taker.publicKey, bet), 'take');

    const takerBefore = await bal(connection, taker.publicKey);
    const feeBefore = await bal(connection, feeWallet);
    const deadline = (await readBet(connection, bet)).expiresAt;
    console.log(`  deadline ${new Date(deadline * 1000).toISOString()}; the keeper waits for`);
    console.log(`  candles covering the whole window before it calls this one`);
    if (await waitSettled(bet, Number(flags['expire-wait'] ?? 900), 'the expiring bet')) {
      const takerAfter = await bal(connection, taker.publicKey);
      const feeAfter = await bal(connection, feeWallet);
      pass(takerAfter - takerBefore === stake * 2 - fee,
        `taker got the pot less the 2% fee (${takerAfter - takerBefore} vs ${stake * 2 - fee})`);
      pass(feeAfter - feeBefore === fee, `the fee wallet got exactly ${fee} lamports (${feeAfter - feeBefore})`);
      console.log(`  ${explorer('address', bet.toBase58())}`);
    }
    console.log('');
  }

  // ── nothing left behind ──────────────────────────────────────────────────
  const dust = await bal(connection, taker.publicKey);
  if (dust > 5_000) {
    await send(connection, [taker], SystemProgram.transfer({
      fromPubkey: taker.publicKey, toPubkey: creator.publicKey, lamports: dust - 5_000,
    }), 'sweep taker');
  }
  console.log(`  funding wallet now holds ${(await bal(connection, creator.publicKey)) / LAMPORTS} SOL`);
  console.log(`  ${process.exitCode ? 'SOME CHECKS FAILED' : 'every check passed'}\n`);
}

main().catch((e) => die(e.stack || String(e)));
