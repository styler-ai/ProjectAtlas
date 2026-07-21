[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet(
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc"
    )]
    [string]$Target,

    [Parameter(Mandatory = $true)]
    [string]$SourceManifest,

    [Parameter(Mandatory = $true)]
    [string]$Destination
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$maxSourceManifestBytes = 1MB
$maxSidecarBytes = 4KB
$maxNativeBundleBytes = 64MB
$maxUpstreamManifestBytes = 1MB
$downloadBufferBytes = 64KB

function Get-RegularFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [long]$MaximumBytes
    )

    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "Input is not a regular file: $($item.FullName)"
    }
    if ($item.Length -le 0 -or $item.Length -gt $MaximumBytes) {
        throw "Input file length is outside its release bound: $($item.FullName)"
    }
    return $item
}

function Get-Sha256Hex {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $stream = [System.IO.File]::Open(
        $Path,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::Read
    )
    try {
        $hasher = [System.Security.Cryptography.SHA256]::Create()
        try {
            return [System.Convert]::ToHexString($hasher.ComputeHash($stream)).ToLowerInvariant()
        }
        finally {
            $hasher.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

function Assert-Sha256 {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,

        [Parameter(Mandatory = $true)]
        [string]$Owner
    )

    $normalized = $Value.ToLowerInvariant()
    if ($normalized -notmatch '\A[0-9a-f]{64}\z') {
        throw "$Owner does not contain one lowercase-compatible SHA-256 value."
    }
    return $normalized
}

function Assert-HttpsUri {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value,

        [Parameter(Mandatory = $true)]
        [string]$Owner
    )

    $uri = [System.Uri]::new($Value, [System.UriKind]::Absolute)
    if ($uri.Scheme -ne [System.Uri]::UriSchemeHttps -or -not [string]::IsNullOrEmpty($uri.UserInfo)) {
        throw "$Owner must be one credential-free HTTPS URI."
    }
    return $uri
}

function Receive-PinnedFile {
    param(
        [Parameter(Mandatory = $true)]
        [System.Net.Http.HttpClient]$Client,

        [Parameter(Mandatory = $true)]
        [System.Uri]$Uri,

        [Parameter(Mandatory = $true)]
        [string]$OutputPath,

        [Parameter(Mandatory = $true)]
        [long]$ExpectedBytes,

        [Parameter(Mandatory = $true)]
        [string]$ExpectedSha256,

        [Parameter(Mandatory = $true)]
        [long]$MaximumBytes
    )

    if ($ExpectedBytes -le 0 -or $ExpectedBytes -gt $MaximumBytes) {
        throw "Pinned input length is outside its release bound."
    }

    $response = $Client.GetAsync(
        $Uri,
        [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead
    ).GetAwaiter().GetResult()
    try {
        $response.EnsureSuccessStatusCode() | Out-Null
        $finalUri = $response.RequestMessage.RequestUri
        if ($null -eq $finalUri -or $finalUri.Scheme -ne [System.Uri]::UriSchemeHttps -or
            -not [string]::IsNullOrEmpty($finalUri.UserInfo)) {
            throw "Pinned input redirected outside credential-free HTTPS."
        }
        if ($null -ne $response.Content.Headers.ContentLength -and
            $response.Content.Headers.ContentLength -ne $ExpectedBytes) {
            throw "Pinned input Content-Length differs from the accepted source manifest."
        }

        $input = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $output = [System.IO.File]::Open(
            $OutputPath,
            [System.IO.FileMode]::CreateNew,
            [System.IO.FileAccess]::Write,
            [System.IO.FileShare]::None
        )
        $hasher = [System.Security.Cryptography.IncrementalHash]::CreateHash(
            [System.Security.Cryptography.HashAlgorithmName]::SHA256
        )
        try {
            $buffer = [byte[]]::new($downloadBufferBytes)
            [long]$total = 0
            while ($true) {
                $read = $input.ReadAsync($buffer, 0, $buffer.Length).GetAwaiter().GetResult()
                if ($read -eq 0) {
                    break
                }
                if ($total -gt ([long]::MaxValue - [long]$read)) {
                    throw "Pinned input byte count overflowed."
                }
                $total += [long]$read
                if ($total -gt $ExpectedBytes -or $total -gt $MaximumBytes) {
                    throw "Pinned input exceeded its accepted byte length."
                }
                $hasher.AppendData($buffer, 0, $read)
                $output.Write($buffer, 0, $read)
            }
            $output.Flush($true)
            $observedSha256 = [System.Convert]::ToHexString($hasher.GetHashAndReset()).ToLowerInvariant()
            if ($total -ne $ExpectedBytes -or $observedSha256 -ne $ExpectedSha256) {
                throw "Pinned input bytes or SHA-256 differ from the accepted source manifest."
            }
        }
        finally {
            $hasher.Dispose()
            $output.Dispose()
            $input.Dispose()
        }
    }
    finally {
        $response.Dispose()
    }
}

$sourceManifestPath = [System.IO.Path]::GetFullPath($SourceManifest)
$sourceManifestItem = Get-RegularFile -Path $sourceManifestPath -MaximumBytes $maxSourceManifestBytes
$sidecarPath = "$sourceManifestPath.sha256"
$sidecarItem = Get-RegularFile -Path $sidecarPath -MaximumBytes $maxSidecarBytes
$sidecarText = [System.IO.File]::ReadAllText($sidecarItem.FullName).Trim()
$sidecarMatch = [System.Text.RegularExpressions.Regex]::Match(
    $sidecarText,
    '\A(?<sha>[0-9A-Fa-f]{64})(?:\s+[^\r\n]+)?\z'
)
if (-not $sidecarMatch.Success) {
    throw "Source-manifest sidecar does not contain one canonical SHA-256 row."
}
$expectedManifestSha256 = Assert-Sha256 -Value $sidecarMatch.Groups["sha"].Value -Owner "source-manifest sidecar"
$observedManifestSha256 = Get-Sha256Hex -Path $sourceManifestItem.FullName
if ($observedManifestSha256 -ne $expectedManifestSha256) {
    throw "Source-manifest SHA-256 differs from its checked-in sidecar."
}

$sourceIntake = [System.IO.File]::ReadAllText($sourceManifestItem.FullName) | ConvertFrom-Json -Depth 64
$platformRows = @($sourceIntake.platforms | Where-Object { $_.platform -eq $Target })
if ($platformRows.Count -ne 1) {
    throw "Source manifest must contain exactly one row for target $Target."
}
$platformRow = $platformRows[0]
$bundleUri = Assert-HttpsUri -Value ([string]$platformRow.url) -Owner "native bundle"
$bundleSha256 = Assert-Sha256 -Value ([string]$platformRow.sha256) -Owner "native bundle"
$bundleBytes = [long]$platformRow.byte_length

$upstreamRow = $sourceIntake.upstream_release_manifest
if ($null -eq $upstreamRow) {
    throw "Source manifest does not contain an upstream parser-manifest pin."
}
$upstreamUri = Assert-HttpsUri -Value ([string]$upstreamRow.url) -Owner "upstream parser manifest"
$upstreamSha256 = Assert-Sha256 -Value ([string]$upstreamRow.sha256) -Owner "upstream parser manifest"
$upstreamBytes = [long]$upstreamRow.byte_length

$destinationPath = [System.IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $destinationPath) {
    throw "Destination already exists: $destinationPath"
}
$destinationParent = [System.IO.Path]::GetDirectoryName($destinationPath)
if ([string]::IsNullOrWhiteSpace($destinationParent)) {
    throw "Destination must have an explicit parent directory."
}
[System.IO.Directory]::CreateDirectory($destinationParent) | Out-Null
$stagingPath = [System.IO.Path]::Combine(
    $destinationParent,
    ".parser-pack-acquisition-$([System.Guid]::NewGuid().ToString('N'))"
)
[System.IO.Directory]::CreateDirectory($stagingPath) | Out-Null

$handler = $null
$client = $null
$published = $false
try {
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.AllowAutoRedirect = $true
    $handler.MaxAutomaticRedirections = 5
    $handler.AutomaticDecompression = [System.Net.DecompressionMethods]::None
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [System.TimeSpan]::FromMinutes(10)
    $client.DefaultRequestHeaders.UserAgent.ParseAdd("ProjectAtlas-parser-pack-acquisition/1")

    Receive-PinnedFile `
        -Client $client `
        -Uri $bundleUri `
        -OutputPath ([System.IO.Path]::Combine($stagingPath, "source-bundle.tar.zst")) `
        -ExpectedBytes $bundleBytes `
        -ExpectedSha256 $bundleSha256 `
        -MaximumBytes $maxNativeBundleBytes
    Receive-PinnedFile `
        -Client $client `
        -Uri $upstreamUri `
        -OutputPath ([System.IO.Path]::Combine($stagingPath, "parsers.json")) `
        -ExpectedBytes $upstreamBytes `
        -ExpectedSha256 $upstreamSha256 `
        -MaximumBytes $maxUpstreamManifestBytes

    [System.IO.Directory]::Move($stagingPath, $destinationPath)
    $published = $true
}
finally {
    if ($null -ne $client) {
        $client.Dispose()
    }
    elseif ($null -ne $handler) {
        $handler.Dispose()
    }
    if (-not $published -and [System.IO.Directory]::Exists($stagingPath)) {
        [System.IO.Directory]::Delete($stagingPath, $true)
    }
}

[pscustomobject]@{
    target = $Target
    source_manifest_sha256 = $observedManifestSha256
    native_bundle = [pscustomobject]@{
        file = "source-bundle.tar.zst"
        bytes = $bundleBytes
        sha256 = $bundleSha256
    }
    upstream_parser_manifest = [pscustomobject]@{
        file = "parsers.json"
        bytes = $upstreamBytes
        sha256 = $upstreamSha256
    }
} | ConvertTo-Json -Depth 4 -Compress
