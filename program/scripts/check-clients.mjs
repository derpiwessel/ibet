#!/usr/bin/env node
// Checks that every client sends the account list the program actually wants.
//
//   node program/scripts/check-clients.mjs
//
// Three separate clients hardcode these lists — the site, the keeper and the
// smoke test — and the compiler checks none of them. Phase 3 made `config`
// writable on every instruction, and all three were left sending it read-only.
// Nothing failed until a real transaction hit mainnet, because devnet was
// still running the older program. This is that missing check.
//
// Signer and writable flags are compared against target/idl/ibet_escrow.json,
// which Anchor regenerates from the Accounts structs on every build. Run it
// after any change to those structs. An entry it cannot parse is a failure,
// not a pass — a silent skip here is exactly the hole this is closing.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '..', '..');
const IDL = JSON.parse(fs.readFileSync(path.join(here, '..', 'target', 'idl', 'ibet_escrow.json'), 'utf8'));

const want = (name) => {
  const ix = IDL.instructions.find((i) => i.name === name);
  if (!ix) throw new Error(`${name} is not in the IDL`);
  return ix.accounts.map((a) => ({ name: a.name, signer: !!a.signer, writable: !!a.writable }));
};

let failures = 0;
const read = (rel) => fs.readFileSync(path.join(root, rel), 'utf8');

/// Compares one client's list against the IDL, positionally.
function check(file, ixName, got) {
  const expected = want(ixName);
  const label = `${file} ${ixName}`;
  if (got.length !== expected.length) {
    console.log(`  ✗ ${label}: sends ${got.length} accounts, the program wants ${expected.length}`);
    failures += 1;
    return;
  }
  // Signer has to match exactly: demanding a signature nobody gave fails the
  // transaction, and omitting one the program needs fails it too.
  //
  // Writable only has to cover what the program needs. Handing over more than
  // that is allowed — the fee payer, for one, must be writable whether or not
  // the Accounts struct says `mut`, which is why the resolver and the caller
  // are sent writable on instructions that never write to them.
  const wrong = expected
    .map((e, i) => {
      if (e.signer !== got[i].signer) {
        return `${e.name} should be signer=${e.signer}, sends signer=${got[i].signer}`;
      }
      if (e.writable && !got[i].writable) {
        return `${e.name} must be writable and is sent read-only`;
      }
      return null;
    })
    .filter(Boolean);
  if (wrong.length) {
    console.log(`  ✗ ${label}:`);
    wrong.forEach((w) => console.log(`      ${w}`));
    failures += wrong.length;
  } else {
    console.log(`  ✓ ${label}`);
  }
}

/// Pulls the first [...] list out of a function body and reads its triples.
/// index.html writes them as [pubkey, isSigner, isWritable] with 1 and 0.
function siteList(src, fnName) {
  const fn = src.slice(src.indexOf(`function ${fnName}(`));
  const body = fn.slice(0, fn.indexOf('\n  }'));
  // The only bracketed triples in these bodies are the account entries, so
  // scanning the whole body beats trying to balance the outer brackets.
  const triples = [...body.matchAll(/\[\s*([^[\],]+?)\s*,\s*([01])\s*,\s*([01])\s*\]/g)];
  if (!triples.length) throw new Error(`could not parse the account list in ${fnName}`);
  return triples.map((t) => ({ signer: t[2] === '1', writable: t[3] === '1' }));
}

/// keeper/index.ts and smoke.mjs both spell the flags out in words.
function namedList(src, anchor, style) {
  const from = src.indexOf(anchor);
  if (from < 0) throw new Error(`could not find ${anchor}`);
  const body = src.slice(from, src.indexOf('],', from));
  // The pubkey argument can itself be a call with commas in it, so one level
  // of nested parentheses has to be allowed before the two flags.
  const re = style === 'keeper'
    ? /isSigner:\s*(true|false),\s*isWritable:\s*(true|false)/g
    : /meta\(\s*(?:[^(),]|\([^()]*\))+\s*,\s*(true|false)\s*,\s*(true|false)\s*\)/g;
  const found = [...body.matchAll(re)];
  if (!found.length) throw new Error(`could not parse the account list at ${anchor}`);
  return found.map((f) => ({ signer: f[1] === 'true', writable: f[2] === 'true' }));
}

try {
  console.log('\n  index.html');
  const site = read('index.html');
  check('index.html', 'create_bet', siteList(site, 'ixCreateBet'));
  check('index.html', 'take_bet', siteList(site, 'ixTakeBet'));
  check('index.html', 'cancel_bet', siteList(site, 'ixCancelBet'));
  check('index.html', 'settle_bet', siteList(site, 'ixSettleBet'));
  check('index.html', 'refund_expired', siteList(site, 'ixRefund'));

  console.log('\n  keeper/index.ts');
  check('keeper', 'settle_bet', namedList(read('keeper/index.ts'), 'keys: [', 'keeper'));

  console.log('\n  program/scripts/smoke.mjs');
  const smoke = read('program/scripts/smoke.mjs');
  check('smoke.mjs', 'create_bet', namedList(smoke, 'function ixCreate(', 'meta'));
  check('smoke.mjs', 'take_bet', namedList(smoke, 'const ixTake =', 'meta'));
  check('smoke.mjs', 'cancel_bet', namedList(smoke, 'const ixCancel =', 'meta'));
} catch (e) {
  console.log(`\n  ✗ ${e.message}`);
  failures += 1;
}

console.log(failures
  ? `\n  ${failures} mismatch${failures === 1 ? '' : 'es'} — the clients disagree with the program.\n`
  : '\n  Every client sends what the program wants.\n');
process.exit(failures ? 1 : 0);
