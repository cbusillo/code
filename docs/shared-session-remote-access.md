# Shared Session Remote Access

The browser companion is intentionally not designed to talk to a raw public
`code-app-server` socket.

It is also intentionally single-user. If another person can reach the same
companion endpoint, tunnel, VPN, or authenticated proxy, they can act as the
same operator on your shared session rather than as a separate scoped user.

The browser now reflects that boundary directly: non-loopback targets require
an explicit acknowledgment before the companion will connect.

That reminder also stays visible after connect so a trusted-LAN or
authenticated-`wss://` session does not silently look like a local-only
loopback connection once the settings panel collapses.

## Loopback smoke check

Use this to validate the supported local browser path against a real
`code-app-server`.

1. Start the app-server on loopback:

```sh
cargo run -p code-app-server -- --listen ws://127.0.0.1:8877
```

1. In another terminal, run the shared-session smoke script:

```sh
node scripts/smoke-shared-session-loopback.mjs ws://127.0.0.1:8877
```

The smoke checks the same browser-facing contract used by the companion:

- `initialize`
- `thread/list`
- `thread/start`
- `thread/loaded/list`

It exits non-zero if the local loopback deployment is not behaving like the
browser companion expects.

The supported deployment patterns are:

## 1. Local-only default

Use this when the browser is on the same machine as the app-server.

- Run `code-app-server` on `127.0.0.1:8877`.
- Connect the browser companion to `ws://127.0.0.1:8877`.
- Do not bind the app-server directly to a public interface just to make the
  browser work remotely.

## 2. Remote access over SSH tunnel

Use this when you are away from the machine and can SSH to it directly.

- Keep `code-app-server` bound to `127.0.0.1:8877` on the remote machine.
- Forward the socket to your local device:

```sh
ssh -L 8877:127.0.0.1:8877 user@remote-host
```

- Point the browser companion at `ws://127.0.0.1:8877` after the tunnel is up.

This is the safest general-purpose remote pattern because the app-server still
never leaves loopback.

## 3. Trusted LAN or VPN only

Use plain `ws://` on a private address only when the network itself is already
the trust boundary, such as a home LAN or a tailnet you control.

- Accept that transport is still unencrypted at the WebSocket layer.
- Treat this as a convenience mode, not the long-term internet-facing story.
- For broader reach, move to the authenticated `wss://` proxy model below.

## 4. Authenticated `wss://` proxy for mobile/internet access

Use this when the browser companion needs real remote or mobile access.

### Required shape

1. Keep `code-app-server` on `127.0.0.1:8877`.
2. Put TLS termination and WebSocket upgrade handling in front of it.
3. Enforce access control outside the app-server itself.
4. Connect the browser companion to a `wss://` URL exposed by that boundary.

Important: the raw app-server does not authenticate browser clients. A plain
`wss://` proxy is not enough unless you also add a real outer trust boundary.

Important: that outer boundary should normally be personal. If multiple people
can clear it, they are effectively sharing one companion identity and can steer
the same threads with the same authority.

Acceptable outer boundaries include:

- VPN-only reachability such as Tailscale, WireGuard, or an equivalent private
  network.
- Reverse-proxy auth such as forward-auth, SSO, identity-aware proxy, or a
  comparable gate.
- A tightly controlled bastion path where only authenticated users can reach
  the proxy.

### Example reverse-proxy upstream

The upstream target should still just be loopback on the machine running
`code-app-server`.

```txt
reverse_proxy 127.0.0.1:8877
```

That is only the upstream mapping. You still need TLS plus one of the access
control layers above.

### Example Caddy sketch

```caddyfile
companion.example.com {
    reverse_proxy 127.0.0.1:8877
}
```

### Example nginx sketch

```nginx
server {
    listen 443 ssl;
    server_name companion.example.com;

    location / {
        proxy_pass http://127.0.0.1:8877;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

Those snippets only show the WebSocket/TLS shape. Add your auth or VPN gate on
top of them before treating the endpoint as supported.

## Explicitly unsupported

- Public `ws://host:port` exposure.
- Public `wss://` exposure with no outer auth or network boundary.
- Treating the current companion as a true multi-user or per-user-isolated
  browser surface.
- Binding the app-server directly to a broad interface and assuming the browser
  should connect to it raw from the internet.

## Product intent

This keeps the browser companion companion-first:

- the desktop or primary machine remains the source of truth,
- the browser gets safe remote steering and review,
- and we avoid turning the app-server into an unauthenticated internet service.
