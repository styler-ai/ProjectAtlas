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

function Assert-ProjectAtlasDirectPath {
    param(
        [string]$Path,
        [string]$Label
    )
    $item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($item -and (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "$Label must not be a symlink, junction, or reparse point: $Path"
    }
}

function Assert-ProjectAtlasDirectFilePath {
    param(
        [string]$Path,
        [string]$Label
    )
    Assert-ProjectAtlasDirectPath $Path $Label
    $item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($item -and -not ($item -is [System.IO.FileInfo])) {
        throw "$Label must be a regular file: $Path"
    }
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

function Invoke-ProjectAtlasRuntimeInfo {
    param(
        [string]$FilePath
    )
    if (-not $FilePath -or -not (Test-Path -LiteralPath $FilePath)) {
        return $null
    }
    $probeTimeoutMs = 5000
    $maximumOutputBytes = 1024 * 1024
    $probeId = [Guid]::NewGuid().ToString("N")
    $standardOutput = Join-Path ([IO.Path]::GetTempPath()) "projectatlas-runtime-probe-$probeId.stdout"
    $standardError = Join-Path ([IO.Path]::GetTempPath()) "projectatlas-runtime-probe-$probeId.stderr"
    $probeFiles = @($standardOutput, $standardError)
    $process = $null
    try {
        $process = Start-Process `
            -FilePath $FilePath `
            -ArgumentList @("--format", "json", "runtime-info") `
            -WindowStyle Hidden `
            -PassThru `
            -RedirectStandardOutput $standardOutput `
            -RedirectStandardError $standardError
        # Windows PowerShell 5 can lose a fast child's exit code unless its handle is opened first.
        [void]$process.Handle
        $probeClock = [Diagnostics.Stopwatch]::StartNew()
        do {
            $exited = $process.WaitForExit(25)
            $outputLimitExceeded = $false
            foreach ($probeFile in $probeFiles) {
                if ((Test-Path -LiteralPath $probeFile) `
                    -and (Get-Item -LiteralPath $probeFile).Length -gt $maximumOutputBytes) {
                    $outputLimitExceeded = $true
                    break
                }
            }
            if ($outputLimitExceeded -or (-not $exited -and $probeClock.ElapsedMilliseconds -ge $probeTimeoutMs)) {
                if (-not $exited) {
                    $process.Kill()
                    [void]$process.WaitForExit($probeTimeoutMs)
                }
                return $null
            }
        }
        while (-not $exited)
        $process.WaitForExit()
        $process.Refresh()
        if ($process.ExitCode -ne 0) {
            return $null
        }
        foreach ($probeFile in $probeFiles) {
            if ((Get-Item -LiteralPath $probeFile -ErrorAction Stop).Length -gt $maximumOutputBytes) {
                return $null
            }
        }
        $runtimeJson = Get-Content -Raw -LiteralPath $standardOutput
        $payload = $runtimeJson | ConvertFrom-Json
        return $(if ($payload.runtime) { $payload.runtime } else { $payload })
    }
    catch {
        return $null
    }
    finally {
        if ($process) {
            if (-not $process.HasExited) {
                $process.Kill()
                [void]$process.WaitForExit($probeTimeoutMs)
            }
            $process.Dispose()
        }
        Remove-Item -LiteralPath $standardOutput, $standardError -Force -ErrorAction SilentlyContinue
    }
}

function Test-ProjectAtlasRuntime {
    param(
        [string]$FilePath,
        [string]$ExpectedVersion
    )
    $runtime = Invoke-ProjectAtlasRuntimeInfo $FilePath
    if (-not $runtime) {
        return $false
    }
    $majorVersion = 0
    if (-not [int]::TryParse([string]$runtime.major_version, [ref]$majorVersion)) {
        return $false
    }
    $expectedRuntimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    $versionMatches = -not $expectedRuntimeVersion -or $runtime.version -eq $expectedRuntimeVersion
    return $runtime.project -eq "ProjectAtlas" `
        -and $majorVersion -ge 3 `
        -and @($runtime.capabilities) -contains "mcp" `
        -and $runtime.text_format -eq "TOON" `
        -and $versionMatches
}

function Get-ProjectAtlasRuntimeVersion {
    param(
        [string]$FilePath
    )
    $runtime = Invoke-ProjectAtlasRuntimeInfo $FilePath
    return $(if ($runtime) { $runtime.version } else { $null })
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

function Set-ProjectAtlasProcessPathPrecedence {
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
}

function Test-ProjectAtlasBareCommandResolutionOnPath {
    param(
        [string]$PathValue,
        [string]$VerifiedPath
    )
    $installerProcessPath = $env:Path
    try {
        $env:Path = [Environment]::ExpandEnvironmentVariables($PathValue)
        $command = Get-Command projectatlas -ErrorAction SilentlyContinue | Select-Object -First 1
        return $command `
            -and (Get-NormalizedPathEntry $command.Source) -eq (Get-NormalizedPathEntry $VerifiedPath)
    }
    finally {
        $env:Path = $installerProcessPath
    }
}

function Set-ProjectAtlasPathPrecedence {
    param(
        [string]$FilePath
    )
    Set-ProjectAtlasProcessPathPrecedence $FilePath
    $runtimeDir = Split-Path -Parent $FilePath
    if (-not $runtimeDir) {
        return $false
    }

    $normalizedRuntimeDir = Get-NormalizedPathEntry $runtimeDir

    if (Test-Truthy $env:PROJECTATLAS_SKIP_USER_PATH_UPDATE) {
        return $false
    }

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $userEntries = Split-PathList $userPath
    $userEntries = @($userEntries | Where-Object { (Get-NormalizedPathEntry $_) -ne $normalizedRuntimeDir })
    $futureUserPath = (@($runtimeDir) + $userEntries) -join ";"
    [Environment]::SetEnvironmentVariable("Path", $futureUserPath, "User")
    $machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $freshProcessPath = @($machinePath, $futureUserPath) -join ";"
    return (Test-ProjectAtlasBareCommandResolutionOnPath $freshProcessPath $FilePath)
}

function Confirm-ProjectAtlasBareCommandResolution {
    param(
        [string]$VerifiedPath,
        [string]$ExpectedVersion
    )
    if (-not $VerifiedPath) {
        return
    }
    $verified = Get-NormalizedPathEntry $VerifiedPath
    $projectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue
    if (-not $projectAtlasCommand) {
        Write-Warning "Active process still cannot resolve bare 'projectatlas'. Generated MCP configs use the verified absolute runtime: $VerifiedPath. Restart Codex or the shell before relying on bare projectatlas."
        return
    }
    $commandPath = $projectAtlasCommand.Source
    if ((Get-NormalizedPathEntry $commandPath) -eq $verified -and (Test-ProjectAtlasRuntime $commandPath $ExpectedVersion)) {
        Write-Output "Active process resolves bare projectatlas to verified runtime: $commandPath"
        return
    }
    $commandVersion = Get-ProjectAtlasRuntimeVersion $commandPath
    Write-Warning "Active process still resolves bare 'projectatlas' to $commandPath version '$commandVersion', not the verified runtime $VerifiedPath. Generated MCP configs use the absolute runtime; restart Codex or the shell, put $(Split-Path -Parent $VerifiedPath) first on PATH, or remove the obsolete shim before relying on bare projectatlas."
}

function Sync-ProjectAtlasRuntimeToLocalAppData {
    param(
        [string]$FilePath,
        [string]$ExpectedVersion
    )
    $synchronizationVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if (-not $synchronizationVersion) {
        $synchronizationVersion = Convert-ProjectAtlasVersionTag (Get-ProjectAtlasRuntimeVersion $FilePath)
    }
    if (-not $synchronizationVersion -or -not (Test-ProjectAtlasRuntime $FilePath $synchronizationVersion)) {
        return $false
    }
    $installDir = Join-Path $env:LOCALAPPDATA "ProjectAtlas\bin"
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    $target = Join-Path $installDir "projectatlas.exe"
    if (Test-ProjectAtlasRuntime $target $synchronizationVersion) {
        return $true
    }
    if ((Get-NormalizedPathEntry $FilePath) -ne (Get-NormalizedPathEntry $target)) {
        try {
            Copy-Item -LiteralPath $FilePath -Destination $target -Force
        }
        catch {
            Write-Warning "ProjectAtlas LocalAppData mirror skipped because ${target} is locked: $($_.Exception.Message) Close any running ProjectAtlas or Codex session using that file, then rerun this installer. Codex MCP and generated configs continue to use verified runtime $FilePath."
            return $false
        }
    }
    return (Test-ProjectAtlasRuntime $target $synchronizationVersion)
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
            Write-Warning "Obsolete ProjectAtlas runtime or shim still exists on PATH: $candidate version '$version'. It was not removed automatically. Close any process using that file, then rerun this installer or remove the shim manually; generated MCP configs use the verified runtime $VerifiedPath."
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

function Get-ProjectAtlasMcpLaunchArguments {
    param(
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath,
        [string]$ExpectedVersion
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        return @()
    }
    $launchArgs = @("--require-version", $runtimeVersion, "--db", $DbPath)
    if (Test-Path -LiteralPath $ProjectConfigPath) {
        $launchArgs += @("--config", $ProjectConfigPath)
    }
    elseif (Test-Path -LiteralPath $FlatConfigPath) {
        $launchArgs += @("--config", $FlatConfigPath)
    }
    $launchArgs += "mcp"
    return $launchArgs
}

function Get-ProjectAtlasComparablePath {
    param(
        [string]$Path
    )
    if ([string]::IsNullOrWhiteSpace($Path)) {
        return ""
    }
    return [System.IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
}

function Assert-ProjectAtlasEquivalentPath {
    param(
        [string]$Actual,
        [string]$Expected,
        [string]$Label
    )
    if ([string]::IsNullOrWhiteSpace($Actual)) {
        throw "${Label} is missing."
    }
    if (-not [System.IO.Path]::IsPathRooted($Actual)) {
        throw "${Label} path is not absolute: $Actual"
    }
    $actualPath = Get-ProjectAtlasComparablePath $Actual
    $expectedPath = Get-ProjectAtlasComparablePath $Expected
    if (-not [string]::Equals($actualPath, $expectedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "${Label} path mismatch: expected $Expected, found $Actual"
    }
}

function Assert-ProjectAtlasArgumentValue {
    param(
        [object[]]$Arguments,
        [string]$Name,
        [string]$Expected,
        [string]$Label,
        [switch]$PathValue
    )
    for ($index = 0; $index -lt $Arguments.Count - 1; $index += 1) {
        if ([string]$Arguments[$index] -ne $Name) {
            continue
        }
        $actual = [string]$Arguments[$index + 1]
        if ($PathValue) {
            Assert-ProjectAtlasEquivalentPath $actual $Expected $Label
        }
        elseif ($actual -ne $Expected) {
            throw "${Label} mismatch: expected $Expected, found $actual"
        }
        return
    }
    throw "${Label} argument $Name is missing."
}

function Get-ProjectAtlasEffectiveConfigPath {
    param(
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath
    )
    if (Test-Path -LiteralPath $ProjectConfigPath) {
        return $ProjectConfigPath
    }
    if (Test-Path -LiteralPath $FlatConfigPath) {
        return $FlatConfigPath
    }
    return $null
}

function Confirm-ProjectAtlasGeneratedMcpConfig {
    param(
        [string]$ConfigPath,
        [string]$Harness,
        [string]$VerifiedPath,
        [string]$ExpectedVersion,
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath,
        [string]$ProjectRoot
    )
    if (-not (Test-Path -LiteralPath $ConfigPath)) {
        throw "${Harness} ProjectAtlas generated MCP config was not written: $ConfigPath"
    }
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        $runtimeVersion = Get-ProjectAtlasRuntimeVersion $VerifiedPath
    }
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        throw "${Harness} ProjectAtlas generated MCP config cannot be verified because the runtime version is unknown."
    }
    $config = Get-Content -LiteralPath $ConfigPath -Raw | ConvertFrom-Json
    $expectedConfigPath = Get-ProjectAtlasEffectiveConfigPath $ProjectConfigPath $FlatConfigPath
    if ($Harness -eq "Claude Code") {
        $server = $config.mcpServers.projectatlas
        if (-not $server) {
            throw "Claude Code generated MCP config is missing mcpServers.projectatlas."
        }
        Assert-ProjectAtlasEquivalentPath ([string]$server.command) $VerifiedPath "Claude Code command"
        $arguments = @($server.args)
        if ($server.PSObject.Properties.Name -contains "cwd") {
            throw "Claude Code generated MCP config must not rely on cwd."
        }
    }
    elseif ($Harness -eq "OpenCode") {
        $server = $config.mcp.projectatlas
        if (-not $server) {
            throw "OpenCode generated MCP config is missing mcp.projectatlas."
        }
        if ([string]$server.type -ne "local") {
            throw "OpenCode generated MCP config type mismatch: expected local, found $($server.type)"
        }
        if ($server.enabled -ne $true) {
            throw "OpenCode generated MCP config must set enabled=true."
        }
        Assert-ProjectAtlasEquivalentPath ([string]$server.cwd) $ProjectRoot "OpenCode cwd"
        $command = @($server.command)
        if ($command.Count -lt 2) {
            throw "OpenCode generated MCP config command array is incomplete."
        }
        Assert-ProjectAtlasEquivalentPath ([string]$command[0]) $VerifiedPath "OpenCode command"
        $arguments = @($command | Select-Object -Skip 1)
    }
    else {
        throw "Unsupported generated MCP config harness: $Harness"
    }
    Assert-ProjectAtlasArgumentValue $arguments "--require-version" $runtimeVersion "${Harness} --require-version"
    Assert-ProjectAtlasArgumentValue $arguments "--db" $DbPath "${Harness} --db" -PathValue
    if ($expectedConfigPath) {
        Assert-ProjectAtlasArgumentValue $arguments "--config" $expectedConfigPath "${Harness} --config" -PathValue
    }
    if ($arguments.Count -eq 0 -or [string]$arguments[$arguments.Count - 1] -ne "mcp") {
        throw "${Harness} generated MCP config does not end with mcp."
    }
    Write-Output "${Harness} ProjectAtlas generated MCP config verified for runtime $VerifiedPath and database $DbPath."
}

function Resolve-ProjectAtlasCodexCommand {
    param(
        [string]$Operation
    )
    $codexCommandPath = $null
    if (-not [string]::IsNullOrWhiteSpace($env:PROJECTATLAS_CODEX_COMMAND)) {
        $codexCommandPath = (Resolve-Path $env:PROJECTATLAS_CODEX_COMMAND -ErrorAction SilentlyContinue).Path
        if (-not $codexCommandPath) {
            $codexCommand = Get-Command $env:PROJECTATLAS_CODEX_COMMAND -ErrorAction SilentlyContinue
            if ($codexCommand) {
                $codexCommandPath = $codexCommand.Source
            }
        }
        if (-not $codexCommandPath) {
            Write-Warning "${Operation} skipped: PROJECTATLAS_CODEX_COMMAND does not resolve."
            return $null
        }
    }
    else {
        $codexCommand = Get-Command codex -ErrorAction SilentlyContinue
        if ($codexCommand) {
            $codexCommandPath = $codexCommand.Source
        }
    }
    if (-not $codexCommandPath) {
        Write-Host "${Operation} skipped: codex command not found."
        return $null
    }
    return $codexCommandPath
}

function Test-ProjectAtlasOfficialMarketplaceSource {
    param(
        [string]$Source
    )
    if ([string]::IsNullOrWhiteSpace($Source)) {
        return $false
    }
    $normalized = $Source.Trim().TrimEnd("/")
    $allowedSources = @(
        "styler-ai/ProjectAtlas",
        "styler-ai/ProjectAtlas.git",
        "https://github.com/styler-ai/ProjectAtlas",
        "https://github.com/styler-ai/ProjectAtlas.git",
        "git@github.com:styler-ai/ProjectAtlas",
        "git@github.com:styler-ai/ProjectAtlas.git",
        "ssh://git@github.com/styler-ai/ProjectAtlas",
        "ssh://git@github.com/styler-ai/ProjectAtlas.git"
    )
    foreach ($allowed in $allowedSources) {
        if ([string]::Equals($allowed, $normalized, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    return $false
}

function Get-ProjectAtlasCodexConfigPath {
    if (-not [string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
        return Join-Path $env:CODEX_HOME "config.toml"
    }
    if (-not [string]::IsNullOrWhiteSpace($env:USERPROFILE)) {
        return Join-Path $env:USERPROFILE ".codex\config.toml"
    }
    return $null
}

function Get-ProjectAtlasCodexMarketplaceRef {
    $configPath = Get-ProjectAtlasCodexConfigPath
    if (-not $configPath -or -not (Test-Path -LiteralPath $configPath)) {
        return $null
    }
    $inProjectAtlasMarketplace = $false
    foreach ($line in Get-Content -LiteralPath $configPath) {
        if ($line -match '^\s*\[marketplaces\.projectatlas\]\s*$') {
            $inProjectAtlasMarketplace = $true
            continue
        }
        if ($inProjectAtlasMarketplace -and $line -match '^\s*\[') {
            break
        }
        if ($inProjectAtlasMarketplace -and $line -match '^\s*ref\s*=\s*["'']([^"'']+)["'']') {
            return $Matches[1]
        }
    }
    return $null
}

function Restore-ProjectAtlasCodexMarketplace {
    param(
        [string]$CodexCommandPath,
        [string]$PreviousSource,
        [string]$PreviousRef
    )
    if ([string]::IsNullOrWhiteSpace($PreviousSource)) {
        return
    }
    & $CodexCommandPath plugin marketplace remove projectatlas --json | Out-Null
    $restoreArgs = @("plugin", "marketplace", "add", $PreviousSource)
    if (-not [string]::IsNullOrWhiteSpace($PreviousRef)) {
        $restoreArgs += @("--ref", $PreviousRef)
    }
    $restoreArgs += "--json"
    & $CodexCommandPath @restoreArgs | Out-Null
    if ($LASTEXITCODE -eq 0) {
        & $CodexCommandPath plugin add projectatlas --marketplace projectatlas --json | Out-Null
    }
}

function Get-ProjectAtlasCodexPlugin {
    param(
        [string]$CodexCommandPath
    )
    $pluginsText = & $CodexCommandPath plugin list --marketplace projectatlas --json 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) {
        return $null
    }
    try {
        $plugins = $pluginsText | ConvertFrom-Json
        $installed = @($plugins.installed)
        $projectAtlasPlugin = @($installed | Where-Object {
                $_.pluginId -eq "projectatlas@projectatlas" -or ($_.name -eq "projectatlas" -and $_.marketplaceName -eq "projectatlas")
            }) | Select-Object -First 1
        return $projectAtlasPlugin
    }
    catch {
        return $null
    }
    return $null
}

function Get-ProjectAtlasCodexPluginVersion {
    param(
        [string]$CodexCommandPath
    )
    $projectAtlasPlugin = Get-ProjectAtlasCodexPlugin $CodexCommandPath
    if ($projectAtlasPlugin -and $projectAtlasPlugin.version) {
        return $projectAtlasPlugin.version
    }
    return $null
}

function Get-ProjectAtlasCodexPluginSourcePath {
    param(
        [object]$ProjectAtlasPlugin
    )
    if (-not $ProjectAtlasPlugin) {
        return $null
    }
    foreach ($candidate in @($ProjectAtlasPlugin.source.path, $ProjectAtlasPlugin.path, $ProjectAtlasPlugin.root, $ProjectAtlasPlugin.location)) {
        if (-not [string]::IsNullOrWhiteSpace($candidate)) {
            return $candidate
        }
    }
    return $null
}

function Get-ProjectAtlasCodexPluginSourceManifestVersion {
    param(
        [object]$ProjectAtlasPlugin
    )
    $pluginSourcePath = Get-ProjectAtlasCodexPluginSourcePath $ProjectAtlasPlugin
    if ([string]::IsNullOrWhiteSpace($pluginSourcePath)) {
        return $null
    }
    $manifestPath = Join-Path $pluginSourcePath ".codex-plugin\plugin.json"
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        return ""
    }
    try {
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        if ($manifest.version) {
            return [string]$manifest.version
        }
    }
    catch {
        return ""
    }
    return ""
}

function Test-ProjectAtlasCodexPluginSourceManifest {
    param(
        [object]$ProjectAtlasPlugin,
        [string]$ExpectedVersion
    )
    $pluginSourcePath = Get-ProjectAtlasCodexPluginSourcePath $ProjectAtlasPlugin
    if ([string]::IsNullOrWhiteSpace($pluginSourcePath)) {
        return $true
    }
    return (Get-ProjectAtlasCodexPluginSourceManifestVersion $ProjectAtlasPlugin) -eq $ExpectedVersion
}

function Confirm-ProjectAtlasCodexSkillArtifact {
    param(
        [string]$CodexCommandPath,
        [string]$ExpectedVersion
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        Write-Output "Codex ProjectAtlas plugin skill verification skipped: ProjectAtlas version is unknown."
        return
    }
    $projectAtlasPlugin = Get-ProjectAtlasCodexPlugin $CodexCommandPath
    if (-not $projectAtlasPlugin) {
        Write-Warning "Codex ProjectAtlas plugin skill verification skipped: projectatlas plugin is not installed."
        return
    }
    if ($projectAtlasPlugin.version -ne $runtimeVersion) {
        Write-Warning "Codex ProjectAtlas plugin skill verification failed: installed projectatlas plugin version '$($projectAtlasPlugin.version)' does not match $runtimeVersion."
        return
    }
    $pluginSourcePath = Get-ProjectAtlasCodexPluginSourcePath $projectAtlasPlugin
    if ([string]::IsNullOrWhiteSpace($pluginSourcePath)) {
        Write-Output "Codex ProjectAtlas plugin skill version $runtimeVersion is installed; Codex does not expose the active in-process ProjectAtlas skill path. Restart Codex if this session still advertises an older ProjectAtlas skill."
        return
    }
    $manifestPath = Join-Path $pluginSourcePath ".codex-plugin\plugin.json"
    $skillPath = Join-Path $pluginSourcePath "skills\projectatlas\SKILL.md"
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        Write-Warning "Codex ProjectAtlas plugin skill verification failed: plugin manifest was not found at $manifestPath."
        return
    }
    if (-not (Test-Path -LiteralPath $skillPath)) {
        Write-Warning "Codex ProjectAtlas plugin skill verification failed: ProjectAtlas skill was not found at $skillPath."
        return
    }
    try {
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        if ($manifest.version -ne $runtimeVersion) {
            Write-Warning "Codex ProjectAtlas plugin skill verification failed: manifest version '$($manifest.version)' does not match $runtimeVersion."
            return
        }
    }
    catch {
        Write-Warning "Codex ProjectAtlas plugin skill verification failed: could not read $manifestPath."
        return
    }
    Write-Output "Codex ProjectAtlas plugin skill verified at $skillPath for $runtimeVersion."
    Write-Output "Codex does not expose the active in-process ProjectAtlas skill path; restart Codex if this session still advertises an older ProjectAtlas skill."
}

function Update-ProjectAtlasCodexPlugin {
    param(
        [string]$ExpectedVersion
    )
    if (Test-Truthy $env:PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE) {
        Write-Output "Codex ProjectAtlas plugin update skipped by PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE."
        return
    }
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        Write-Output "Codex ProjectAtlas plugin update skipped: ProjectAtlas version is unknown."
        return
    }
    $codexCommandPath = Resolve-ProjectAtlasCodexCommand "Codex ProjectAtlas plugin update"
    if (-not $codexCommandPath) {
        return
    }
    try {
        $marketplacesText = & $codexCommandPath plugin marketplace list --json 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) {
            Write-Output "Codex ProjectAtlas plugin update skipped: could not list Codex plugin marketplaces."
            return
        }
        $marketplaces = ($marketplacesText | ConvertFrom-Json).marketplaces
        $projectAtlasMarketplace = @($marketplaces | Where-Object { $_.name -eq "projectatlas" }) | Select-Object -First 1
        if (-not $projectAtlasMarketplace) {
            Write-Output "Codex ProjectAtlas plugin update skipped: projectatlas marketplace is not configured."
            return
        }
        $source = if ($projectAtlasMarketplace.marketplaceSource) { $projectAtlasMarketplace.marketplaceSource.source } else { $null }
        if (-not (Test-ProjectAtlasOfficialMarketplaceSource $source)) {
            Write-Output "Codex ProjectAtlas plugin update skipped: projectatlas marketplace is not the official styler-ai/ProjectAtlas source."
            return
        }

        $releaseTag = "v$runtimeVersion"
        $previousRef = Get-ProjectAtlasCodexMarketplaceRef
        $projectAtlasPlugin = Get-ProjectAtlasCodexPlugin $codexCommandPath
        $currentPluginVersion = if ($projectAtlasPlugin -and $projectAtlasPlugin.version) { $projectAtlasPlugin.version } else { $null }
        $currentSourceManifestMatches = Test-ProjectAtlasCodexPluginSourceManifest $projectAtlasPlugin $runtimeVersion
        if ($previousRef -eq $releaseTag -and $currentPluginVersion -eq $runtimeVersion -and $currentSourceManifestMatches) {
            Write-Output "Codex ProjectAtlas plugin marketplace already points to $releaseTag."
            Confirm-ProjectAtlasCodexSkillArtifact $codexCommandPath $ExpectedVersion
            return
        }
        if ($previousRef -eq $releaseTag) {
            if ($currentPluginVersion -eq $runtimeVersion -and -not $currentSourceManifestMatches) {
                $sourceManifestVersion = Get-ProjectAtlasCodexPluginSourceManifestVersion $projectAtlasPlugin
                Write-Output "Codex ProjectAtlas plugin source manifest version '$sourceManifestVersion' does not match $runtimeVersion; refreshing official projectatlas plugin cache."
            }
            & $codexCommandPath plugin remove projectatlas --marketplace projectatlas --json | Out-Null
            & $codexCommandPath plugin add projectatlas --marketplace projectatlas --json | Out-Null
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "Codex ProjectAtlas plugin update failed: could not install projectatlas plugin at $releaseTag."
                Restore-ProjectAtlasCodexMarketplace $codexCommandPath $source $previousRef
                return
            }
            $installedVersion = Get-ProjectAtlasCodexPluginVersion $codexCommandPath
            if ($installedVersion -ne $runtimeVersion) {
                Write-Warning "Codex ProjectAtlas plugin update failed: installed projectatlas plugin version '$installedVersion' does not match $runtimeVersion."
                Restore-ProjectAtlasCodexMarketplace $codexCommandPath $source $previousRef
                return
            }
            $installedPlugin = Get-ProjectAtlasCodexPlugin $codexCommandPath
            if (-not (Test-ProjectAtlasCodexPluginSourceManifest $installedPlugin $runtimeVersion)) {
                $sourceManifestVersion = Get-ProjectAtlasCodexPluginSourceManifestVersion $installedPlugin
                Write-Warning "Codex ProjectAtlas plugin update failed: source manifest version '$sourceManifestVersion' does not match $runtimeVersion after refresh."
                Restore-ProjectAtlasCodexMarketplace $codexCommandPath $source $previousRef
                return
            }
            Write-Output "Codex ProjectAtlas plugin marketplace updated to $releaseTag."
            Confirm-ProjectAtlasCodexSkillArtifact $codexCommandPath $ExpectedVersion
            return
        }

        & $codexCommandPath plugin marketplace remove projectatlas --json | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex ProjectAtlas plugin update failed: could not remove stale projectatlas marketplace."
            return
        }
        & $codexCommandPath plugin marketplace add styler-ai/ProjectAtlas --ref $releaseTag --json | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex ProjectAtlas plugin update failed: could not add projectatlas marketplace at $releaseTag."
            Restore-ProjectAtlasCodexMarketplace $codexCommandPath $source $previousRef
            return
        }
        & $codexCommandPath plugin remove projectatlas --marketplace projectatlas --json | Out-Null
        & $codexCommandPath plugin add projectatlas --marketplace projectatlas --json | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex ProjectAtlas plugin update failed: could not install projectatlas plugin at $releaseTag."
            Restore-ProjectAtlasCodexMarketplace $codexCommandPath $source $previousRef
            return
        }
        $installedVersion = Get-ProjectAtlasCodexPluginVersion $codexCommandPath
        if ($installedVersion -ne $runtimeVersion) {
            Write-Warning "Codex ProjectAtlas plugin update failed: installed projectatlas plugin version '$installedVersion' does not match $runtimeVersion."
            Restore-ProjectAtlasCodexMarketplace $codexCommandPath $source $previousRef
            return
        }
        $installedPlugin = Get-ProjectAtlasCodexPlugin $codexCommandPath
        if (-not (Test-ProjectAtlasCodexPluginSourceManifest $installedPlugin $runtimeVersion)) {
            $sourceManifestVersion = Get-ProjectAtlasCodexPluginSourceManifestVersion $installedPlugin
            Write-Warning "Codex ProjectAtlas plugin update failed: source manifest version '$sourceManifestVersion' does not match $runtimeVersion after refresh."
            Restore-ProjectAtlasCodexMarketplace $codexCommandPath $source $previousRef
            return
        }
        Write-Output "Codex ProjectAtlas plugin marketplace updated to $releaseTag."
        Confirm-ProjectAtlasCodexSkillArtifact $codexCommandPath $ExpectedVersion
    }
    catch {
        Write-Warning "Codex ProjectAtlas plugin update failed: $($_.Exception.Message)"
    }
}

function Update-ProjectAtlasCodexMcpRegistry {
    param(
        [string]$VerifiedPath,
        [string]$ExpectedVersion,
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath
    )
    if (Test-Truthy $env:PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE) {
        Write-Output "Codex MCP registry update skipped by PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE."
        return
    }
    $codexCommandPath = Resolve-ProjectAtlasCodexCommand "Codex MCP registry update"
    if (-not $codexCommandPath) {
        return
    }
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    $launchArgs = Get-ProjectAtlasMcpLaunchArguments $DbPath $ProjectConfigPath $FlatConfigPath $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion) -or $launchArgs.Count -eq 0) {
        Write-Output "Codex MCP registry update skipped: ProjectAtlas version is unknown."
        return
    }
    try {
        $existing = & $codexCommandPath mcp get projectatlas 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) {
            Write-Output "Codex MCP registry update skipped: no global projectatlas MCP server is configured."
            return
        }
        $expectedConfigPath = if (Test-Path -LiteralPath $ProjectConfigPath) { $ProjectConfigPath } elseif (Test-Path -LiteralPath $FlatConfigPath) { $FlatConfigPath } else { $null }
        $alreadyCurrent = $existing.Contains($VerifiedPath) -and $existing.Contains($runtimeVersion) -and $existing.Contains($DbPath)
        if ($expectedConfigPath) {
            $alreadyCurrent = $alreadyCurrent -and $existing.Contains($expectedConfigPath)
        }
        if ($alreadyCurrent) {
            Write-Output "Codex MCP registry already points to ProjectAtlas $runtimeVersion for $DbPath."
            return
        }

        & $codexCommandPath mcp remove projectatlas | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex MCP registry update failed: could not remove stale global projectatlas server."
            return
        }
        $addArgs = @("mcp", "add", "projectatlas", "--", $VerifiedPath) + $launchArgs
        & $codexCommandPath @addArgs | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex MCP registry update failed: could not add verified global projectatlas server."
            return
        }
        Write-Output "Codex MCP registry updated to ProjectAtlas runtime $VerifiedPath with database $DbPath."
    }
    catch {
        Write-Warning "Codex MCP registry update failed: $($_.Exception.Message)"
    }
}

function Write-ProjectAtlasWorkflowPinReport {
    param(
        [string]$Root,
        [string]$ExpectedVersion
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        return
    }
    $workflowDir = Join-Path $Root ".github\workflows"
    if (-not (Test-Path -LiteralPath $workflowDir)) {
        return
    }
    $releaseTag = "v$runtimeVersion"
    $rootPath = (Resolve-Path -LiteralPath $Root).Path.TrimEnd('\', '/')
    $workflowFiles = Get-ChildItem -LiteralPath $workflowDir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Extension -eq ".yml" -or $_.Extension -eq ".yaml" }
    foreach ($file in $workflowFiles) {
        $lineNumber = 0
        foreach ($line in Get-Content -LiteralPath $file.FullName) {
            $lineNumber += 1
            if ($line -notmatch 'github\.com/styler-ai/ProjectAtlas/releases/download/') {
                continue
            }
            $pinMatches = [System.Text.RegularExpressions.Regex]::Matches($line, 'v[0-9]+\.[0-9]+\.[0-9]+')
            foreach ($match in $pinMatches) {
                $foundTag = $match.Value
                if ($foundTag -and $foundTag -ne $releaseTag) {
                    $relativePath = $file.FullName
                    if ($relativePath.StartsWith($rootPath, [System.StringComparison]::OrdinalIgnoreCase)) {
                        $relativePath = $relativePath.Substring($rootPath.Length).TrimStart('\', '/')
                    }
                    Write-Warning "Stale ProjectAtlas workflow release pin in ${relativePath}:${lineNumber} uses $foundTag; expected $releaseTag."
                }
            }
        }
    }
}

function Get-ReleaseRuntimeInstallPath {
    param(
        [string]$Version
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $Version
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        $runtimeVersion = "unknown"
    }
    $safeVersion = $runtimeVersion -replace '[^A-Za-z0-9_.-]', '_'
    $installDir = Join-Path $env:LOCALAPPDATA "ProjectAtlas\runtimes\$safeVersion\x86_64-pc-windows-msvc"
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    return Join-Path $installDir "projectatlas.exe"
}

function Get-ProjectAtlasSha256 {
    param(
        [string]$Archive
    )
    $stream = [System.IO.File]::OpenRead($Archive)
    try {
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            $hash = $sha256.ComputeHash($stream)
            return ([System.BitConverter]::ToString($hash) -replace '-', '').ToLowerInvariant()
        }
        finally {
            $sha256.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

function Confirm-ReleaseArchiveChecksum {
    param(
        [string]$Archive,
        [string]$Asset,
        [string]$Version,
        [string]$BaseUrl,
        [string]$TempDir
    )
    $checksums = Join-Path $TempDir "SHA256SUMS"
    Invoke-WebRequest -Uri "$BaseUrl/$Version/SHA256SUMS" -OutFile $checksums
    $expected = $null
    foreach ($line in Get-Content -LiteralPath $checksums) {
        $parts = $line.Trim() -split '\s+'
        if ($parts.Count -ge 2 -and ($parts[1] -eq $Asset -or $parts[1] -eq "./$Asset")) {
            $expected = $parts[0].ToLowerInvariant()
            break
        }
    }
    if ([string]::IsNullOrWhiteSpace($expected)) {
        throw "SHA256SUMS did not contain an entry for $Asset"
    }
    $actual = Get-ProjectAtlasSha256 $Archive
    if ($actual -ne $expected) {
        throw "Checksum mismatch for ${Asset}: expected $expected, found $actual"
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
    $target = Get-ReleaseRuntimeInstallPath $Version
    if (Test-ProjectAtlasRuntime $target $Version) {
        return $target
    }
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("projectatlas-" + [guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
    $archive = Join-Path $tempDir $asset
    try {
        Invoke-WebRequest -Uri $url -OutFile $archive
        Confirm-ReleaseArchiveChecksum $archive $asset $Version $BaseUrl $tempDir
        Expand-Archive -LiteralPath $archive -DestinationPath $tempDir -Force
        $binary = Get-ChildItem -LiteralPath $tempDir -Filter "projectatlas.exe" -Recurse | Select-Object -First 1
        if (-not $binary) {
            throw "Release archive did not contain projectatlas.exe"
        }
        Copy-Item -LiteralPath $binary.FullName -Destination $target -Force
        if (-not (Test-ProjectAtlasRuntime $target $Version)) {
            throw "Release archive produced an invalid runtime for ProjectAtlas ${Version}: $target"
        }
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
$atlasDir = Join-Path $ProjectRoot ".projectatlas"
Assert-ProjectAtlasDirectPath $atlasDir "ProjectAtlas project state directory"
$inheritedProcessPath = $env:Path
$inheritedProjectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue | Select-Object -First 1
$inheritedProjectAtlasPath = if ($inheritedProjectAtlasCommand) { $inheritedProjectAtlasCommand.Source } else { $null }
$futureProcessPathReady = $false

if ($RuntimePath) {
    $projectAtlas = (Resolve-Path $RuntimePath).Path
    if (-not (Test-ProjectAtlasRuntime $projectAtlas $ProjectAtlasVersion)) {
        throw "Provided ProjectAtlas runtime does not satisfy the ProjectAtlas runtime/version contract: $projectAtlas"
    }
    $stableMirrorSynchronized = Sync-ProjectAtlasRuntimeToLocalAppData $projectAtlas $ProjectAtlasVersion
    Set-ProjectAtlasProcessPathPrecedence $projectAtlas
}
else {
    $cargo = Find-Cargo
    $installedBinary = $null

    if ($releaseBinaryOnly) {
        $installedBinary = Install-ReleaseBinary $ProjectAtlasVersion $ReleaseBaseUrl
        if (-not $installedBinary) {
            throw "ProjectAtlas release-binary install was required but failed for $ProjectAtlasVersion."
        }
        if (-not (Test-ProjectAtlasRuntime $installedBinary $ProjectAtlasVersion)) {
            throw "ProjectAtlas release-binary install produced an invalid runtime for ${ProjectAtlasVersion}: $installedBinary"
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
    $stableMirrorSynchronized = Sync-ProjectAtlasRuntimeToLocalAppData $projectAtlas $ProjectAtlasVersion

    Set-ProjectAtlasProcessPathPrecedence $projectAtlas
}
Invoke-Checked $projectAtlas @("--format", "json", "runtime-info") | Out-Null
Confirm-ProjectAtlasBareCommandResolution $projectAtlas $ProjectAtlasVersion
$verifiedRuntimePath = Get-NormalizedPathEntry $projectAtlas
$stableMirrorPath = Get-NormalizedPathEntry (Join-Path $env:LOCALAPPDATA "ProjectAtlas\bin\projectatlas.exe")
Quarantine-ProjectAtlasStaleShims $projectAtlas $ProjectAtlasVersion
if (-not $RuntimePath) {
    $futureProcessPathReady = Set-ProjectAtlasPathPrecedence $projectAtlas
}
Write-ProjectAtlasPathShadowReport $projectAtlas $ProjectAtlasVersion
$effectiveInheritedProjectAtlasPath = $inheritedProjectAtlasPath
if ([string]::IsNullOrWhiteSpace($effectiveInheritedProjectAtlasPath) -or -not (Test-Path -LiteralPath $effectiveInheritedProjectAtlasPath)) {
    $installerProcessPath = $env:Path
    try {
        $env:Path = $inheritedProcessPath
        $effectiveInheritedProjectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue | Select-Object -First 1
        $effectiveInheritedProjectAtlasPath = if ($effectiveInheritedProjectAtlasCommand) { $effectiveInheritedProjectAtlasCommand.Source } else { $null }
    }
    finally {
        $env:Path = $installerProcessPath
    }
}
$inheritedCommandReady = -not [string]::IsNullOrWhiteSpace($effectiveInheritedProjectAtlasPath) `
    -and (Get-NormalizedPathEntry $effectiveInheritedProjectAtlasPath) -eq $verifiedRuntimePath
$inheritedSynchronizedMirrorReady = $stableMirrorSynchronized `
    -and -not [string]::IsNullOrWhiteSpace($effectiveInheritedProjectAtlasPath) `
    -and (Get-NormalizedPathEntry $effectiveInheritedProjectAtlasPath) -eq $stableMirrorPath
$installerProjectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue | Select-Object -First 1
$installerProjectAtlasPath = if ($installerProjectAtlasCommand) { $installerProjectAtlasCommand.Source } else { $null }
$installerCliReady = -not [string]::IsNullOrWhiteSpace($installerProjectAtlasPath) `
    -and (Get-NormalizedPathEntry $installerProjectAtlasPath) -eq $verifiedRuntimePath
$parentCliReady = $inheritedCommandReady -or $inheritedSynchronizedMirrorReady
$hostRestartRequired = -not $parentCliReady -and $futureProcessPathReady
$hostRepairRequired = -not $parentCliReady -and -not $futureProcessPathReady

Assert-ProjectAtlasDirectPath $atlasDir "ProjectAtlas project state directory"
New-Item -ItemType Directory -Force -Path $atlasDir | Out-Null
Assert-ProjectAtlasDirectPath $atlasDir "ProjectAtlas project state directory"
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
    Assert-ProjectAtlasDirectFilePath $OutputPath "ProjectAtlas MCP config output"
    $temporaryOutputPath = Join-Path $atlasDir (".projectatlas-mcp-config-" + [guid]::NewGuid().ToString("N") + ".tmp")
    try {
        [System.IO.File]::WriteAllText($temporaryOutputPath, $mcpConfigText, $utf8NoBom)
        Assert-ProjectAtlasDirectFilePath $OutputPath "ProjectAtlas MCP config output"
        Move-Item -LiteralPath $temporaryOutputPath -Destination $OutputPath -Force
    }
    finally {
        if ([System.IO.File]::Exists($temporaryOutputPath)) {
            [System.IO.File]::Delete($temporaryOutputPath)
        }
    }
}

Write-ProjectAtlasMcpConfig $mcpConfigPath $null
Write-ProjectAtlasMcpConfig $claudeMcpConfigPath "claude-code"
Write-ProjectAtlasMcpConfig $opencodeConfigPath "opencode"
Confirm-ProjectAtlasGeneratedMcpConfig $claudeMcpConfigPath "Claude Code" $projectAtlas $ProjectAtlasVersion $dbPath $projectConfigPath $flatConfigPath $ProjectRoot
Confirm-ProjectAtlasGeneratedMcpConfig $opencodeConfigPath "OpenCode" $projectAtlas $ProjectAtlasVersion $dbPath $projectConfigPath $flatConfigPath $ProjectRoot
Update-ProjectAtlasCodexPlugin $ProjectAtlasVersion
Update-ProjectAtlasCodexMcpRegistry $projectAtlas $ProjectAtlasVersion $dbPath $projectConfigPath $flatConfigPath
Write-ProjectAtlasWorkflowPinReport $ProjectRoot $ProjectAtlasVersion

Write-Output "ProjectAtlas runtime installed and verified: $projectAtlas"
Write-Output "ProjectAtlas update preserved project state under $atlasDir; use reset-index --apply for explicit state cleanup."
Write-Output "Project-local MCP config written: $mcpConfigPath"
Write-Output "Project-local Claude Code MCP config written: $claudeMcpConfigPath"
Write-Output "Project-local OpenCode MCP config written: $opencodeConfigPath"
Write-Output "Claude Code ProjectAtlas integration verified through generated MCP config; restart Claude Code if an older session cached previous instructions."
Write-Output "OpenCode ProjectAtlas integration verified through generated MCP config; restart OpenCode if an older session cached previous instructions."
if ($hostRestartRequired) {
    Write-Warning "Existing host restart required: the inherited bare 'projectatlas' command remains stale, but the verified runtime is first on the persisted fresh-process PATH. Restart the environment-owning Windows launcher or terminal session, then start a new Codex or shell; restarting only a child of an unchanged launcher can retain stale PATH. The runtime and generated MCP configs are already ready through the verified absolute runtime."
}
elseif ($hostRepairRequired) {
    Write-Warning "Existing host bare CLI is not ready, and restart alone will not repair it because this installation could not make the verified runtime the first bare command for a fresh process. Unlock or remove the stale command and rerun this installer, or configure $(Split-Path -Parent $projectAtlas) first on PATH. The runtime and generated MCP configs are ready through the verified absolute runtime."
}
Write-Output "ProjectAtlas readiness: runtime_mcp_configs_ready=true installer_cli_ready=$($installerCliReady.ToString().ToLowerInvariant()) parent_cli_ready=$($parentCliReady.ToString().ToLowerInvariant()) host_restart_required=$($hostRestartRequired.ToString().ToLowerInvariant())"
