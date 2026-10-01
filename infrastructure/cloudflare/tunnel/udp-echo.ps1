<#
.SYNOPSIS
Echoes every UDP datagram back to its sender.

.DESCRIPTION
Run on the workstation that hosts Eustress, then run udp-probe.ps1 from a machine
with the Cloudflare One client connected. The probe reports which datagram sizes
survive Cloudflare Tunnel's private-network UDP path, which is what decides whether
QUIC (and so the engine's WebTransport host) can run over it: QUIC needs at least
1200-byte datagrams, and the tunnel caps UDP payloads near 1280.

Port 7779 sits beside the game's 7777 so the probe never competes with a live host.
#>
param(
    [int]$Port = 7779,
    [string]$Bind = '0.0.0.0'
)

$endpoint = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Parse($Bind), $Port)
$udp = [System.Net.Sockets.UdpClient]::new($endpoint)
Write-Host "udp-echo listening on ${Bind}:$Port (Ctrl+C to stop)"
try {
    while ($true) {
        $remote = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
        $bytes = $udp.Receive([ref]$remote)
        [void]$udp.Send($bytes, $bytes.Length, $remote)
        Write-Host ('{0:HH:mm:ss} {1} bytes from {2}' -f (Get-Date), $bytes.Length, $remote)
    }
}
finally {
    $udp.Close()
}
