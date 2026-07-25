[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("discover-resolver", "require-reachable", "require-denied")]
    [string]$Mode,

    [string]$ResolverAddress
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$dnsTimeoutMilliseconds = 5000
$tcpTimeoutMilliseconds = 5000
$httpsTimeoutSeconds = 10
$directTcpAddress = "1.1.1.1"
$directTcpPort = 443
$httpsUri = "https://example.com/"

function Assert-Ipv4Address {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value
    )

    $parsed = $null
    if (-not [System.Net.IPAddress]::TryParse($Value, [ref]$parsed) -or
        $parsed.AddressFamily -ne [System.Net.Sockets.AddressFamily]::InterNetwork -or
        $parsed.Equals([System.Net.IPAddress]::Any) -or
        $parsed.Equals([System.Net.IPAddress]::None) -or
        $parsed.Equals([System.Net.IPAddress]::Broadcast)) {
        throw "ResolverAddress must be one usable IPv4 address."
    }
    return $parsed
}

function Test-DnsQuery {
    param(
        [Parameter(Mandatory = $true)]
        [System.Net.IPAddress]$Resolver
    )

    [byte[]]$query = @(
        0x50, 0x41, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x07, [byte][char]'e', [byte][char]'x',
        [byte][char]'a', [byte][char]'m', [byte][char]'p', [byte][char]'l',
        [byte][char]'e', 0x03, [byte][char]'c', [byte][char]'o', [byte][char]'m',
        0x00, 0x00, 0x01, 0x00, 0x01
    )
    $socket = [System.Net.Sockets.Socket]::new(
        [System.Net.Sockets.AddressFamily]::InterNetwork,
        [System.Net.Sockets.SocketType]::Dgram,
        [System.Net.Sockets.ProtocolType]::Udp
    )
    try {
        $socket.SendTimeout = $dnsTimeoutMilliseconds
        $socket.ReceiveTimeout = $dnsTimeoutMilliseconds
        $socket.Connect([System.Net.IPEndPoint]::new($Resolver, 53))
        if ($socket.Send($query) -ne $query.Length) {
            return $false
        }
        $response = [byte[]]::new(512)
        $received = $socket.Receive($response)
        return $received -ge 12 -and $response[0] -eq 0x50 -and $response[1] -eq 0x41
    }
    catch [System.Net.Sockets.SocketException] {
        return $false
    }
    finally {
        $socket.Dispose()
    }
}

function Test-DirectTcpConnection {
    $client = [System.Net.Sockets.TcpClient]::new(
        [System.Net.Sockets.AddressFamily]::InterNetwork
    )
    $connection = $null
    try {
        $connection = $client.BeginConnect($directTcpAddress, $directTcpPort, $null, $null)
        if (-not $connection.AsyncWaitHandle.WaitOne($tcpTimeoutMilliseconds)) {
            return $false
        }
        $client.EndConnect($connection)
        return $client.Connected
    }
    catch [System.Net.Sockets.SocketException] {
        return $false
    }
    finally {
        if ($null -ne $connection) {
            $connection.AsyncWaitHandle.Dispose()
        }
        $client.Dispose()
    }
}

function Test-HttpsRequest {
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.AllowAutoRedirect = $true
    $handler.MaxAutomaticRedirections = 3
    $handler.UseProxy = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [System.TimeSpan]::FromSeconds($httpsTimeoutSeconds)
    try {
        $response = $client.GetAsync($httpsUri).GetAwaiter().GetResult()
        try {
            return [int]$response.StatusCode -ge 200 -and [int]$response.StatusCode -lt 500
        }
        finally {
            $response.Dispose()
        }
    }
    catch [System.Net.Http.HttpRequestException] {
        return $false
    }
    catch [System.Threading.Tasks.TaskCanceledException] {
        return $false
    }
    finally {
        $client.Dispose()
    }
}

function Find-ReachableResolver {
    $candidates = @(
        foreach ($networkInterface in
        [System.Net.NetworkInformation.NetworkInterface]::GetAllNetworkInterfaces()) {
        if ($networkInterface.OperationalStatus -ne
            [System.Net.NetworkInformation.OperationalStatus]::Up -or
            $networkInterface.NetworkInterfaceType -eq
            [System.Net.NetworkInformation.NetworkInterfaceType]::Loopback) {
            continue
        }
        foreach ($address in $networkInterface.GetIPProperties().DnsAddresses) {
            if ($address.AddressFamily -eq [System.Net.Sockets.AddressFamily]::InterNetwork) {
                $address.ToString()
            }
        }
        }
        # Hosted Linux may expose its resolver only through resolv.conf.
        if (Test-Path -LiteralPath "/etc/resolv.conf" -PathType Leaf) {
            foreach ($line in Get-Content -LiteralPath "/etc/resolv.conf") {
                if ($line -match '^\s*nameserver\s+([0-9]+(?:\.[0-9]+){3})(?:\s|$)') {
                    $Matches[1]
                }
            }
        }
    )
    foreach ($candidate in @($candidates | Sort-Object -Unique)) {
        try {
            $parsed = Assert-Ipv4Address -Value $candidate
        }
        catch {
            continue
        }
        if (Test-DnsQuery -Resolver $parsed) {
            return $candidate
        }
    }
    throw "No configured IPv4 DNS resolver returned the bounded baseline query."
}

if ($Mode -eq "discover-resolver") {
    [pscustomobject]@{
        resolver_address = Find-ReachableResolver
        dns_reachable = $true
    } | ConvertTo-Json -Compress
    exit 0
}

if ([string]::IsNullOrWhiteSpace($ResolverAddress)) {
    throw "ResolverAddress is required outside discover-resolver mode."
}
$resolver = Assert-Ipv4Address -Value $ResolverAddress
$dnsReachable = Test-DnsQuery -Resolver $resolver
$tcpReachable = Test-DirectTcpConnection
$httpsReachable = Test-HttpsRequest
$expectedReachable = $Mode -eq "require-reachable"
$boundarySatisfied = if ($expectedReachable) {
    # Resolver discovery already proved DNS; do not make that transient UDP probe a duplicate gate.
    $tcpReachable -and $httpsReachable
}
else {
    -not $dnsReachable -and -not $tcpReachable -and -not $httpsReachable
}
if (-not $boundarySatisfied) {
    throw "Network boundary did not satisfy $Mode (dns=$dnsReachable tcp=$tcpReachable https=$httpsReachable)."
}

$observed = "denied"
if ($expectedReachable) {
    $observed = "reachable"
}
[pscustomobject]@{
    expected = $observed
    resolver_address = $resolver.ToString()
    dns_reachable = $dnsReachable
    direct_tcp_reachable = $tcpReachable
    https_reachable = $httpsReachable
} | ConvertTo-Json -Compress
