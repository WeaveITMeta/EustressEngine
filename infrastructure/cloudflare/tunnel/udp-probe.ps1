<#
.SYNOPSIS
Measures which UDP datagram sizes reach the Eustress host and come back.

.DESCRIPTION
Pair with udp-echo.ps1 on the host. Sends tagged datagrams at each size and counts the
matching echoes, so a late echo from an earlier size is never counted as a success.

Exit code 0 means every 1200-byte datagram came back: QUIC's minimum datagram fits the
path, so the WebTransport host can run over it. Exit code 1 means it cannot.

.EXAMPLE
pwsh -File udp-probe.ps1 -Target 192.168.1.20
#>
param(
    [Parameter(Mandatory)][string]$Target,
    [int]$Port = 7779,
    # A comma-separated string rather than [int[]]: `pwsh -File` hands an array
    # argument over as one string, which [int[]] would fold into a single number.
    [string]$Sizes = '1200,1232,1252,1280,1350,1400,1472',
    [int]$Count = 20,
    [int]$TimeoutMs = 800
)

$sizeList = $Sizes -split ',' | ForEach-Object { [int]$_.Trim() }
$udp = [System.Net.Sockets.UdpClient]::new()
$udp.Client.ReceiveTimeout = $TimeoutMs
$udp.Connect($Target, $Port)

$tag = 0
$results = foreach ($size in $sizeList) {
    $echoed = 0
    for ($i = 0; $i -lt $Count; $i++) {
        $payload = [byte[]]::new($size)
        $tag++
        [System.BitConverter]::GetBytes([int]$tag).CopyTo($payload, 0)
        [void]$udp.Send($payload, $payload.Length)
        $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
        while ([DateTime]::UtcNow -lt $deadline) {
            try {
                $remote = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
                $echo = $udp.Receive([ref]$remote)
            }
            catch { break }
            if ($echo.Length -ge 4 -and [System.BitConverter]::ToInt32($echo, 0) -eq $tag) {
                if ($echo.Length -eq $size) { $echoed++ }
                break
            }
        }
    }
    [pscustomobject]@{ Bytes = $size; Sent = $Count; Echoed = $echoed }
}
$udp.Close()

$results | Format-Table -AutoSize | Out-String | Write-Host
$floor = $results | Where-Object Bytes -eq 1200
if ($floor -and $floor.Echoed -eq $floor.Sent) {
    Write-Host 'UDP_PROBE=OK (every 1200-byte datagram returned: QUIC fits this path)'
    exit 0
}
Write-Host 'UDP_PROBE=FAIL (1200-byte datagrams are lost: QUIC cannot run over this path)'
exit 1
