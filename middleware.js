// Vercel Edge Middleware: tells the page which country the visitor is in, and
// whether betting is closed to them.
//
// It deliberately does not block the site. Someone in a restricted country
// must still be able to reach a bet they already have, to cancel it or claim a
// refund after the grace window — their money is in escrow and nothing should
// stand between them and it. Only creating and taking are stopped, which the
// page does when it sees the flag below.
//
// Set BLOCKED_COUNTRIES in the Vercel project to change the list without a
// deploy; it defaults to NL.
//
// Worth being honest about what this is: a compliance gesture, not a security
// control. A VPN defeats it, and the program itself has no idea where anyone
// is. It stops casual access from a restricted country; it does not make the
// product unavailable there in any technical sense.

export const config = {
  // Everything except static assets, so the cookie is set on the page itself.
  matcher: ['/((?!_next|logos/|favicon).*)'],
};

const DEFAULT_BLOCKED = 'NL';

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
