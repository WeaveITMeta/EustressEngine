# Live play over UDP through Cloudflare Tunnel

The workstation hosts a Space (Studio, F9) on UDP 7777. Remote players reach it through Cloudflare
Tunnel, with no router port forwarding and no public exposure of the home address. The goal and its
milestones are in `docs/launch/GOAL_LIVE_GALLERY_PLAY.md`.

**Why this shape.** A tunnel's public hostnames carry HTTP, HTTPS, WebSocket, and TCP, not UDP. Its
private network routes carry UDP to devices running the Cloudflare One (WARP) client enrolled in the
Zero Trust organization. So Stage 1 uses a private route, and Stage 2 puts Spectrum private origins
(public UDP to a private origin through the same tunnel) in front of it once the account has access.
The engine's WebTransport host is used unchanged in both.

---

## Stage 1: UDP to invited players

Every step that touches the Cloudflare account can be done by Meta Muse from the vault.

1. **Install `cloudflared` on the workstation.**
   ```
   winget install --id Cloudflare.cloudflared
   ```
2. **Zero Trust organization.** In the Cloudflare dashboard, open Zero Trust and choose a team name
   (for example `eustress`). The free plan covers up to 50 users.
3. **Device enrollment rule.** Settings, WARP Client, Device enrollment permissions: allow the email
   addresses of the invited players, with one-time PIN login.
4. **Create the tunnel.** Networks, Tunnels, Create a tunnel, Cloudflared, name `eustress-play`. Run
   the install command the dashboard shows, which registers `cloudflared` as a Windows service.
   Keep outbound UDP 7844 open: UDP rides only when `cloudflared` reaches Cloudflare over QUIC, and
   the HTTP/2 fallback carries none.
5. **Private network route.** In the tunnel, add a private network route for the workstation's LAN
   address as a /32 (for example `192.168.1.20/32`).
6. **Split tunnels.** The client excludes private address ranges by default, which would send
   traffic for `192.168.x.x` around the tunnel. Settings, WARP Client, Device settings, Split
   Tunnels: remove the range holding the workstation's address from the exclude list, or switch to
   include mode with only that /32.
7. **Windows Firewall on the workstation.** Allow inbound UDP 7777 (the host) and 7779 (the probe).
8. **The second player.** Install the Cloudflare One client, enroll it with the team name, and
   connect.

### Probe before playing

UDP through the tunnel is capped near 1,280 bytes, and QUIC needs 1,200-byte datagrams. Measure it
before debugging the engine:

```
# On the workstation
pwsh -File infrastructure/cloudflare/tunnel/udp-echo.ps1

# On the second player's machine, with the Cloudflare One client connected
pwsh -File udp-probe.ps1 -Target <workstation LAN address>
```

`UDP_PROBE=OK` means QUIC fits. The table shows the largest size that survives; the engine's QUIC
stack must stay at or below it, and quinn probes upward from 1,200 on its own, discarding sizes that
are lost.

A raw pass does not prove the browser fits, because a web page cannot set Chrome's QUIC packet size.
Once a host runs (milestone M0), the second check is a real browser WebTransport handshake through the
tunnel, with `webtransport-probe.html`. Open DevTools first, paste the join link Studio shows (or pass
`?link=<urlencoded>`), and click Run. One run reports the time to open (Chrome's own QUIC packets
through the tunnel, the pin, and Private or Local Network Access), the protocol round trip on the
reliable stream, and, once the host serves `/probe/<key>`, datagram round trips, loss, and the largest
datagram that returns. It ends with a JSON report whose key and pin are shortened, so it is safe to
paste.

It tests Private or Local Network Access only when served from a public HTTPS origin with the host on a
private address, which is Stage 1's shape. Opened from a file or from localhost it tests everything
else; the `tunnel-probe` entry in `.claude/launch.json` serves it on `127.0.0.1:3190`. Chrome logs the
failing `net::` line to the Console where the page cannot read it, so the page maps that line to its
cause for you.

### Host

Set `EUSTRESS_HOST_LAN=1` so the host binds the LAN address, open the Space in Studio, and press F9,
which enters Play itself. The join link Studio shows carries the LAN address, the key, and the
certificate pin.

---

## Stage 2: public UDP

Spectrum private origins, announced 2026-06-10, route public TCP and UDP to an origin on a private
address through Cloudflare Tunnel. It is a closed beta for eligible Enterprise customers, with
general availability targeted for Q4 2026; access is requested through a Cloudflare account team.
Once granted: a Spectrum application for UDP 7777 on `play.eustress.dev`, whose origin is the
workstation's private address with this tunnel's virtual network. Players then need no client, and
the engine does not change.

If access is refused, the public alternative is Cloudflare Realtime TURN, which relays WebRTC over
UDP and would move the transport from WebTransport to WebRTC data channels.

---

## Files

| File | Purpose |
|---|---|
| `udp-echo.ps1` | Echo server for the probe, run on the workstation |
| `udp-probe.ps1` | Measures which datagram sizes survive the path; exit 0 when QUIC fits |
| `webtransport-probe.html` | A real browser WebTransport handshake through the tunnel, with a paste-safe JSON report; owned by the Eustress WASM session |
