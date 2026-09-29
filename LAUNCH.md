# Mainnet launch runbook

Every step here moves real money or real authority. Nothing in this file has
been executed. Work through it in order — each step's verification has to pass
before the next one starts.

Two roles appear below. **Owner** means steps only you can do, because they need
your wallets to sign. **Dev** means steps the developer runs, each one asked for
and approved first.

Program on devnet stays exactly as it is throughout, so there is always a
working environment to test against.

---

## 0. Before anything

| | |
|---|---|
| Fee wallet | `EDiALz4fYTJcNaTcoAuuo6X5rWsvys199dnwgeWCyW5Q` |
| Mainnet RPC | Helius, key restricted to `i-bet.fun` and `www.i-bet.fun` ✅ |
| Second source | Birdeye, key in Supabase Vault ✅ |
| Alerts | Discord webhook in Supabase Vault, delivery tested ✅ |
| Caps | 0.01–0.1 SOL per side, 10 SOL total escrow |

Still open: the terms text is a developer-written placeholder in `terms.html`
and has not been through a lawyer. The phase 3 brief geo-blocks NL because
offering real-money betting there needs a Kansspelautoriteit licence.

---

## 1. Create the multisig — **Owner**

2-of-3 on Squads, mainnet. Signers:

| role | address |
|---|---|
| Phantom | `EDiALz4fYTJcNaTcoAuuo6X5rWsvys199dnwgeWCyW5Q` |
| Backup wallet | `Gv3y2h57Kr6nHaBLoGJrPqHSRGxAHHbaFMy8vcu1tnTf` |
| Friend | `8xE9b1krmF6WDUmbYjErkMhghtxedtdwEUVHtqfhmBfq` |

All three verified as valid, distinct, on-curve wallet addresses.

1. Go to <https://app.squads.so> on **mainnet** and connect your Phantom.
2. Create a new Squad. Add the two other addresses as members.
3. Set the approval threshold to **2**.
4. Confirm and pay the rent (a fraction of a SOL).
5. Copy two addresses from the Squad's settings:
   - the **multisig address** (the Squad itself), and
   - the **vault address** (often labelled "Vault 1" or the treasury).

> The vault address is the one that holds authorities and funds. Transferring
> the upgrade authority to the multisig address instead of the vault is the
> classic way to lose a program permanently. When in doubt, send a tiny amount
> of SOL to the vault first and check it arrives.

6. Have the friend open the Squad once and confirm they can see it. A 2-of-3
   where the third signer never logged in is really a 2-of-2.

**Give the developer both addresses.** Neither is secret.

---

## 2. Fund the deploy wallet — **Owner**, needs approval

The developer generates a fresh mainnet deploy keypair and gives you its
address. You send **≈ 3 SOL**:

- ~1.3 SOL becomes program rent, returned if the program is ever closed
- ~1.3 SOL is the temporary upgrade buffer, returned immediately after deploy
- the rest covers transaction fees

This wallet holds authority only until step 4, and nothing afterwards.

---

## 3. Deploy and initialise — **Dev**, needs approval

```bash
cd program
anchor build --arch v0
solana program deploy target/deploy/ibet_escrow.so \
  --program-id target/deploy/ibet_escrow-keypair.json \
  --url mainnet-beta

node scripts/config.mjs init --cluster mainnet-beta \
  --owner EDiALz4fYTJcNaTcoAuuo6X5rWsvys199dnwgeWCyW5Q \
  --min 0.01 --max 0.1 --exposure 10
node scripts/config.mjs show --cluster mainnet-beta
```

Verify: fee 200 bps, stakes 0.01–0.1 SOL, exposure cap 10 SOL, grace 48h,
`open_exposure` 0, resolver and fee wallet both the owner's Phantom for now.

### Verifiable build

Needs Docker, which the development sandbox does not have, so this runs on a
machine that does:

```bash
cargo install solana-verify
solana-verify build --library-name ibet_escrow
solana-verify verify-from-repo --url mainnet-beta \
  --program-id <MAINNET_PROGRAM_ID> https://github.com/derpiwessel/ibet
```

The build has to match before step 6. If it does not, stop — a program nobody
can verify is a program nobody should put money in.

---

## 4. Hand over the keys — **Dev**, needs approval. One way.

```bash
# config admin -> the Squads vault
node scripts/config.mjs set --cluster mainnet-beta --admin <SQUADS_VAULT>

# program upgrade authority -> the Squads vault
solana program set-upgrade-authority <MAINNET_PROGRAM_ID> \
  --new-upgrade-authority <SQUADS_VAULT> \
  --skip-new-upgrade-authority-signer-check --url mainnet-beta
```

Verify before going further:

```bash
solana program show <MAINNET_PROGRAM_ID> --url mainnet-beta   # Authority = vault
node scripts/config.mjs show --cluster mainnet-beta           # admin = vault
solana balance <DEPLOY_WALLET> --url mainnet-beta             # only dust left
```

From here the developer cannot change the program or its limits. Every change
needs two of your three signers.

---

## 5. Keeper for mainnet — **Dev**, creating a secret needs approval

1. Deploy a mainnet copy of the keeper pointed at the mainnet program.
2. Call `?action=init-key` once. It generates its own signing key inside
   Supabase Vault and returns only the public key — nobody, including the
   developer, ever sees the secret half.
3. Fund that address with **≤ 0.05 SOL** for transaction fees.
4. Through the multisig, set `resolver` to that address.
5. Confirm a run reports `ok: true` and that Discord receives an alert.

**Rotation:** delete `ibet_keeper_key` in Supabase, call `?action=init-key`
again, fund the new address, and have the multisig point `resolver` at it. The
old key can do nothing once the config no longer names it.

---

## 6. Smoke test on mainnet — **Owner**, real SOL

Two of your own wallets, 0.01 SOL a side. Run all three paths:

| path | expected |
|---|---|
| create → take → touch | creator paid immediately, 2% fee to the fee wallet |
| create → take → expire | taker paid after the deadline, once candles cover it |
| create → cancel | stake and rent fully returned, no fee |

Check every lamport against `settlements`, and confirm each one produced a
Discord alert carrying both the deciding value and the second source's value.

---

## 7. Go live — **Dev**, merging to `main` needs approval

1. Set `CLUSTER = 'mainnet'` and fill in the mainnet `programId` in
   `index.html`.
2. Merge `phase-3` into `main`.
3. On i-bet.fun verify: the beta strip shows the real cap read off the chain,
   the 18+ gate appears before a first bet, a Dutch IP sees the country notice
   but can still reach its own bets, and the chart, logos and wizard are
   unchanged.
4. Previews stay on devnet. The Helius key refuses preview and localhost
   origins by design, so mainnet can only be exercised on the real domain.

---

## Where the keys live

| key | where | backup |
|---|---|---|
| Mainnet program keypair | `program/target/deploy/` (gitignored) | **Copy offline before step 3.** Lose it and the program can never be redeployed at that address. |
| Mainnet deploy wallet | `~/.config/solana/` | Holds nothing after step 4; keep it anyway |
| Keeper signing key | Supabase Vault only | None by design — rotate instead |
| Birdeye API key | Supabase Vault | Rotate: it passed through a chat transcript |
| Discord webhook | Supabase Vault | Rotate if the channel is ever shared |
| Devnet keypairs | `~/.config/solana/ibet-*` | Test only, no value |

Nothing above is in the repository, and `.gitignore` keeps it that way.

---

## If something goes wrong

- **Stop new bets:** the multisig sets `paused = true`. Cancel, settle and
  refund keep working — a pause can never trap money.
- **Settlement stalls:** every matched bet can be refunded by anyone 48 hours
  after its deadline, both sides whole, no fee. This needs no keeper and no
  authority.
- **Sources disagree:** the keeper refuses to settle and alerts. It retries
  every minute; if the disagreement persists, the 48-hour refund is the floor.
- **Keeper out of SOL:** settlements stop silently apart from the alert. Top it
  up; nothing is lost, the bets simply settle late.
- **Limits too tight:** only the multisig can raise them, and only upward
  through `update_config`.
