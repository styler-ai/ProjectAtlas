# Purpose: Install or update the ProjectAtlas plugin runtime and Windows MCP configs.

param(
    [string]$ProjectRoot,
    [string]$Repository = "https://github.com/styler-ai/ProjectAtlas",
    [string]$ProjectAtlasVersion,
    [string]$ReleaseBaseUrl = "https://github.com/styler-ai/ProjectAtlas/releases/download",
    [string]$RuntimePath,
    [switch]$ReleaseBinaryOnly
)

$ErrorActionPreference = "Stop"

function Resolve-DefaultProjectRoot {
    (Get-Location).Path
}

function Test-Truthy {
    param(
        [string]$Value
    )
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $false
    }
    return @("1", "true", "yes", "on") -contains $Value.ToLowerInvariant()
}

function Resolve-PluginReleaseVersion {
    $scriptDirectory = Split-Path -Parent $PSCommandPath
    $pluginRoot = Split-Path -Parent $scriptDirectory
    $manifestPath = Join-Path $pluginRoot ".codex-plugin\plugin.json"
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        return $null
    }
    try {
        $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
        if ($manifest.version) {
            return "v$($manifest.version)"
        }
    }
    catch {
        return $null
    }
    return $null
}

function Find-Cargo {
    $cargoHome = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path -LiteralPath $cargoHome) {
        return $cargoHome
    }
    $cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cargoCommand) {
        return $cargoCommand.Source
    }
    return $null
}

function Convert-ProjectAtlasVersionTag {
    param(
        [string]$Version
    )
    if ([string]::IsNullOrWhiteSpace($Version)) {
        return $null
    }
    return $Version.Trim().TrimStart("v")
}

function Test-ProjectAtlasRuntime {
    param(
        [string]$FilePath,
        [string]$ExpectedVersion
    )
    if (-not $FilePath -or -not (Test-Path -LiteralPath $FilePath)) {
        return $false
    }
    try {
        $runtimeJson = & $FilePath --format json runtime-info 2>$null | Out-String
        if ($LASTEXITCODE -ne 0) {
            return $false
        }
        $payload = $runtimeJson | ConvertFrom-Json
        $runtime = if ($payload.runtime) { $payload.runtime } else { $payload }
        $expectedRuntimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
        $versionMatches = -not $expectedRuntimeVersion -or $runtime.version -eq $expectedRuntimeVersion
        return $runtime.project -eq "ProjectAtlas" `
            -and [int]$runtime.major_version -ge 3 `
            -and @($runtime.capabilities) -contains "mcp" `
            -and $runtime.text_format -eq "TOON" `
            -and $versionMatches
    }
    catch {
        return $false
    }
}

function Get-ProjectAtlasRuntimeVersion {
    param(
        [string]$FilePath
    )
    if (-not $FilePath -or -not (Test-Path -LiteralPath $FilePath)) {
        return $null
    }
    try {
        $runtimeJson = & $FilePath --format json runtime-info 2>$null | Out-String
        if ($LASTEXITCODE -ne 0) {
            return $null
        }
        $payload = $runtimeJson | ConvertFrom-Json
        $runtime = if ($payload.runtime) { $payload.runtime } else { $payload }
        return $runtime.version
    }
    catch {
        return $null
    }
}

function Get-KnownProjectAtlasShimPaths {
    $paths = @()
    if ($env:USERPROFILE) {
        $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
        $paths += @(
            (Join-Path $cargoBin "projectatlas.exe"),
            (Join-Path $cargoBin "projectatlas.cmd"),
            (Join-Path $cargoBin "projectatlas.ps1")
        )
    }
    if ($env:APPDATA) {
        $npmBin = Join-Path $env:APPDATA "npm"
        $paths += @(
            (Join-Path $npmBin "projectatlas.exe"),
            (Join-Path $npmBin "projectatlas.cmd"),
            (Join-Path $npmBin "projectatlas.ps1"),
            (Join-Path $npmBin "projectatlas")
        )
    }
    return @($paths | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
}

function Test-KnownProjectAtlasShimPath {
    param(
        [string]$FilePath
    )
    if (-not $FilePath) {
        return $false
    }
    $normalized = Get-NormalizedPathEntry $FilePath
    foreach ($knownPath in (Get-KnownProjectAtlasShimPaths)) {
        if ($normalized -eq (Get-NormalizedPathEntry $knownPath)) {
            return $true
        }
    }
    return $false
}

function New-ProjectAtlasShimQuarantinePath {
    param(
        [string]$FilePath,
        [string]$Version
    )
    $safeVersion = if ([string]::IsNullOrWhiteSpace($Version)) { "unknown" } else { $Version -replace '[^A-Za-z0-9_.-]', '_' }
    $basePath = "$FilePath.projectatlas-stale-$safeVersion.bak"
    if (-not (Test-Path -LiteralPath $basePath)) {
        return $basePath
    }
    $timestampPath = "$basePath.$(Get-Date -Format 'yyyyMMddHHmmss')"
    if (-not (Test-Path -LiteralPath $timestampPath)) {
        return $timestampPath
    }
    return "$timestampPath.$([Guid]::NewGuid().ToString('N'))"
}

function Quarantine-ProjectAtlasStaleShims {
    param(
        [string]$VerifiedPath,
        [string]$ExpectedVersion
    )
    $expectedRuntimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if (-not $VerifiedPath -or -not $expectedRuntimeVersion) {
        return
    }
    $verified = Get-NormalizedPathEntry $VerifiedPath
    $candidates = @()
    $candidates += @(where.exe projectatlas 2>$null | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $candidates += Get-KnownProjectAtlasShimPaths
    $seen = @{}
    foreach ($candidate in $candidates) {
        if (-not (Test-Path -LiteralPath $candidate)) {
            continue
        }
        $normalized = Get-NormalizedPathEntry $candidate
        if ($normalized -eq $verified -or $seen.ContainsKey($normalized)) {
            continue
        }
        $seen[$normalized] = $true
        if (-not (Test-KnownProjectAtlasShimPath $candidate)) {
            continue
        }
        if (-not (Test-ProjectAtlasRuntime $candidate $null)) {
            continue
        }
        $version = Get-ProjectAtlasRuntimeVersion $candidate
        if ([string]::IsNullOrWhiteSpace($version) -or $version -eq $expectedRuntimeVersion) {
            continue
        }
        try {
            $quarantinePath = New-ProjectAtlasShimQuarantinePath $candidate $version
            Move-Item -LiteralPath $candidate -Destination $quarantinePath
            Write-Output "Quarantined stale ProjectAtlas shim: $candidate -> $quarantinePath version '$version'"
        }
        catch {
            Write-Warning "Could not quarantine stale ProjectAtlas shim ${candidate} version '$version': $($_.Exception.Message)"
        }
    }
}

function Split-PathList {
    param(
        [string]$Value
    )
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return @()
    }
    return $Value -split ";" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
}

function Get-NormalizedPathEntry {
    param(
        [string]$Value
    )
    try {
        return ([System.IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($Value))).TrimEnd("\")
    }
    catch {
        return $Value.TrimEnd("\")
    }
}

function Set-ProjectAtlasPathPrecedence {
    param(
        [string]$FilePath
    )
    $runtimeDir = Split-Path -Parent $FilePath
    if (-not $runtimeDir) {
        return
    }

    $normalizedRuntimeDir = Get-NormalizedPathEntry $runtimeDir

    $processEntries = Split-PathList $env:Path
    $processEntries = @($processEntries | Where-Object { (Get-NormalizedPathEntry $_) -ne $normalizedRuntimeDir })
    $env:Path = (@($runtimeDir) + $processEntries) -join ";"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $userEntries = Split-PathList $userPath
    $userEntries = @($userEntries | Where-Object { (Get-NormalizedPathEntry $_) -ne $normalizedRuntimeDir })
    [Environment]::SetEnvironmentVariable("Path", ((@($runtimeDir) + $userEntries) -join ";"), "User")
}

function Sync-ProjectAtlasRuntimeToLocalAppData {
    param(
        [string]$FilePath,
        [string]$ExpectedVersion
    )
    if (-not (Test-ProjectAtlasRuntime $FilePath $ExpectedVersion)) {
        return $null
    }
    $installDir = Join-Path $env:LOCALAPPDATA "ProjectAtlas\bin"
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    $target = Join-Path $installDir "projectatlas.exe"
    if ((Get-NormalizedPathEntry $FilePath) -ne (Get-NormalizedPathEntry $target)) {
        try {
            Copy-Item -LiteralPath $FilePath -Destination $target -Force
        }
        catch {
            Write-Warning "ProjectAtlas LocalAppData mirror skipped because ${target} is locked: $($_.Exception.Message)"
            return $FilePath
        }
    }
    if (Test-ProjectAtlasRuntime $target $ExpectedVersion) {
        return $target
    }
    return $FilePath
}

function Find-ProjectAtlas {
    param(
        [string]$ExpectedVersion
    )
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "ProjectAtlas\bin\projectatlas.exe"),
        (Join-Path $env:USERPROFILE ".cargo\bin\projectatlas.exe")
    )
    foreach ($candidate in $candidates) {
        if (Test-ProjectAtlasRuntime $candidate $ExpectedVersion) {
            return $candidate
        }
    }
    $projectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue
    if ($projectAtlasCommand -and (Test-ProjectAtlasRuntime $projectAtlasCommand.Source $ExpectedVersion)) {
        return $projectAtlasCommand.Source
    }
    return $null
}

function Write-ProjectAtlasPathShadowReport {
    param(
        [string]$VerifiedPath,
        [string]$ExpectedVersion
    )
    if (-not $VerifiedPath) {
        return
    }
    $verified = Get-NormalizedPathEntry $VerifiedPath
    $candidates = @(where.exe projectatlas 2>$null | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($candidates.Count -eq 0) {
        Write-Warning "Bare 'projectatlas' is not on PATH. Generated MCP configs use the verified absolute runtime: $VerifiedPath"
        return
    }
    $first = Get-NormalizedPathEntry $candidates[0]
    if ($first -ne $verified) {
        $firstVersion = Get-ProjectAtlasRuntimeVersion $candidates[0]
        Write-Warning "Bare 'projectatlas' resolves to $($candidates[0]) version '$firstVersion', not the verified runtime $VerifiedPath. Start a new shell, put $(Split-Path -Parent $VerifiedPath) first on PATH, or remove the obsolete shim."
    }
    foreach ($candidate in $candidates) {
        $normalized = Get-NormalizedPathEntry $candidate
        if ($normalized -eq $verified) {
            continue
        }
        if (-not (Test-ProjectAtlasRuntime $candidate $ExpectedVersion)) {
            $version = Get-ProjectAtlasRuntimeVersion $candidate
            Write-Warning "Obsolete ProjectAtlas runtime or shim still exists on PATH: $candidate version '$version'. It was not removed automatically."
        }
    }
}

function Invoke-Checked {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

function Install-ReleaseBinary {
    param(
        [string]$Version,
        [string]$BaseUrl
    )
    if (-not $Version) {
        return $null
    }
    $asset = "projectatlas-$Version-x86_64-pc-windows-msvc.zip"
    $url = "$BaseUrl/$Version/$asset"
    $installDir = Join-Path $env:LOCALAPPDATA "ProjectAtlas\bin"
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("projectatlas-" + [guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
    $archive = Join-Path $tempDir $asset
    try {
        Invoke-WebRequest -Uri $url -OutFile $archive
        Expand-Archive -LiteralPath $archive -DestinationPath $tempDir -Force
        $binary = Get-ChildItem -LiteralPath $tempDir -Filter "projectatlas.exe" -Recurse | Select-Object -First 1
        if (-not $binary) {
            throw "Release archive did not contain projectatlas.exe"
        }
        $target = Join-Path $installDir "projectatlas.exe"
        Copy-Item -LiteralPath $binary.FullName -Destination $target -Force
        return $target
    }
    catch {
        Write-Warning "Release binary install failed from ${url}: $($_.Exception.Message)"
        return $null
    }
    finally {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if (-not $ProjectRoot) {
    $ProjectRoot = Resolve-DefaultProjectRoot
}

if (-not $ProjectAtlasVersion) {
    if ($env:PROJECTATLAS_VERSION) {
        $ProjectAtlasVersion = $env:PROJECTATLAS_VERSION
    }
    else {
        $ProjectAtlasVersion = Resolve-PluginReleaseVersion
    }
}

if (-not $RuntimePath -and $env:PROJECTATLAS_RUNTIME_PATH) {
    $RuntimePath = $env:PROJECTATLAS_RUNTIME_PATH
}

$releaseBinaryOnly = $ReleaseBinaryOnly -or (Test-Truthy $env:PROJECTATLAS_RELEASE_BINARY_ONLY)
$ProjectRoot = (Resolve-Path $ProjectRoot).Path

if ($RuntimePath) {
    $projectAtlas = (Resolve-Path $RuntimePath).Path
    if (-not (Test-ProjectAtlasRuntime $projectAtlas $ProjectAtlasVersion)) {
        throw "Provided ProjectAtlas runtime does not satisfy the ProjectAtlas runtime/version contract: $projectAtlas"
    }
}
else {
    $cargo = Find-Cargo
    $sourceManifest = Join-Path $ProjectRoot "crates\projectatlas-cli\Cargo.toml"
    $installedBinary = $null

    if ($releaseBinaryOnly) {
        $installedBinary = Install-ReleaseBinary $ProjectAtlasVersion $ReleaseBaseUrl
        if (-not $installedBinary) {
            throw "ProjectAtlas release-binary install was required but failed for $ProjectAtlasVersion."
        }
    }
    elseif ($cargo -and (Test-Path -LiteralPath $sourceManifest)) {
        Push-Location $ProjectRoot
        try {
            Invoke-Checked $cargo @("install", "--path", "crates/projectatlas-cli", "--locked", "--force")
        }
        finally {
            Pop-Location
        }
    }
    else {
        $releaseBinary = Install-ReleaseBinary $ProjectAtlasVersion $ReleaseBaseUrl
        if ($releaseBinary) {
            $installedBinary = $releaseBinary
        }
        if (-not $releaseBinary -and $cargo) {
            $installArgs = @("install", "--git", $Repository)
            if ($ProjectAtlasVersion) {
                $installArgs += @("--tag", $ProjectAtlasVersion)
            }
            $installArgs += @("projectatlas-cli", "--locked", "--force")
            Invoke-Checked $cargo $installArgs
        }
    }

    $projectAtlas = if ($installedBinary -and (Test-ProjectAtlasRuntime $installedBinary $ProjectAtlasVersion)) { $installedBinary } else { Find-ProjectAtlas $ProjectAtlasVersion }
    if (-not $projectAtlas) {
        throw "A ProjectAtlas runtime matching $ProjectAtlasVersion was not found. Install Rust/Cargo or provide the matching ProjectAtlas release binary on PATH."
    }
    $mirroredProjectAtlas = Sync-ProjectAtlasRuntimeToLocalAppData $projectAtlas $ProjectAtlasVersion
    if ($mirroredProjectAtlas) {
        $projectAtlas = $mirroredProjectAtlas
    }

    Set-ProjectAtlasPathPrecedence $projectAtlas
}
Invoke-Checked $projectAtlas @("--format", "json", "runtime-info") | Out-Null
Quarantine-ProjectAtlasStaleShims $projectAtlas $ProjectAtlasVersion
Write-ProjectAtlasPathShadowReport $projectAtlas $ProjectAtlasVersion

$atlasDir = Join-Path $ProjectRoot ".projectatlas"
New-Item -ItemType Directory -Force -Path $atlasDir | Out-Null
$dbPath = Join-Path $atlasDir "projectatlas.db"
$projectConfigPath = Join-Path $atlasDir "config.toml"
$flatConfigPath = Join-Path $ProjectRoot "projectatlas.toml"
$mcpConfigPath = Join-Path $atlasDir "projectatlas.mcp.json"
$claudeMcpConfigPath = Join-Path $atlasDir "projectatlas.claude.mcp.json"
$opencodeConfigPath = Join-Path $atlasDir "projectatlas.opencode.json"

function Write-ProjectAtlasMcpConfig {
    param(
        [string]$OutputPath,
        [string]$Harness
    )
    $mcpArgs = @("--format", "json", "--db", $dbPath)
    if (Test-Path -LiteralPath $projectConfigPath) {
        $mcpArgs += @("--config", $projectConfigPath)
    }
    elseif (Test-Path -LiteralPath $flatConfigPath) {
        $mcpArgs += @("--config", $flatConfigPath)
    }
    $mcpArgs += @("mcp-config")
    if ($Harness) {
        $mcpArgs += @("--harness", $Harness)
    }
    $mcpConfig = & $projectAtlas @mcpArgs
    if ($LASTEXITCODE -ne 0) {
        throw "ProjectAtlas MCP config generation failed with exit code $LASTEXITCODE for harness '$Harness'."
    }
    $utf8NoBom = New-Object System.Text.UTF8Encoding -ArgumentList $false
    $mcpConfigText = ($mcpConfig -join [Environment]::NewLine) + [Environment]::NewLine
    [System.IO.File]::WriteAllText($OutputPath, $mcpConfigText, $utf8NoBom)
}

Write-ProjectAtlasMcpConfig $mcpConfigPath $null
Write-ProjectAtlasMcpConfig $claudeMcpConfigPath "claude-code"
Write-ProjectAtlasMcpConfig $opencodeConfigPath "opencode"

Write-Output "ProjectAtlas runtime installed and verified: $projectAtlas"
Write-Output "ProjectAtlas update preserved project state under $atlasDir; use reset-index --apply for explicit state cleanup."
Write-Output "Project-local MCP config written: $mcpConfigPath"
Write-Output "Project-local Claude Code MCP config written: $claudeMcpConfigPath"
Write-Output "Project-local OpenCode MCP config written: $opencodeConfigPath"
