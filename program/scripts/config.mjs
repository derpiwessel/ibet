#!/usr/bin/env node
// Reads and writes the escrow's Config account from the command line.
//
//   node scripts/config.mjs show
//   node scripts/config.mjs init     --owner <PUBKEY>
//   node scripts/config.mjs set      --paused true
//   node scripts/config.mjs handover --owner <PUBKEY>
//
// The instruction data is built straight from target/idl/ibet_escrow.json, so
// this never drifts from the deployed program. Run `anchor build --arch v0`
// first if the IDL is missing.

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

const CLUSTERS = {
  devnet: 'https://api.devnet.solana.com',
  testnet: 'https://api.testnet.solana.com',
  'mainnet-beta': 'https://api.mainnet-beta.solana.com',
  localnet: 'http://127.0.0.1:8899',
};

// ── args ────────────────────────────────────────────────────────────────────
const [, , command, ...rest] = process.argv;
const flags = {};
for (let i = 0; i < rest.length; i += 1) {
  if (!rest[i].startsWith('--')) continue;
  const key = rest[i].slice(2);
  const next = rest[i + 1];
  flags[key] = next && !next.startsWith('--') ? (i += 1, next) : 'true';
}

const cluster = flags.cluster ?? 'devnet';
const url = flags.url ?? CLUSTERS[cluster] ?? cluster;
const keypairPath = (flags.keypair ?? '~/.config/solana/ibet-devnet-deployer.json')
  .replace(/^~/, process.env.HOME);

function die(msg) {
  console.error(`\n  ${msg}\n`);
  process.exit(1);
}

function loadKeypair() {
  if (!fs.existsSync(keypairPath)) die(`No keypair at ${keypairPath} (pass --keypair).`);
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(keypairPath, 'utf8'))));
}

function pubkeyFlag(name) {
  if (!flags[name]) die(`--${name} <PUBKEY> is required.`);
  try { return new PublicKey(flags[name]); } catch { return die(`--${name} is not a valid address.`); }
}

// ── borsh ───────────────────────────────────────────────────────────────────
class Buf {
  constructor() { this.b = []; }
  disc(name) {
    const ix = IDL.instructions.find((i) => i.name === name);
    if (!ix) die(`Instruction ${name} is not in the IDL.`);
    this.b.push(...ix.discriminator);
    return this;
  }
  u8(v) { this.b.push(v & 0xff); return this; }
  u16(v) { this.b.push(v & 0xff, (v >> 8) & 0xff); return this; }
  u64(v) { let n = BigInt(v); for (let i = 0; i < 8; i += 1) { this.b.push(Number(n & 0xffn)); n >>= 8n; } return this; }
  i64(v) { return this.u64(v); }
  bool(v) { this.b.push(v ? 1 : 0); return this; }
  key(k) { this.b.push(...new PublicKey(k).toBytes()); return this; }
  // Borsh options: one tag byte, then the value only when present.
  opt(present, write) { if (!present) { this.b.push(0); return this; } this.b.push(1); write(this); return this; }
  out() { return Buffer.from(this.b); }
}

class Rd {
  constructor(bytes) { this.a = bytes; this.o = 0; }
  skip(n) { this.o += n; return this; }
  u8() { return this.a[this.o++]; }
  u16() { const v = this.a.readUInt16LE(this.o); this.o += 2; return v; }
  u64() { const v = this.a.readBigUInt64LE(this.o); this.o += 8; return Number(v); }
  i64() { const v = this.a.readBigInt64LE(this.o); this.o += 8; return Number(v); }
  bool() { return this.u8() === 1; }
  key() { const v = new PublicKey(this.a.subarray(this.o, this.o + 32)); this.o += 32; return v; }
}

const configPda = () => PublicKey.findProgramAddressSync([Buffer.from('config')], PROGRAM_ID)[0];

async function readConfig(connection) {
  const info = await connection.getAccountInfo(configPda());
  if (!info) return null;
  const r = new Rd(info.data).skip(8);
  return {
    admin: r.key(), resolver: r.key(), feeWallet: r.key(), feeBps: r.u16(),
    minStake: r.u64(), maxStake: r.u64(), graceSecs: r.i64(), paused: r.bool(),
  };
}

function printConfig(c) {
  if (!c) { console.log('\n  No config account — the program has not been initialised yet.\n'); return; }
  console.log(`
  admin        ${c.admin.toBase58()}
  resolver     ${c.resolver.toBase58()}
  fee wallet   ${c.feeWallet.toBase58()}
  fee          ${c.feeBps} bps (${c.feeBps / 100}%)
  stake range  ${c.minStake / LAMPORTS} – ${c.maxStake / LAMPORTS} SOL
  grace        ${c.graceSecs}s (${(c.graceSecs / 3600).toFixed(1)}h)
  paused       ${c.paused}
`);
}

async function send(connection, payer, ix, label) {
  const tx = new Transaction().add(ix);
  tx.feePayer = payer.publicKey;
  tx.recentBlockhash = (await connection.getLatestBlockhash('confirmed')).blockhash;
  tx.sign(payer);
  const sig = await connection.sendRawTransaction(tx.serialize());
  await connection.confirmTransaction(sig, 'confirmed');
  console.log(`  ${label}: ${sig}`);
  console.log(`  https://explorer.solana.com/tx/${sig}?cluster=${cluster}`);
  return sig;
}

// ── commands ────────────────────────────────────────────────────────────────
async function main() {
  const connection = new Connection(url, 'confirmed');
  console.log(`\n  program  ${PROGRAM_ID.toBase58()}\n  config   ${configPda().toBase58()}\n  cluster  ${cluster} (${url})`);

  if (command === 'show') {
    printConfig(await readConfig(connection));
    return;
  }

  const payer = loadKeypair();
  console.log(`  signer   ${payer.publicKey.toBase58()}`);

  if (command === 'init') {
    if (await readConfig(connection)) die('Config already exists — use `set` to change it.');
    const owner = pubkeyFlag('owner');
    const feeBps = Number(flags['fee-bps'] ?? 200);
    const minStake = Math.round(Number(flags.min ?? 0.01) * LAMPORTS);
    const maxStake = Math.round(Number(flags.max ?? 5) * LAMPORTS);
    const graceSecs = Number(flags.grace ?? 48 * 3600);

    // The admin is whoever signs; resolver and fee wallet are set to the owner
    // straight away, so the owner can settle and collect from the first bet.
    const data = new Buf().disc('initialize_config')
      .key(owner).key(owner).u16(feeBps).u64(minStake).u64(maxStake).i64(graceSecs)
      .out();
    const ix = new TransactionInstruction({
      programId: PROGRAM_ID,
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: true },
        { pubkey: configPda(), isSigner: false, isWritable: true },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      ],
      data,
    });
    await send(connection, payer, ix, 'initialize_config');
    printConfig(await readConfig(connection));
    return;
  }

  if (command === 'set' || command === 'handover') {
    const current = await readConfig(connection);
    if (!current) die('No config account yet — run `init` first.');
    if (!current.admin.equals(payer.publicKey)) {
      die(`Only the admin can change the config. Admin is ${current.admin.toBase58()}.`);
    }

    // `handover` is just the one update that also moves admin rights away.
    if (command === 'handover') {
      const owner = pubkeyFlag('owner');
      flags.admin = owner.toBase58();
      flags.resolver ??= owner.toBase58();
      flags['fee-wallet'] ??= owner.toBase58();
    }

    const data = new Buf().disc('update_config')
      .opt(flags.admin, (b) => b.key(flags.admin))
      .opt(flags.resolver, (b) => b.key(flags.resolver))
      .opt(flags['fee-wallet'], (b) => b.key(flags['fee-wallet']))
      .opt(flags['fee-bps'], (b) => b.u16(Number(flags['fee-bps'])))
      .opt(flags.min, (b) => b.u64(Math.round(Number(flags.min) * LAMPORTS)))
      .opt(flags.max, (b) => b.u64(Math.round(Number(flags.max) * LAMPORTS)))
      .opt(flags.grace, (b) => b.i64(Number(flags.grace)))
      .opt(flags.paused, (b) => b.bool(flags.paused === 'true'))
      .out();
    const ix = new TransactionInstruction({
      programId: PROGRAM_ID,
      keys: [
        { pubkey: payer.publicKey, isSigner: true, isWritable: false },
        { pubkey: configPda(), isSigner: false, isWritable: true },
      ],
      data,
    });
    await send(connection, payer, ix, 'update_config');
    printConfig(await readConfig(connection));
    if (command === 'handover') {
      console.log('  Admin rights have moved. This keypair can no longer change the config.\n');
    }
    return;
  }

  die('Usage: config.mjs <show|init|set|handover> [--owner PUBKEY] [--cluster devnet] [--keypair PATH]');
}

main().catch((e) => die(e.stack || String(e)));
