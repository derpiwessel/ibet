# ibet — i-bet.fun

Two people stake SOL against each other on whether a Solana coin's market cap
will **touch** a target before a deadline. An Anchor program holds both stakes
in escrow, and a keeper settles automatically the moment it happens.

**Touch = win.** For a Higher bet the creator wins as soon as the market cap
reaches the target at any moment in the window — the high of a 1-minute candle
— and is paid immediately, days early if that is when it happens. For a Lower
bet it is the candle's low. The taker wins only if the target is never touched
before the deadline, which can only be settled once that deadline passes.

> **Live on mainnet with real SOL, deliberately capped: 0.01–0.1 SOL a side and
> 10 SOL across the whole program at once.** The caps exist so that a bug, a bad
> candle or a stolen key costs a bounded amount. Raising them needs two of three
> multisig signers. What has *not* happened is an independent audit — see
> [what is still open](#what-is-still-open).

| | |
|---|---|
| Frontend | `index.html` — one file, no build step, deployed by Vercel |
| Program | `program/` — Anchor workspace, crate `ibet_escrow` |
| Mainnet | `FTB9qgtwzRjSU3Hq5KwZmQkRWWxg8TqxwS2hxPetLt7W`, config `5X5sz4Kz3BbWfTKAn4fdd2r7VdZH6WSYKu2VgmT1fHvv` |
| Devnet | `51Qu3DKZZ9bHJKiNyXhKcTp7PqJNW1YqVcx7Cqm6Vyv`, config `LYnPs2rV65RsjffKTEuoziqWy7jYXVPqem9npFpeN6s` |
| Keeper | `keeper` (devnet) and `keeper-mainnet`, both every minute via `pg_cron` |
| Backend | Supabase `ibet` (`bryjuhrovcgeurtlysmt`, eu-central-1) |

`main` ships `CLUSTER = 'mainnet'`; previews stay on devnet, so there is always
somewhere to test that cannot cost anyone money.

## The two deployments

| | mainnet | devnet |
|---|---|---|
| Program | `FTB9qgtwzRjSU3Hq5KwZmQkRWWxg8TqxwS2hxPetLt7W` | `51Qu3DKZZ9bHJKiNyXhKcTp7PqJNW1YqVcx7Cqm6Vyv` |
| Config | `5X5sz4Kz3BbWfTKAn4fdd2r7VdZH6WSYKu2VgmT1fHvv` | `LYnPs2rV65RsjffKTEuoziqWy7jYXVPqem9npFpeN6s` |
| Resolver | `FroYveCYYRbJZAhvT37iCAj1sBiyYg1nzBAjGxQFrFTE` | `FYjinyZZyVks7eQfexDffM9TDFJyb6VEyAPzEFiLDEwj` |
| Fee wallet | `EDiALz4fYTJcNaTcoAuuo6X5rWsvys199dnwgeWCyW5Q` | same |
| Admin | the Squads vault | the devnet deployer |
| Upgrade authority | the Squads vault | the devnet deployer |
| Stakes | **0.01 – 0.1 SOL** | 0.01 – 5 SOL |
| Exposure cap | **10 SOL** | none (phase-2 program) |
| Fee, grace, lengths | 200 bps · 48h · 5 min–30 days | same |

### Who can change what

Mainnet admin and upgrade authority are both the **Squads vault**
`Bk24511fFRiHnFtpdZnYHxnf48pqCHEqoWS6YR8o8Gzp`, which is vault index 0 of
multisig `6fNREmB2kiGnnr6YGRhtYCmDQkMPFRDURxbB4EN9D7Yg` — 2-of-3 over the
owner's Phantom, a backup wallet and one more trusted signer. Verify that
derivation yourself before sending anything there; sending authority to the
multisig address instead of its vault is the classic way to lose a program.

Nobody else can upgrade the program, raise the caps, change the fee or pause it.
The deploy wallet holds nothing and has no authority. The keeper can only settle,
and only to the destinations the bet already stores.

Raising a limit, pausing, or pointing the resolver at a new keeper address all go
through the multisig: build the `update_config` instruction, propose it in Squads,
and have a second signer approve. Every field is optional, so one proposal
changes only what it names.

The program refuses some changes outright whoever proposes them: a fee above 5%,
a `max_stake` below `min_stake`, a zero grace window, and an exposure cap that
cannot hold two whole stakes.

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
rounding. Point `dataRpcUrl` at an RPC that accepts browser traffic and the page
reads the supply itself instead.

On devnet the coins are still **mainnet** tokens even though the escrow is not,
so market data always comes from mainnet. On mainnet the two are the same chain.

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

**One file, deployed twice.** The slug it answers on decides everything that
differs: `keeper` is devnet, `keeper-mainnet` is mainnet, and each picks its own
program, escrow RPC and vault key from that. Two copies of a 700-line settlement
loop would have drifted apart, and this is the code that moves other people's
money. Both deployments report the same `ezbr_sha256`, which is how you check
they really are the same bytes:

```sql
select slug, version, ezbr_sha256 from edge_functions;  -- or the dashboard
```

Each run:

1. reads every matched bet off the chain and mirrors them into `chain_bets`;
2. picks the coin's pool on DexScreener — the deepest one that has actually
   traded recently, not simply the deepest, because a deep pool that has gone
   quiet produces no candles and a bet with no candles cannot be settled either
   way — and walks GeckoTerminal's 1-minute candles from where it last looked;
3. converts each candle's high (Higher) or low (Lower) to a market cap;
4. on the first candle inside `[taken_at, expires_at]` that reaches the target,
   sends `settle_bet(CreatorWins, that market cap, that candle's time)`;
5. once the deadline has passed **and** the candles reach it, sends
   `settle_bet(TakerWins, …)` — the source publishes a minute or two late,
   which is a large slice of a five-minute bet, so it waits rather than calling
   it early and records `waiting for candles to cover the whole window` while
   it does;
6. before paying anyone, checks the deciding candle against a second provider
   and refuses to settle if they disagree by more than 5% (see
   [the sanity check](#the-sanity-check));
7. writes the deciding candle, its pool, both providers' values, the deviation
   and the transaction signature into `settlements`.

Devnet and mainnet share one database, so `chain_bets`, `settlements` and
`alerts` each carry a `cluster` column and the site filters on it. Without that a
mainnet visitor would be shown devnet bets as though they were real.

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

Each cluster has its own secret: `ibet_keeper_key` for devnet,
`ibet_keeper_key_mainnet` for mainnet. `keeper_key_set` refuses to overwrite one
that already exists, so rotation is deliberate rather than accidental.

To rotate: delete the secret in the Supabase dashboard, call the function with
`?action=init-key` to mint a new one, fund the address it returns, and point the
resolver at it — on mainnet that last step is a multisig proposal. The old key
can do nothing the moment the config stops naming it.

```sql
select public.keeper_tick('init-key', 'mainnet');
```

The keeper needs a little SOL for transaction fees — it was funded with 0.05 SOL,
which is thousands of settlements. Top it up when it runs low, or settlements
stop silently apart from the alert and bets fall through to the 48h refund.

### The sanity check

GeckoTerminal's 1-minute candles decide every outcome, because they are the same
data the chart shows. Birdeye is asked about the same minute, ±1 minute for clock
skew, only at the moment a settlement is about to happen — once per run would be
43,200 calls a month and well past any free allowance.

Within 5% the keeper settles. Beyond it, it pays nobody, alerts, and retries
every minute; if the disagreement persists the 48h refund is the floor. If
Birdeye has no data at all it settles on GeckoTerminal alone and says so. Both
values and the deviation go into `settlements` either way, so every payout can be
re-checked afterwards.

Measured deviations on real settlements so far: 0.79%, 0.81%, 2.10%. The 5%
threshold sits well above that noise and well below a broken feed.

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
node scripts/check-clients.mjs
```

`--arch v0` is not optional: Anchor 1.2 defaults to SBPF v3, which the LiteSVM
version the tests run on cannot load. Building v0 keeps the artifact the tests
load byte-identical to the one that gets deployed.

The suite runs on [LiteSVM](https://github.com/LiteSVM/litesvm) rather than a
local validator, because most paths need the clock moved forward days at a time
and a validator's clock only advances at wall-clock speed. One dedicated payer
covers every transaction fee, so the parties' balances move only through escrow
and are asserted to the lamport.

62 tests cover: a touch paying the creator out early, in both directions and on
the exact target; evidence from outside the window being refused; a creator win
whose market cap does not reach the target being refused; a taker win before the
deadline being refused; a touch still settling after the deadline inside the
grace window; the fee arithmetic at 2% and 0%; the stake cap at exactly the limit
and one lamport over; the exposure cap at it, over it, and after a refund frees
room; that the counter always equals the sum of what is really escrowed; the
pause rules; and that no lamport is created or destroyed on any path.

`check-clients.mjs` is a different kind of check, and it exists because of a real
bug. Three clients hardcode the account list of every instruction — the site, the
keeper and the smoke test — and the compiler checks none of them. Phase 3 made
`Config` writable everywhere, and all three were left sending it read-only, which
broke every single instruction on mainnet while devnet carried on working because
it still ran the older program. The script compares each client against the IDL
Anchor generates from the Accounts structs. **Run it after any change to those
structs.**

### Deploy

The IDL names the mainnet program, so both scripts talk to mainnet by default and
devnet's older deployment is reached with `--program`:

```bash
node scripts/config.mjs show --cluster devnet \
  --program 51Qu3DKZZ9bHJKiNyXhKcTp7PqJNW1YqVcx7Cqm6Vyv
```

A fresh deployment, on either cluster:

```bash
cd program && anchor build --arch v0 && cargo test
solana program deploy target/deploy/ibet_escrow.so \
  --program-id target/deploy/ibet_escrow-keypair.json \
  --keypair <DEPLOY_WALLET> --url <CLUSTER>
```

Then confirm the chain got the bytes that were tested, before trusting it with
anything:

```bash
solana program dump <PROGRAM_ID> /tmp/onchain.so --url <CLUSTER>
head -c $(stat -f%z target/deploy/ibet_escrow.so) /tmp/onchain.so \
  | cmp - target/deploy/ibet_escrow.so && echo same bytes
```

Create the config. On mainnet the resolver is named here rather than changed
later, because the keeper's key exists before the program does:

```bash
node scripts/config.mjs init --cluster mainnet-beta --keypair <DEPLOY_WALLET> \
  --owner <OWNER_WALLET> --resolver <KEEPER> --min 0.01 --max 0.1 --exposure 10
node scripts/config.mjs show --cluster mainnet-beta
```

Defaults are fee 200 bps, stakes 0.01–5 SOL, grace 48h, exposure 10 SOL.
Override with `--fee-bps`, `--min`, `--max`, `--grace`, `--exposure`.

Then check it against the live program:

```bash
node scripts/smoke.mjs --cluster mainnet-beta --keypair <WALLET> --stake 0.01
```

All three paths, in order: create → cancel, create → take → touch, and
create → take → five-minute expiry. Every balance is asserted to the lamport,
including the fee wallet's share, and no bet is left open at the end — which
matters on mainnet, where a dangling matched bet would sit in escrow until the
48-hour refund. It needs about `stake * 3 + 0.05` SOL, nearly all of which comes
back. `--only 2` runs one path, which is how to resume after a dropped
connection.

Config changes on devnet, where the deployer is still admin:

```bash
node scripts/config.mjs set --cluster devnet --program <DEVNET_ID> --paused true
node scripts/config.mjs set --cluster devnet --program <DEVNET_ID> --fee-bps 100
```

On mainnet the same instruction goes through the multisig.

### Hand control over

Both steps are one-way, and on mainnet they have already happened — admin and
upgrade authority are the Squads vault. For reference, or for a future
deployment:

```bash
node scripts/config.mjs set --cluster mainnet-beta --admin <VAULT>
solana program set-upgrade-authority <PROGRAM_ID> \
  --new-upgrade-authority <VAULT> \
  --skip-new-upgrade-authority-signer-check --url mainnet-beta
```

Do it after the deployment has been smoke-tested, not before: a program fix is
cheap while the deploy wallet still holds authority and a multisig procedure
afterwards. Check the result with `solana program show <PROGRAM_ID>` and
`config.mjs show`, and confirm the deploy wallet is down to dust.

### Keypairs

Nothing secret is in the repo, and `.gitignore` keeps it that way.

| file | what it is |
|---|---|
| `~/.config/solana/ibet-escrow-mainnet-program-keypair.json` | **the mainnet program's identity.** Back this up offline. Losing it means the program can never be redeployed at `FTB9qgtwz…` again. It lives outside `target/`, which `cargo clean` would wipe. |
| `~/.config/solana/ibet-mainnet-deploy.json` | the mainnet deploy wallet. Holds nothing and has no authority since the handover; keep it anyway. |
| `~/.config/solana/ibet-escrow-v2-program-keypair.json` | the devnet program's identity |
| `~/.config/solana/ibet-devnet-deployer.json` | devnet admin and upgrade authority |
| Vault `ibet_keeper_key_mainnet` | the mainnet resolver. **No backup by design** — rotate instead. Nobody has ever seen the secret half, including whoever deployed it. |

`program/target/deploy/ibet_escrow-keypair.json` is a working copy of the mainnet
program keypair, so `anchor build` and `solana program deploy` find it where they
expect. The copy under `~/.config/solana` is the one that matters.

### Pointing the frontend somewhere else

One line at the top of the script in `index.html` picks a cluster, and one table
holds both:

```js
var CLUSTER = 'mainnet';          // 'devnet' on previews
var CHAINS = { devnet: { … }, mainnet: { … } };
```

Nothing below that block knows which cluster it is on. `rpcUrl` is where the
escrow lives; `dataRpcUrl` is where the coins live — on devnet they differ, on
mainnet they are the same endpoint. `CLUSTER` is also what the Supabase history
queries filter on, so the two chains never show each other's bets.

The mainnet `rpcUrl` is Helius with the key restricted to the `i-bet.fun`
origins. That restriction is the whole protection: the key sits in client-side
source, and what stops anyone else using it is that Helius refuses it from any
other origin. It therefore does **not** work from localhost or a preview URL,
which is deliberate — previews stay on devnet, which is where testing belongs.
It also refuses the CLI, which sends no origin at all, so command-line work
against mainnet goes through the public RPC.

The Rust test `the_account_sizes_the_frontend_hardcodes_still_hold` fails if the
account layout moves out from under the constants further down, and
`check-clients.mjs` fails if the account lists do.

To run the site locally:

```bash
python3 -m http.server 4173
```

## Deploying the keeper

The database objects are Supabase migrations (`settlements`, `chain_bets`, the
vault helpers, `keeper_tick`, and the `ibet-keeper` cron job). The function
itself is `keeper/index.ts`, deployed with `verify_jwt` off:

```bash
supabase functions deploy keeper         --no-verify-jwt
supabase functions deploy keeper-mainnet --no-verify-jwt
```

Deploy both from the same file, always, and check afterwards that the two report
the same `ezbr_sha256`. A difference there means one chain is running code the
other is not.

Run one by hand, or check what the last run did:

```sql
select public.keeper_tick(null, 'mainnet');         -- fire one run
select status_code, content::text, created
  from net._http_response order by id desc limit 1; -- read the result
select jobid, jobname, schedule, command, active from cron.job;
```

### Alerts

Every settlement, every blocked settlement, a second source that is down or out
of budget, and an escrow above 80% of its cap all post to a Discord webhook kept
in the vault as `alert_webhook_url`. Each line is prefixed with the cluster, so
devnet noise is obvious. Everything is also written to `alerts` with a
`delivered` flag, so a webhook that was broken or not yet configured loses
nothing.

```sql
select created_at, cluster, level, kind, message, delivered
  from alerts order by created_at desc limit 20;
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
- **One deciding price source.** GeckoTerminal's candles decide every outcome;
  Birdeye only vetoes data that looks broken. Both agreeing on a wrong number
  still settles wrongly, and DexScreener's pool list is a third dependency on
  top. What bounds the damage is the stake and exposure caps, not the data.
- **A quiet pool cannot be settled.** Resolution needs candles, and a pool with
  no trades produces none. The keeper now prefers pools that have actually
  traded, but a coin that stops trading entirely leaves its bet to the 48-hour
  refund. That is the honest outcome, and it is not a failure mode anyone loses
  money to.

## What is still open

Done for the mainnet launch:

- [x] Stake cap 0.01–0.1 SOL, exposure cap 10 SOL, both enforced on chain with
      checked math and invariant tests.
- [x] Upgrade authority and config admin on a 2-of-3 Squads multisig; the
      deploy wallet holds nothing.
- [x] A second price source with a 5% tolerance that blocks rather than decides,
      and both values recorded on every settlement.
- [x] Alerts to Discord, and a smoke test that asserts every lamport on all
      three paths against the live program.

Still open, and the caps are what stand in for them:

- [ ] **Independent security audit of the program and the keeper.** This has not
      happened. The 10 SOL ceiling is a bound on what an undiscovered bug can
      cost, not a substitute for the audit.
- [ ] **Terms of service, of any kind.** There are none. The placeholder page
      and the 18+ confirmation were both removed at the owner's instruction, so
      nothing on the site states how a bet is judged, what the fee is, or that
      settlement is final — and nobody is asked to agree to anything before
      staking real SOL.
- [ ] **Legal review, and it is now the sharpest of these.** The owner is in
      the Netherlands, where offering real-money betting needs a
      Kansspelautoriteit licence. ibet is open to every country, including NL —
      the owner's decision, taken knowing that. The block was never a control
      anyway (a VPN defeats it, and the program has no idea where anyone is),
      but it is now not even a gesture. Turning it back on is one Vercel
      environment variable: set `BLOCKED_COUNTRIES` to `NL`, or any
      comma-separated list, and it applies on the next request with no deploy.
- [ ] **A minimum liquidity and age filter on coins.** Any token can be bet on
      and nothing screens them. On a thin coin a single trade can push the
      market cap through a target for one minute and fall straight back, and
      that wins the bet.
- [ ] **Whether a one-minute touch is the right rule for real money.** It
      rewards exactly the volatility that is easiest to manufacture. A sustained
      window would be fairer and is a different product; this is the owner's
      call, and the site says plainly which rule it uses.
