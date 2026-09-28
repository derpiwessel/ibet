# ibet — i-bet.fun

Two people stake SOL against each other on whether a Solana coin's market cap
will **touch** a target before a deadline. An Anchor program holds both stakes
in escrow, and a keeper settles automatically the moment it happens.

**Touch = win.** For a Higher bet the creator wins as soon as the market cap
reaches the target at any moment in the window — the high of a 1-minute candle
— and is paid immediately, days early if that is when it happens. For a Lower
bet it is the candle's low. The taker wins only if the target is never touched
before the deadline, which can only be settled once that deadline passes.

> **Devnet only.** The deployed program runs on Solana devnet with test SOL that
> has no value. Do not deploy this to mainnet before working through the
> [mainnet checklist](#mainnet-checklist).

| | |
|---|---|
| Frontend | `index.html` — one file, no build step, deployed by Vercel |
| Program | `program/` — Anchor workspace, crate `ibet_escrow` |
| Program ID | `51Qu3DKZZ9bHJKiNyXhKcTp7PqJNW1YqVcx7Cqm6Vyv` (devnet) |
| Config PDA | `LYnPs2rV65RsjffKTEuoziqWy7jYXVPqem9npFpeN6s` |
| Keeper | `keeper` Edge Function, every minute via `pg_cron` |
| Backend | Supabase `ibet` (`bryjuhrovcgeurtlysmt`, eu-central-1) |

## Current devnet deployment

| | |
|---|---|
| Program | `51Qu3DKZZ9bHJKiNyXhKcTp7PqJNW1YqVcx7Cqm6Vyv` |
| Resolver | `FYjinyZZyVks7eQfexDffM9TDFJyb6VEyAPzEFiLDEwj` (the keeper) |
| Fee wallet | `EDiALz4fYTJcNaTcoAuuo6X5rWsvys199dnwgeWCyW5Q` (the owner) |
| Admin, upgrade authority | `GXckV6zEZwjMWgXdxd6bbEdBKARSGKKpWfp9GB4YcUNZ` (the deployer, **temporary**) |
| Fee | 200 bps · stakes 0.01–5 SOL · grace 48h · lengths 5 min–30 days |

Admin rights and the upgrade authority still sit with the deployer so the
program can be patched while it is being tested. Move both to the owner's
wallet with the two commands under
[Hand control to the owner's wallet](#hand-control-to-the-owners-wallet).

### Phase 2 runs on a new program address

The `Bet` account grew from 213 to 221 bytes, which old accounts cannot be read
as. Rather than stranding bets that were still matched, phase 2 was deployed to
a **new** program id and the phase-1 program was left running. Anything still
open there — including the JEANPHIL bet the owner took — can still be settled by
the owner's wallet on the old program, or refunded 48h after its deadline.

| phase 1 (still live) | |
|---|---|
| Program | `GACUebjQB1zZWuQW3pkhQLzYJL9yMtU8dX4TsfRyugq1` |
| Config | `2TuYCqG6BoPJDX3pQYgPABBPNxBNVegaZMHbBeLN6eu8` |
| Resolver | the owner's wallet |

## Where the data lives

Bets are **not** stored in a database. They are accounts owned by the escrow
program, and the site reads them straight off the chain.

| | source |
|---|---|
| Open and live bets | `getProgramAccounts` on the program |
| Wallet balance | `getBalance` |
| Fee, stake limits, grace window, pause | the on-chain `Config` account |
| Live market caps | DexScreener |
| Charts | GeckoTerminal candles × supply, drawn with TradingView Lightweight Charts |
| Coin logo, ticker and name | Supabase `token_meta`, filled by the token-meta function |
| Settled history and leaderboard | Supabase `chain_bets` + `settlements`, written by the keeper |
| Sign-in, `admins`, `trending` | Supabase |

A settled bet's account is closed on chain — that is what returns its rent — so
the chain cannot tell you what happened afterwards. The keeper mirrors every bet
it sees into `chain_bets` and writes the deciding candle into `settlements`, so
history and the leaderboard survive devnet ledger pruning. Both tables are
world-readable and only the keeper writes to them.

`Bet` stores the coin's address but no ticker. A ticker typed for a coin that is
not on the trending list is remembered in that browser only; everyone else sees
the shortened address.

### Market cap is price × total supply

One definition, used on the page and in resolution, so what you see is what you
are judged on.

The keeper reads total supply from mainnet with `getTokenSupply` and multiplies
by the candle price. The browser cannot do that: every free mainnet RPC refuses
browser traffic (`403`), so it uses DexScreener's fully-diluted valuation, which
is the same quantity — price × total supply — computed by them. The two agree to
rounding. Point `CHAIN.dataRpcUrl` at an RPC that accepts browser traffic and
the page reads the supply itself instead.

Note the coins are **mainnet** tokens even though the escrow is on devnet, so
market data always comes from mainnet.

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
| `settle_bet` | resolver | `CreatorWins` any time on evidence of a touch, `TakerWins` only after the deadline; both inside the grace window |
| `refund_expired` | anyone | after the grace window: both stakes back, no fee |

Rules worth knowing:

- A bet runs for anything from **5 minutes to 30 days**. Five minutes is the
  floor because settlement reads 1-minute candles; less than that and a bet
  would be decided by one or two of them.
- Touching **exactly** the target counts, in both directions.
- The program cannot see price history, so it checks what it can. A
  `CreatorWins` settlement must carry a market cap that really reaches the
  target, observed between `taken_at` and `expires_at`; the account keeps that
  evidence and the `BetSettled` event carries it. `TakerWins` means "never
  touched", which cannot be proven on chain and cannot be known early, so it is
  refused before the deadline.
- Payout destinations are pinned to the creator and taker stored on the bet and
  the fee wallet stored on the config. The resolver cannot redirect a payout by
  passing different accounts.
- `paused` blocks `create_bet` and `take_bet` only. Cancel, settle and refund
  keep working, so a pause can never trap money in escrow.
- Every matched bet ends in Settled or Refunded, and every open bet can be
  cancelled, so no path leaves funds stuck.

## Coin identity

Every coin shows its real logo and ticker. The `token-meta` Edge Function
resolves them in order — DexScreener, then GeckoTerminal, then on chain — and
caches the result in `token_meta`, which the site reads and only the function
writes. Rows older than 24h are refreshed; misses are cached too, so a coin with
no metadata anywhere is not looked up again by every visitor all day.

The on-chain step handles both shapes: pump.fun issues **Token-2022** mints that
carry their metadata as a TLV extension inside the mint account, while classic
SPL mints have a Metaplex metadata PDA. Metadata URIs live on IPFS, where a
single gateway is regularly unreachable, so several are tried.

```bash
# resolve and cache
curl "$SUPABASE_URL/functions/v1/token-meta?ca=<mint>,<mint>"
# force one step, to check a fallback a working first step would hide
curl "$SUPABASE_URL/functions/v1/token-meta?ca=<mint>&only=chain"
```

Images render in an `<img>` with `referrerpolicy="no-referrer"` and
`loading="lazy"`, with the coloured badge behind them, so a logo that fails to
load simply uncovers it again.

## The keeper

`keeper/index.ts` is a Supabase Edge Function that `pg_cron` fires every minute.
Each run:

1. reads every matched bet off the chain and mirrors them into `chain_bets`;
2. finds the coin's deepest pool on DexScreener and walks GeckoTerminal's
   1-minute candles from where it last looked;
3. converts each candle's high (Higher) or low (Lower) to a market cap;
4. on the first candle inside `[taken_at, expires_at]` that reaches the target,
   sends `settle_bet(CreatorWins, that market cap, that candle's time)`;
5. once the deadline has passed **and** the candles reach it, sends
   `settle_bet(TakerWins, …)` — the source publishes a minute or two late,
   which is a large slice of a five-minute bet, so it waits rather than calling
   it early and records `waiting for candles to cover the whole window` while
   it does;
6. writes the deciding candle, its pool, its source and the transaction
   signature into `settlements`.

A seven-day window is more minute candles than one API page holds, so each run
only fetches what is new and records how far it got in `chain_bets.checked_through`.
A taker win needs that column to have reached the deadline, so a gap in the data
can never be mistaken for "never touched". `observed_at` is always the candle's
own time, not the time the run happened, so a keeper that was down still settles
correctly on the candle that mattered. If a source is unreachable the run
changes nothing and tries again a minute later; the 48h refund is the backstop.

### Its key never leaves Supabase

The keeper signs with its own keypair, which **the function generates itself**
and stores in Supabase Vault. Nobody pastes it anywhere and it is not in the
repo. `pg_cron` authenticates with a second secret that Postgres mints into the
vault and sends as `x-keeper-token`; the function checks it in constant time.
That is why `verify_jwt` is off — the caller is cron, not a signed-in user.

```bash
# what the keeper's address is, and whether it matches the configured resolver
node program/scripts/config.mjs show
```

To rotate the key: delete the `ibet_keeper_key` secret in the Supabase
dashboard, call the function once with `?action=init-key` to mint a new one,
fund the address it returns, and point the resolver at it with
`config.mjs set --resolver <NEW>`.

The keeper needs a little devnet SOL for transaction fees. Top it up when it
runs dry, or settlements silently stop and bets fall through to the 48h refund.

### Force resolve

The site shows a **Force resolve** button on a matched bet when the keeper has
been quiet for over 30 minutes after a visible touch or after the deadline. It
is only shown to whoever holds the resolver role, which is normally the keeper —
so in practice nobody sees it. To take over by hand, point the resolver at your
own wallet first:

```bash
node program/scripts/config.mjs set --resolver <YOUR_WALLET>
```

Everyone else always has the **Refund** button once the 48h grace has passed.

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
and are asserted to the lamport.

50 tests cover: a touch paying the creator out early, in both directions and on
the exact target; evidence from outside the window being refused; a creator win
whose market cap does not reach the target being refused; a taker win before the
deadline being refused; a touch still settling after the deadline inside the
grace window; the fee arithmetic at 2% and 0%; every phase-1 rejection; the
pause rules; and that no lamport is created or destroyed on any path.

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
solana program set-upgrade-authority 51Qu3DKZZ9bHJKiNyXhKcTp7PqJNW1YqVcx7Cqm6Vyv \
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
  cluster:    'devnet',
  rpcUrl:     'https://api.devnet.solana.com',
  programId:  '51Qu3DKZZ9bHJKiNyXhKcTp7PqJNW1YqVcx7Cqm6Vyv',
  dataRpcUrl: 'https://api.mainnet-beta.solana.com'
};
```

Nothing below that block knows which cluster it is on. Moving to mainnet means
changing these values and deploying the same program there — everything else,
including the Anchor discriminators and account sizes further down, stays as it
is. The Rust test `the_account_sizes_the_frontend_hardcodes_still_hold` fails if
the account layout ever moves out from under those constants.

`rpcUrl` is where the escrow lives; `dataRpcUrl` is where the coins live. On
devnet they differ, and on mainnet they become the same endpoint. The keeper has
the same pair at the top of `keeper/index.ts` and has to be redeployed with
them.

To run the site locally:

```bash
python3 -m http.server 4173
```

## Deploying the keeper

The database objects are Supabase migrations (`settlements`, `chain_bets`, the
vault helpers, `keeper_tick`, and the `ibet-keeper` cron job). The function
itself is `keeper/index.ts`, deployed with `verify_jwt` off:

```bash
supabase functions deploy keeper --no-verify-jwt
```

Run it by hand, or check what the last run did:

```sql
select public.keeper_tick();                        -- fire one run
select status_code, content::text, created
  from net._http_response order by id desc limit 1; -- read the result
select * from cron.job where jobname = 'ibet-keeper';
```

## What is still worth knowing

- **Touch resolution rewards volatility.** A single large buy that pushes a
  small cap through a target for one minute wins the bet outright, even if the
  price falls straight back. That is the owner's chosen rule, and the site says
  so, but it makes targets near the current price very easy to hit.
- **The keeper is one key.** It cannot steal — payouts are pinned to the stored
  creator, taker and fee wallet — but it can settle wrongly, and a `TakerWins`
  is "never touched", which nothing on chain can verify. The `settlements`
  table is what makes that auditable after the fact.
- **One price source.** GeckoTerminal's candles and DexScreener's pool choice
  are both single points of failure. A second source to cross-check would be
  the next thing to add.

## Mainnet checklist

Not done, and all of it comes before any mainnet deploy:

- [ ] Independent security audit of the program **and** the keeper.
- [ ] Upgrade authority moved to a multisig (Squads).
- [ ] A second, independent price source, and a rule for what to do when they
      disagree. Consider whether a one-minute touch is still the right rule
      when real money is at stake, or whether a sustained window is fairer.
- [ ] A minimum liquidity and age filter on which coins can be bet on.
- [ ] Legal review. The owner is in the Netherlands, where offering real-money
      betting needs a Kansspelautoriteit licence — plus terms, an age gate and
      whatever geo-restrictions follow from that review.
