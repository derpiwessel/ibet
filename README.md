# ibet — i-bet.fun

Two people stake SOL against each other on whether a Solana coin's market cap
ends **higher** or **lower** than a target by a deadline. An Anchor program
holds both stakes in escrow until the bet is settled, cancelled or refunded.

> **Devnet only.** The deployed program runs on Solana devnet with test SOL that
> has no value. Do not deploy this to mainnet before working through the
> [mainnet checklist](#mainnet-checklist).

| | |
|---|---|
| Frontend | `index.html` — one file, no build step, deployed by Vercel |
| Program | `program/` — Anchor workspace, crate `ibet_escrow` |
| Program ID | `GACUebjQB1zZWuQW3pkhQLzYJL9yMtU8dX4TsfRyugq1` (devnet) |
| Config PDA | `2TuYCqG6BoPJDX3pQYgPABBPNxBNVegaZMHbBeLN6eu8` |
| Backend | Supabase `ibet` (`bryjuhrovcgeurtlysmt`, eu-central-1) |

## Current devnet deployment

Live since 27 September 2026.

| | |
|---|---|
| Deploy tx | `67eNKNFEeLFtPhPL44HA4HbAUTy2o7gdQUZGk1twqtr8UQcbJzG3jnuUAwc3CpS8FvLzaaWUzWR6513oiCJwFmZn` |
| Config tx | `adnrkqLinNkVRNAz4mG2sgmZN1D4zTb2ozHcpimmc8gEqbFfjM53vM7nBh1BKSxuttKvjcP2JHf49XXz5x5dpS9` |
| Resolver, fee wallet | `EDiALz4fYTJcNaTcoAuuo6X5rWsvys199dnwgeWCyW5Q` (the owner) |
| Admin, upgrade authority | `GXckV6zEZwjMWgXdxd6bbEdBKARSGKKpWfp9GB4YcUNZ` (the deployer, **temporary**) |
| Fee | 200 bps · stakes 0.01–5 SOL · grace 48h |

Admin rights and the upgrade authority still sit with the deployer so the
program can be patched while it is being tested. Move both to the owner's
wallet with the two commands under
[Hand control to the owner's wallet](#hand-control-to-the-owners-wallet).

Two bets were left on chain by the deployment check: one open bet on JEANPHIL
(0.02 SOL) that can be taken or cancelled, and one matched bet whose taker was a
throwaway keypair that was not kept. The matched one is there to exercise
Resolve once it expires, or `refund_expired` 48 hours after that.

## Where the data lives

Bets are **not** stored in a database. They are accounts owned by the escrow
program, and the site reads them straight off the chain.

| | source |
|---|---|
| Open and live bets | `getProgramAccounts` on the program |
| Wallet balance | `getBalance` |
| Fee, stake limits, grace window, pause | the on-chain `Config` account |
| Settled history and leaderboard | the program's own `BetSettled` events, cached in `localStorage` |
| Sign-in, `admins`, `trending` | Supabase |

### Why the leaderboard reads events

A settled bet's account is closed on chain — that is what returns its rent — so
the only durable record of the result is the `BetSettled` event in that
transaction's logs. The site walks the program's signatures once, decodes the
events, and keeps them in `localStorage`, so later loads only fetch signatures
newer than the last one seen.

The trade-off is RPC log retention: a public devnet node prunes old ledger, so a
first-time visitor sees only recent history. That is acceptable for a devnet
test, and it keeps the chain as the single source of truth with nothing to drift
out of sync. If the leaderboard has to be complete for everyone, phase 2 should
add a real indexer (Helius, Geyser) or mirror confirmed settlements into
Supabase.

One other consequence of the on-chain layout: `Bet` stores the coin's address
but no ticker. A ticker typed for a coin that is not on the trending list is
remembered in that browser only; everyone else sees the shortened address.

## The program

`Config` (PDA `["config"]`) holds the admin, the resolver, the fee wallet, the
fee in basis points (capped at 5%), the stake range, the resolver's grace window
and a pause flag.

`Bet` (PDA `["bet", creator, bet_id]`) holds the escrowed lamports on top of its
own rent, along with both sides, the coin, the direction, the target and start
market caps, the stake, the timeline and the outcome.

| instruction | who | what |
|---|---|---|
| `initialize_config` | deployer → admin | creates the singleton config |
| `update_config` | admin | partial update; `None` leaves a field alone |
| `create_bet` | anyone | opens a bet, escrows the creator's stake |
| `take_bet` | anyone but the creator | matches it, escrows the second stake, starts the clock |
| `cancel_bet` | creator | pulls an untaken bet, stake and rent returned |
| `settle_bet` | resolver | inside the grace window: fee to the fee wallet, rest to the winner |
| `refund_expired` | anyone | after the grace window: both stakes back, no fee |

Rules worth knowing:

- Durations are whitelisted to 1, 3, 5, 7, 14 and 30 days.
- Landing **exactly** on the target counts as a win for the creator, in both
  directions.
- Payout destinations are pinned to the creator and taker stored on the bet and
  the fee wallet stored on the config. The resolver cannot redirect a payout by
  passing different accounts.
- `paused` blocks `create_bet` and `take_bet` only. Cancel, settle and refund
  keep working, so a pause can never trap money in escrow.
- Every matched bet ends in Settled or Refunded, and every open bet can be
  cancelled, so no path leaves funds stuck.

## Working on it

### Install the toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
curl -sSfL https://release.anza.xyz/stable/install | sh
cargo install --git https://github.com/coral-xyz/anchor avm --locked --force && avm install latest && avm use latest
```

Built and tested against Rust 1.98.1, Solana CLI 4.2.2, Anchor 1.2.0. Node 20 is
only needed for the config script.

### Build and test

```bash
cd program && anchor build --arch v0 && cargo test
```

`--arch v0` is not optional: Anchor 1.2 defaults to SBPF v3, which the LiteSVM
version the tests run on cannot load. Building v0 keeps the artifact the tests
load byte-identical to the one that gets deployed.

The suite runs on [LiteSVM](https://github.com/LiteSVM/litesvm) rather than a
local validator, because most paths need the clock moved forward days at a time
and a validator's clock only advances at wall-clock speed. One dedicated payer
covers every transaction fee, so the parties' balances move only through escrow
and are asserted to the lamport. 42 tests cover the happy paths, the fee
arithmetic at 2% and 0%, both exact-target edges, every rejection listed above,
the pause rules, and that no lamport is created or destroyed on any path.

### Deploy to devnet

```bash
solana config set --url devnet --keypair ~/.config/solana/ibet-devnet-deployer.json
solana airdrop 2                       # twice; deploy needs ~2.5 SOL of headroom
cd program && anchor build --arch v0
solana program deploy target/deploy/ibet_escrow.so \
  --program-id target/deploy/ibet_escrow-keypair.json
```

Then create the config — resolver and fee wallet go to the owner's wallet
immediately, the signer stays admin:

```bash
cd program && npm install
node scripts/config.mjs init --owner <OWNER_WALLET>
node scripts/config.mjs show
```

Defaults are fee 200 bps (2%), stakes 0.01–5 SOL, grace 48h. Override with
`--fee-bps`, `--min`, `--max`, `--grace`.

Check a deployment for real, against whichever cluster is configured:

```bash
node scripts/smoke.mjs
```

That creates a bet, takes it from a throwaway wallet and cancels a second one,
asserting every balance to the lamport. It needs about `stake * 2 + 0.05` SOL in
the funding wallet. Settling is not covered — only the resolver can sign that,
and a bet cannot expire faster than its shortest timeframe of one day.

Other config changes:

```bash
node scripts/config.mjs set --paused true          # stop new bets
node scripts/config.mjs set --fee-bps 100
node scripts/config.mjs handover --owner <WALLET>  # hands admin over, one way
```

### Hand control to the owner's wallet

Both steps are one-way from the deployer's point of view:

```bash
node scripts/config.mjs handover --owner <OWNER_WALLET>
solana program set-upgrade-authority GACUebjQB1zZWuQW3pkhQLzYJL9yMtU8dX4TsfRyugq1 \
  --new-upgrade-authority <OWNER_WALLET> --skip-new-upgrade-authority-signer-check
```

After that, patching the program or changing the config requires the owner's key
in the CLI, so do it once the deployment has been tested.

### Keypairs

Nothing secret is in the repo, and `.gitignore` keeps it that way.

- `program/target/deploy/ibet_escrow-keypair.json` — **the program's identity**.
  Losing it means the program can never be upgraded at this address again. Back
  it up outside the repo.
- `~/.config/solana/ibet-devnet-deployer.json` — the deployer wallet, currently
  admin and upgrade authority.

### Pointing the frontend somewhere else

One block at the top of the script in `index.html`:

```js
var CHAIN = {
  cluster:   'devnet',
  rpcUrl:    'https://api.devnet.solana.com',
  programId: 'GACUebjQB1zZWuQW3pkhQLzYJL9yMtU8dX4TsfRyugq1'
};
```

Nothing below that block knows which cluster it is on. Moving to mainnet means
changing these three values and deploying the same program there — everything
else, including the Anchor discriminators and account sizes further down, stays
as it is. The Rust test `the_account_sizes_the_frontend_hardcodes_still_hold`
fails if the account layout ever moves out from under those constants.

To run the site locally:

```bash
python3 -m http.server 4173
```

## Mainnet checklist

Not done, and all of it comes before any mainnet deploy:

- [ ] Independent security audit of the program.
- [ ] Upgrade authority moved to a multisig (Squads).
- [ ] Automatic resolution: a price oracle with a time-weighted average (e.g.
      30-minute TWAP) so a small cap cannot be pumped into a result. No single
      human resolver.
- [ ] A minimum liquidity and age filter on which coins can be bet on.
- [ ] Legal review. The owner is in the Netherlands, where offering real-money
      betting needs a Kansspelautoriteit licence — plus terms, an age gate and
      whatever geo-restrictions follow from that review.
