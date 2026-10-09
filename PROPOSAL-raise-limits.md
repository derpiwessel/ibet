# Squads proposal — raise the stake and exposure caps

One `update_config` instruction. It changes two fields and leaves every other
one alone, because each field in that instruction is optional and this proposal
sets only these two.

| field | from | to |
|---|---|---|
| `max_stake` | 0.1 SOL | **2 SOL** |
| `max_open_exposure` | 10 SOL | **25 SOL** |
| `min_stake` | 0.01 SOL | unchanged |
| fee, grace, resolver, fee wallet, admin, paused | | unchanged |

## The instruction

```
program   FTB9qgtwzRjSU3Hq5KwZmQkRWWxg8TqxwS2hxPetLt7W

account 0  Bk24511fFRiHnFtpdZnYHxnf48pqCHEqoWS6YR8o8Gzp   signer ✔   writable ✘
account 1  5X5sz4Kz3BbWfTKAn4fdd2r7VdZH6WSYKu2VgmT1fHvv   signer ✘   writable ✔

data (hex)     1d9efcbf0a53db63000000000001009435770000000000000100ba1dd205000000
data (base58)  9oMohHGfuzKQ3iymz7FNJ7bunE61fE43KZzqqJ9b97dk3
data (base64)  HZ78vwpT22MAAAAAAAEAlDV3AAAAAAAAAQC6HdIFAAAA
```

Account 0 is the Squads vault itself — it is the config's admin, and Squads
signs for it when the transaction executes. Account 1 is the escrow's config.

## For Squads' "Import base58 encoded tx"

```
kbVeK8gaLRgC8n4hDFm86Vt7iDuNSmx3cSbLyjqZtkAc9eLPpxndwCC2Ew3xXx5Mv2msdmrUyC67Zm9MgYdEuiimzsZsyBU7K6nmAXASfCS8rKm7s94fpaxp3mqSXq96qypJAnx66EUt6deS42VRUHfJEgk5huX8vGMwe1PPkr9GZFpcqjDok67bX6HAtC7qC9CXUZDJt5JfzNiJj5uxwZms9hiaboAkxBEqsY9n7cwcDZhGaUGtWqVgFdcu3VKzvDZ9AQsFRjeKM53yWq7bVM1rNoyuD6iVBt5UKCurZDEk6vFaLsFVhYvYef1jfMnDu
```

236 bytes, unsigned, fee payer the vault. Decoded again from that exact string
it is one instruction to `FTB9qgtwz…` carrying the 33 bytes below, and
simulating the decoded version leaves `min_stake 0.01, max_stake 2, exposure
0/25, grace 172800s`.

Two things Squads will show that are expected, not mistakes:

- **The vault appears writable.** A fee payer is always writable in a Solana
  message, so serialising promotes it. The program never writes to it.
- **The blockhash is stale by the time you import.** Squads rebuilds the
  message around its own; it only reads the instruction out of this blob.

## Reading the data yourself

33 bytes, and worth checking rather than trusting:

```
1d9efcbf0a53db63   the update_config discriminator
00                 admin             None
00                 resolver          None
00                 fee_wallet        None
00                 fee_bps           None
00                 min_stake         None      <- stays 0.01 SOL
01 0094357700000000  max_stake       Some(2_000_000_000)   = 2 SOL
00                 grace_secs        None
00                 paused            None
01 00ba1dd205000000  max_open_exposure Some(25_000_000_000) = 25 SOL
```

Little-endian u64: `0094357700000000` reads back as `0x77359400` = 2,000,000,000.
`00ba1dd205000000` is `0x5d21dba00` = 25,000,000,000.

Five `00` bytes before `max_stake` and two between it and the exposure cap. If
any of those were `01` the proposal would be changing something it should not.

## Verified before signing

Simulated against the live program with the vault as signer: accepted, no
error, and the resulting config reads `max_stake 2 SOL, exposure cap 25 SOL`.
The program also enforces its own floor — an exposure cap must hold two whole
stakes, and 25 ≥ 4, so this passes.

## What it does not change

Nothing about who holds power. Admin stays the vault, the resolver stays the
keeper, the fee wallet stays the owner's Phantom. A later proposal can lower
the caps again the same way; nothing here is one-way.
