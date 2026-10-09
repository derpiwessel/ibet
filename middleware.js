// Vercel Edge Middleware: tells the page which country the visitor is in, and
// whether betting is closed to them.
//
// It deliberately does not block the site. Someone in a restricted country
// must still be able to reach a bet they already have, to cancel it or claim a
// refund after the grace window — their money is in escrow and nothing should
// stand between them and it. Only creating and taking are stopped, which the
// page does when it sees the flag below.
//
// The list is empty by default: the owner's decision is that ibet is open to
// everyone, including the Netherlands. The machinery stays because turning it
// back on should not need a code change — set BLOCKED_COUNTRIES in the Vercel
// project (e.g. "NL" or "NL,US") and it takes effect on the next request.
//
// Worth being honest about what this is when it is switched on: a compliance
// gesture, not a security control. A VPN defeats it, and the program itself has
// no idea where anyone is. It stops casual access from a restricted country; it
// does not make the product unavailable there in any technical sense.
//
// The country is still reported either way, because the terms acceptance record
// stores where someone was when they accepted.

export const config = {
  // Everything except static assets, so the cookie is set on the page itself.
  matcher: ['/((?!_next|logos/|favicon).*)'],
};

const DEFAULT_BLOCKED = '';

function blockedList() {
  const raw = process.env.BLOCKED_COUNTRIES ?? DEFAULT_BLOCKED;
  return raw
    .split(',')
    .map((s) => s.trim().toUpperCase())
    .filter(Boolean);
}

export default function middleware(request) {
  const country = (request.headers.get('x-vercel-ip-country') || '').toUpperCase();
  const blocked = country && blockedList().includes(country);

  const response = new Response(null, {
    status: 200,
    headers: { 'x-middleware-next': '1' },
  });

  // Readable by the page, so it can show the notice and disable betting.
  // Not a secret, and not a security boundary — see the note above.
  const attrs = 'Path=/; Max-Age=3600; SameSite=Lax';
  response.headers.append('Set-Cookie', `ibet_country=${country || 'XX'}; ${attrs}`);
  response.headers.append('Set-Cookie', `ibet_blocked=${blocked ? '1' : '0'}; ${attrs}`);
  return response;
}
