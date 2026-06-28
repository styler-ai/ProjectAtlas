param(
    [string]$ProjectRoot,
    [string]$Repository = "https://github.com/styler-ai/ProjectAtlas",
    [string]$ProjectAtlasVersion = "v0.3.0",
    [string]$ReleaseBaseUrl = "https://github.com/styler-ai/ProjectAtlas/releases/download"
)

$ErrorActionPreference = "Stop"

function Resolve-DefaultProjectRoot {
    (Get-Location).Path
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

function Test-ProjectAtlasRuntime {
    param(
        [string]$FilePath
    )
    if (-not $FilePath -or -not (Test-Path -LiteralPath $FilePath)) {
        return $false
    }
    $help = & $FilePath --help 2>$null | Out-String
    if ($LASTEXITCODE -ne 0) {
        return $false
    }
    return $help.Contains("ProjectAtlas 3 repository intelligence engine") -and $help.Contains("mcp-config")
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

function Find-ProjectAtlas {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "ProjectAtlas\bin\projectatlas.exe"),
        (Join-Path $env:USERPROFILE ".cargo\bin\projectatlas.exe")
    )
    foreach ($candidate in $candidates) {
        if (Test-ProjectAtlasRuntime $candidate) {
            return $candidate
        }
    }
    $projectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue
    if ($projectAtlasCommand -and (Test-ProjectAtlasRuntime $projectAtlasCommand.Source)) {
        return $projectAtlasCommand.Source
    }
    return $null
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

$ProjectRoot = (Resolve-Path $ProjectRoot).Path
$cargo = Find-Cargo
$sourceManifest = Join-Path $ProjectRoot "crates\projectatlas-cli\Cargo.toml"

if ($cargo -and (Test-Path -LiteralPath $sourceManifest)) {
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
    if (-not $releaseBinary -and $cargo) {
        $installArgs = @("install", "--git", $Repository, "--package", "projectatlas-cli", "--locked", "--force")
        if ($ProjectAtlasVersion) {
            $installArgs += @("--tag", $ProjectAtlasVersion)
        }
        Invoke-Checked $cargo $installArgs
    }
}

$projectAtlas = Find-ProjectAtlas
if (-not $projectAtlas) {
    throw "ProjectAtlas 3 runtime was not found. Install Rust/Cargo or provide a compatible ProjectAtlas 3 release binary on PATH."
}

Set-ProjectAtlasPathPrecedence $projectAtlas
Invoke-Checked $projectAtlas @("--help") | Out-Null

$atlasDir = Join-Path $ProjectRoot ".projectatlas"
New-Item -ItemType Directory -Force -Path $atlasDir | Out-Null
$dbPath = Join-Path $atlasDir "projectatlas.db"
$configPath = Join-Path $atlasDir "config.toml"
$mcpConfigPath = Join-Path $atlasDir "projectatlas.mcp.json"
$mcpArgs = @("--format", "json", "--db", $dbPath)
if (Test-Path -LiteralPath $configPath) {
    $mcpArgs += @("--config", $configPath)
}
$mcpArgs += @("mcp-config")
$mcpConfig = & $projectAtlas @mcpArgs
if ($LASTEXITCODE -ne 0) {
    throw "ProjectAtlas MCP config generation failed with exit code $LASTEXITCODE."
}
$mcpConfig | Set-Content -LiteralPath $mcpConfigPath -Encoding utf8

Write-Output "ProjectAtlas runtime installed and verified: $projectAtlas"
Write-Output "Project-local MCP config written: $mcpConfigPath"
