// Resolves a coin's logo, ticker and name, and caches it in `token_meta`.
//
//   GET /token-meta?ca=<mint>[,<mint>...]
//
// Lookup order, first hit wins:
//   1. DexScreener token API   — info.imageUrl, baseToken.symbol/name
//   2. GeckoTerminal token info — image_url, symbol, name
//   3. Metaplex metadata on chain — the account's uri, then `image` from that
//      JSON, with ipfs:// rewritten to a gateway
//
// Step 3 is why this runs server-side: the browser cannot reach a mainnet RPC
// (every free one refuses browser traffic), and the coins are mainnet tokens
// even though the escrow is on devnet.
//
// Anyone may call this. It only reads public token data and fills a public
// cache, so there is nothing to protect beyond refusing input that is not an
// address and capping how many are asked for at once.

import { createClient } from "npm:@supabase/supabase-js@2.57.4";
import { Connection, PublicKey } from "npm:@solana/web3.js@1.98.4";

const DATA_RPC = "https://api.mainnet-beta.solana.com";
const METADATA_PROGRAM = new PublicKey("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
const TOKEN_2022 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
/// TLV type of the Token-2022 token-metadata extension.
const METADATA_EXT = 19;
/// Tried in order; public gateways are unreliable one at a time.
const IPFS_GATEWAYS = [
  "https://ipfs.io/ipfs/",
  "https://cloudflare-ipfs.com/ipfs/",
  "https://dweb.link/ipfs/",
];
const FRESH_FOR_MS = 24 * 60 * 60 * 1000;
const MAX_BATCH = 20;
const BASE58 = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;

const mainnet = new Connection(DATA_RPC, "confirmed");
const db = createClient(
  Deno.env.get("SUPABASE_URL")!,
  Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!,
);

type Meta = {
  ca: string;
  symbol: string | null;
  name: string | null;
  image_url: string | null;
  source: string;
};

/// The IPFS content id behind a URL, if it is one.
function ipfsCid(url: string): string | null {
  const u = url.trim();
  if (u.startsWith("ipfs://")) return u.slice("ipfs://".length).replace(/^ipfs\//, "");
  const viaGateway = /\/ipfs\/([A-Za-z0-9]+)/.exec(u);
  if (viaGateway) return viaGateway[1];
  if (/^(Qm[1-9A-HJ-NP-Za-km-z]{44}|baf[a-z2-7]{20,})$/.test(u)) return u;
  return null;
}

/// Point ipfs:// and bare CIDs at a gateway, and refuse anything that is not
/// an https URL we are willing to put in an <img>.
function normaliseImage(url: string | null | undefined): string | null {
  if (!url || typeof url !== "string") return null;
  const u = url.trim();
  if (!u) return null;
  const cid = ipfsCid(u);
  const out = cid ? IPFS_GATEWAYS[0] + cid : u;
  if (!/^https:\/\//i.test(out)) return null;
  return out.slice(0, 500);
}

/// Fetches JSON, walking the gateways when the URL is on IPFS, because any one
/// of them is regularly unreachable.
async function fetchMetadataJson(url: string): Promise<any | null> {
  const cid = ipfsCid(url);
  const candidates = cid ? IPFS_GATEWAYS.map((g) => g + cid) : [url];
  for (const candidate of candidates) {
    try {
      const res = await fetch(candidate, {
        headers: { accept: "application/json" },
        signal: AbortSignal.timeout(7000),
      });
      if (res.ok) return await res.json();
    } catch { /* try the next gateway */ }
  }
  return null;
}

const clean = (s: unknown, max: number): string | null => {
  if (typeof s !== "string") return null;
  // Metaplex pads its strings with NULs to a fixed width.
  const t = s.replace(/\u0000/g, "").trim();
  return t ? t.slice(0, max) : null;
};

// ── step 1: DexScreener ──────────────────────────────────────────────────────
async function fromDexScreener(ca: string): Promise<Meta | null> {
  try {
    const res = await fetch(`https://api.dexscreener.com/latest/dex/tokens/${ca}`);
    if (!res.ok) return null;
    const json = await res.json();
    const pairs = (json.pairs ?? []).filter((p: any) => p.chainId === "solana");
    if (!pairs.length) return null;
    pairs.sort((a: any, b: any) => (b.liquidity?.usd ?? 0) - (a.liquidity?.usd ?? 0));
    // The coin can be either side of the pool.
    for (const p of pairs) {
      const side = p.baseToken?.address === ca ? p.baseToken
                 : p.quoteToken?.address === ca ? p.quoteToken
                 : null;
      if (!side) continue;
      const image = normaliseImage(p.info?.imageUrl);
      const symbol = clean(side.symbol, 16);
      const name = clean(side.name, 64);
      if (image || symbol) {
        return { ca, symbol, name, image_url: image, source: "dexscreener" };
      }
    }
    return null;
  } catch {
    return null;
  }
}

// ── step 2: GeckoTerminal ────────────────────────────────────────────────────
async function fromGeckoTerminal(ca: string): Promise<Meta | null> {
  try {
    const res = await fetch(
      `https://api.geckoterminal.com/api/v2/networks/solana/tokens/${ca}`,
      { headers: { accept: "application/json" } },
    );
    if (!res.ok) return null;
    const a = (await res.json())?.data?.attributes;
    if (!a) return null;
    const image = normaliseImage(a.image_url && a.image_url !== "missing.png" ? a.image_url : null);
    const symbol = clean(a.symbol, 16);
    const name = clean(a.name, 64);
    if (!image && !symbol) return null;
    return { ca, symbol, name, image_url: image, source: "geckoterminal" };
  } catch {
    return null;
  }
}

// ── step 3: Metaplex metadata on chain ───────────────────────────────────────

/// Borsh string: 4-byte length then that many bytes. Metaplex pads them with
/// nulls to a fixed capacity, which `clean` strips.
function readString(view: DataView, bytes: Uint8Array, offset: number): [string, number] {
  const len = view.getUint32(offset, true);
  const start = offset + 4;
  const raw = new TextDecoder().decode(bytes.subarray(start, start + len));
  return [raw, start + len];
}

/// Token-2022 mints carry their metadata inside the mint account as a TLV
/// extension, which is what pump.fun issues these days — there is no Metaplex
/// PDA to look up for them.
function fromToken2022(data: Uint8Array): { name: string; symbol: string; uri: string } | null {
  if (data.length <= 166) return null;
  const v = new DataView(data.buffer, data.byteOffset, data.byteLength);
  // 82 bytes of base mint, padded to 165, then one account-type byte.
  let o = 166;
  while (o + 4 <= data.length) {
    const type = v.getUint16(o, true);
    const len = v.getUint16(o + 2, true);
    const val = o + 4;
    if (type === METADATA_EXT) {
      let p = val + 32 + 32; // update authority, mint
      const str = () => {
        const l = v.getUint32(p, true);
        p += 4;
        const out = new TextDecoder().decode(data.subarray(p, p + l));
        p += l;
        return out;
      };
      return { name: str(), symbol: str(), uri: str() };
    }
    o = val + len;
  }
  return null;
}

async function fromChain(ca: string): Promise<Meta | null> {
  try {
    const mint = new PublicKey(ca);
    let found: { name: string; symbol: string; uri: string } | null = null;
    let origin = "";

    const mintInfo = await mainnet.getAccountInfo(mint);
    if (mintInfo && mintInfo.owner.toBase58() === TOKEN_2022) {
      found = fromToken2022(new Uint8Array(mintInfo.data));
      origin = "token2022";
    }

    if (!found) {
      const pda = PublicKey.findProgramAddressSync(
        [new TextEncoder().encode("metadata"), METADATA_PROGRAM.toBytes(), mint.toBytes()],
        METADATA_PROGRAM,
      )[0];
      const info = await mainnet.getAccountInfo(pda);
      if (info) {
        const bytes = new Uint8Array(info.data);
        const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
        // key(1) + update_authority(32) + mint(32), then name, symbol, uri.
        let o = 1 + 32 + 32;
        const [rawName, afterName] = readString(view, bytes, o);
        o = afterName;
        const [rawSymbol, afterSymbol] = readString(view, bytes, o);
        o = afterSymbol;
        const [rawUri] = readString(view, bytes, o);
        found = { name: rawName, symbol: rawSymbol, uri: rawUri };
        origin = "metaplex";
      }
    }
    if (!found) return null;

    const symbol = clean(found.symbol, 16);
    const name = clean(found.name, 64);
    const uri = clean(found.uri, 400);

    let image: string | null = null;
    if (uri) {
      const json = await fetchMetadataJson(uri);
      image = normaliseImage(json?.image);
    }
    if (!image && !symbol) return null;
    return { ca, symbol, name, image_url: image, source: image ? origin : origin + ":no-image" };
  } catch {
    return null;
  }
}

async function resolve(ca: string, only?: string | null): Promise<Meta> {
  // `only` forces one step, for checking the fallbacks that a working step 1
  // would otherwise hide.
  const found = only === "dexscreener"   ? await fromDexScreener(ca)
              : only === "geckoterminal" ? await fromGeckoTerminal(ca)
              : only === "chain"         ? await fromChain(ca)
              : (await fromDexScreener(ca)
                ?? await fromGeckoTerminal(ca)
                ?? await fromChain(ca));
  // A miss is cached too, so a coin with no metadata anywhere is not looked up
  // again by every visitor for the next day.
  return found ?? { ca, symbol: null, name: null, image_url: null, source: "none" };
}

Deno.serve(async (req: Request) => {
  const json = (body: unknown, status = 200) =>
    new Response(JSON.stringify(body), {
      status,
      headers: {
        "content-type": "application/json",
        "access-control-allow-origin": "*",
        "access-control-allow-headers": "*",
        "cache-control": "public, max-age=300",
      },
    });
  if (req.method === "OPTIONS") return json({}, 204);

  try {
    const params = new URL(req.url).searchParams;
    const only = params.get("only");
    const raw = params.get("ca") ?? "";
    const wanted = [...new Set(raw.split(",").map((s) => s.trim()).filter((s) => BASE58.test(s)))]
      .slice(0, MAX_BATCH);
    if (!wanted.length) return json({ error: "pass ?ca=<mint address>" }, 400);

    const { data: rows } = await db.from("token_meta").select("*").in("ca", wanted);
    const have = new Map((rows ?? []).map((r: any) => [r.ca, r]));

    const out: Record<string, unknown> = {};
    const stale: string[] = [];
    for (const ca of wanted) {
      const row = have.get(ca);
      const fresh = !only && row &&
        Date.now() - new Date(row.updated_at).getTime() < FRESH_FOR_MS;
      if (fresh) out[ca] = row;
      else stale.push(ca);
    }

    if (stale.length) {
      const resolved = await Promise.all(stale.map((ca) => resolve(ca, only)));
      const now = new Date().toISOString();
      const upserts = resolved.map((m) => ({ ...m, updated_at: now }));
      // A forced single-step lookup is for diagnosis, so it must not poison
      // the cache everyone else reads.
      if (!only) await db.from("token_meta").upsert(upserts, { onConflict: "ca" });
      for (const m of upserts) out[m.ca] = m;
    }

    return json({ tokens: out });
  } catch (e) {
    return json({ error: String(e) }, 500);
  }
});
