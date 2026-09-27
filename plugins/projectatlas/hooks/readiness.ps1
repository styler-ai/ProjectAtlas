# Read-only session check. The installer, not a hook, owns runtime and MCP changes.
$ErrorActionPreference = 'Stop'
$pluginRoot = Split-Path -Parent $PSScriptRoot
$skill = Join-Path $pluginRoot 'skills/projectatlas/SKILL.md'
$languageSupport = Join-Path $pluginRoot 'skills/projectatlas/references/language-support.md'
$shortCli = Join-Path $pluginRoot 'skills/projectatlas/references/short-cli.md'
$expected = $null
try {
    $manifestPath = Join-Path $pluginRoot '.codex-plugin/plugin.json'
    if ((Get-Item -LiteralPath $manifestPath).Length -le 65536) {
        $manifestText = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath
        if ($manifestText -cmatch '^[ \t\r\n]*\{') {
            $manifest = $manifestText | ConvertFrom-Json
            if ($manifest -is [pscustomobject] -and $manifest.name -ceq 'projectatlas' -and
                $manifest.skills -is [string] -and $manifest.skills -ceq './skills/' -and
                $manifest.version -is [string] -and $manifest.version -and
                $manifest.version -cnotmatch '[^0-9A-Za-z.+-]') {
                $expected = $manifest.version
            }
        }
    }
} catch {
    # A partial plugin update is incomplete, not a hook startup failure.
}
$reason = 'installer readiness receipt or host files changed; rerun the installer'
function IdentityValue($value) {
    if ($null -eq $value -or [string]::IsNullOrWhiteSpace([string]$value)) { return 'unavailable' }
    if ($value -is [bool]) { return $value.ToString().ToLowerInvariant() }
    return [regex]::Replace([string]$value, '[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]', '?')
}
$startingRoot = (Get-Location).Path
$projectRoot = $startingRoot
$homeRoot = if ($env:USERPROFILE) { [IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\') } else { $null }
while ($true) {
    if ($projectRoot -ne $startingRoot -and $homeRoot -and
        [IO.Path]::GetFullPath($projectRoot).TrimEnd('\') -ieq $homeRoot) {
        $projectRoot = $startingRoot
        break
    }
    if (Test-Path -LiteralPath (Join-Path $projectRoot '.projectatlas/projectatlas.db') -PathType Leaf) { break }
    if ((Test-Path -LiteralPath (Join-Path $projectRoot '.git')) -or
        (Test-Path -LiteralPath (Join-Path $projectRoot '.projectatlas') -PathType Container) -or
        (Test-Path -LiteralPath (Join-Path $projectRoot 'projectatlas.toml') -PathType Leaf)) { break }
    $parent = Split-Path -Parent $projectRoot
    if (-not $parent -or $parent -eq $projectRoot) { break }
    $projectRoot = $parent
}
try {
    $guidancePath = Join-Path $pluginRoot 'hooks/agent-instructions.txt'
    $guidanceFile = Get-Item -LiteralPath $guidancePath -ErrorAction Stop
    if ($guidanceFile.Length -eq 0 -or $guidanceFile.Length -gt 65536) { throw 'invalid guidance size' }
    $guidance = Get-Content -Raw -Encoding UTF8 -LiteralPath $guidancePath -ErrorAction Stop
    if ([string]::IsNullOrWhiteSpace($guidance)) { throw 'empty guidance' }
} catch {
    Write-Output 'ProjectAtlas integration incomplete: bundled agent instructions are missing or invalid; reinstall the version-matched plugin before Atlas use.'
    exit 0
}
foreach ($skillAsset in @($skill, $languageSupport, $shortCli)) {
    try {
        $assetFile = Get-Item -LiteralPath $skillAsset -ErrorAction Stop
        $assetLimit = if ($skillAsset -eq $languageSupport) { 1048576 } else { 65536 }
        if ($assetFile.Length -eq 0 -or $assetFile.Length -gt $assetLimit) { throw 'invalid skill asset size' }
        [void](Get-Content -Raw -Encoding UTF8 -LiteralPath $skillAsset -ErrorAction Stop)
    } catch {
        Write-Output 'ProjectAtlas integration incomplete: bundled skill guidance is missing or invalid; reinstall the version-matched plugin before Atlas use.'
        exit 0
    }
}

if ($projectRoot -ieq [IO.Path]::GetPathRoot($projectRoot) -or
    -not (Test-Path -LiteralPath (Join-Path $projectRoot '.git')) -and
    -not (Test-Path -LiteralPath (Join-Path $projectRoot '.projectatlas') -PathType Container) -and
    -not (Test-Path -LiteralPath (Join-Path $projectRoot 'projectatlas.toml') -PathType Leaf)) {
    Write-Output 'ProjectAtlas integration incomplete: no project root was identified. Select the intended project directory before running the version-matched installer with an explicit project root.'
    exit 0
}

$db = Join-Path $projectRoot '.projectatlas/projectatlas.db'
$config = Join-Path $projectRoot '.projectatlas/config.toml'
if (-not (Test-Path -LiteralPath $config -PathType Leaf)) {
    $config = Join-Path $projectRoot 'projectatlas.toml'
    if (-not (Test-Path -LiteralPath $config -PathType Leaf)) { $config = $null }
}
$runtime = $null
$registry = $null
$generated = $null
$atlasDir = Join-Path $projectRoot '.projectatlas'
$hostConfig = Join-Path $atlasDir 'projectatlas.mcp.json'
$stateBase = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { $env:USERPROFILE }
$receiptPath = if ($stateBase) { Join-Path $stateBase 'ProjectAtlas/state/codex-readiness.json' } else { $null }
$codexConfig = if ($env:CODEX_HOME) { Join-Path $env:CODEX_HOME 'config.toml' } elseif ($env:USERPROFILE) { Join-Path $env:USERPROFILE '.codex/config.toml' } else { $null }
function FileSha256([string]$path) {
    $stream = [IO.File]::OpenRead($path)
    $hash = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($hash.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $hash.Dispose(); $stream.Dispose() }
}
$argsExpected = @('--require-version', $expected, '--db', $db)
if ($config) { $argsExpected += @('--config', $config) }
$argsExpected += 'mcp'
$samePath = {
    param($actual, $wanted)
    $actual -and [IO.Path]::IsPathRooted([string]$actual) -and
    [string]$actual -notmatch '^[A-Za-z]:[^\\/]' -and
    [IO.Path]::GetFullPath([string]$actual) -ieq [IO.Path]::GetFullPath([string]$wanted)
}
$sameArgs = {
    param($actual)
    $actualArgs = @($actual)
    if ($actualArgs.Count -ne $argsExpected.Count) { return $false }
    for ($i = 0; $i -lt $argsExpected.Count; $i++) {
        if (($i -eq 3) -or (($i -eq 5) -and $config)) {
            if (-not (& $samePath $actualArgs[$i] $argsExpected[$i])) { return $false }
        } elseif ([string]$actualArgs[$i] -cne [string]$argsExpected[$i]) {
            return $false
        }
    }
    return $true
}
$bindingReady = {
    param($registered, $configured)
    $registered.name -ceq 'projectatlas' -and
    $registered.enabled -is [bool] -and $registered.enabled -and
    $registered.transport.type -ceq 'stdio' -and
    (& $samePath $receipt.runtime $registered.transport.command) -and
    (& $sameArgs $registered.transport.args) -and
    (& $samePath $configured.mcpServers.projectatlas.command $receipt.runtime) -and
    (& $sameArgs $configured.mcpServers.projectatlas.args) -and
    (& $samePath $configured.mcpServers.projectatlas.cwd $projectRoot)
}
try {
    if (-not $expected) {
        $reason = 'bundled plugin manifest is missing or invalid'
        throw 'not ready'
    }
    $directCommand = Get-Command projectatlas -ErrorAction SilentlyContinue
    $directPath = if ($directCommand -and $directCommand.CommandType -eq 'Application') { $directCommand.Source } else { $null }
    $runtime = [pscustomobject]@{ version = $null; executable = $directPath }
    $cursor = $startingRoot
    while ($true) {
        if (Test-Path -LiteralPath (Join-Path $cursor '.codex/config.toml') -PathType Leaf) {
            throw 'project Codex config may override the global MCP binding'
        }
        if ($cursor -ieq $projectRoot) { break }
        $cursor = Split-Path -Parent $cursor
    }
    $receiptDir = Get-Item -Force -LiteralPath (Split-Path -Parent $receiptPath) -ErrorAction Stop
    $receiptFile = Get-Item -Force -LiteralPath $receiptPath -ErrorAction Stop
    if (($receiptDir.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
        $receiptFile.Length -gt 65536 -or
        (($receiptFile.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) -or
        ($receiptFile.PSObject.Properties.Name -contains 'LinkType' -and $receiptFile.LinkType -eq 'HardLink')) {
        throw 'untrusted readiness state'
    }
    $receipt = Get-Content -Raw -Encoding UTF8 -LiteralPath $receiptPath | ConvertFrom-Json
    if ((Get-Item -LiteralPath $codexConfig).Length -gt 1048576 -or
        (Get-Item -LiteralPath $hostConfig).Length -gt 1048576) {
        throw 'oversized host config'
    }
    if ($receipt.version -cne $expected -or
        -not (& $samePath $receipt.project_root $projectRoot) -or
        (-not (& $samePath $receipt.runtime $directPath) -and
         -not (& $samePath $receipt.direct_cli $directPath)) -or
        -not (& $samePath $receipt.codex_config $codexConfig) -or
        $receipt.runtime_sha256 -cne (FileSha256 $receipt.runtime) -or
        $receipt.direct_cli_sha256 -cne $receipt.runtime_sha256 -or
        $receipt.runtime_sha256 -cne (FileSha256 $directPath) -or
        $receipt.codex_config_sha256 -cne (FileSha256 $codexConfig) -or
        $receipt.generated_sha256 -cne (FileSha256 $hostConfig) -or
        $receipt.agent_guidance_sha256 -cne (FileSha256 $guidancePath) -or
        $receipt.skill_sha256 -cne (FileSha256 $skill) -or
        $receipt.language_support_sha256 -cne (FileSha256 $languageSupport) -or
        $receipt.short_cli_sha256 -cne (FileSha256 $shortCli)) {
        $reason = 'installer readiness receipt or host files changed; rerun the installer'
        throw 'not ready'
    }
    $registry = $receipt.registry
    if (-not (Test-Path -LiteralPath $db -PathType Leaf) -or
        -not (Test-Path -LiteralPath $hostConfig -PathType Leaf)) {
        $reason = 'project database or generated host config unavailable'
        throw 'not ready'
    }
    $generated = Get-Content -Raw -Encoding UTF8 -LiteralPath $hostConfig | ConvertFrom-Json
    if (& $bindingReady $registry $generated) {
        $runtime = & $directPath --format json runtime-info 2>$null | ConvertFrom-Json
    }
    else {
        $reason = 'direct CLI, Codex MCP, or generated config does not match this project/runtime'
        throw 'not ready'
    }
    if ($runtime.project -ceq 'ProjectAtlas' -and $runtime.version -ceq $expected -and
        (& $samePath $runtime.executable $directPath)) {
        $reason = 'project database is incompatible or bound to another root'
                $verifyArgs = @('--db', $db)
                if ($config) { $verifyArgs += @('--config', $config) }
                $verifyArgs += @('--format', 'json', 'root', 'verify', '--binding-only', '--project-root', $projectRoot)
                & $directPath @verifyArgs 2>$null | Out-Null
                if ($LASTEXITCODE -ne 0) {
                    throw 'not ready'
                }
                $reason = 'Codex MCP or generated config does not match this project/runtime'
                $generated = Get-Content -Raw -Encoding UTF8 -LiteralPath $hostConfig | ConvertFrom-Json
                if ($receipt.runtime_sha256 -ceq (FileSha256 $receipt.runtime) -and
                    $receipt.direct_cli_sha256 -ceq (FileSha256 $directPath) -and
                    $receipt.codex_config_sha256 -ceq (FileSha256 $codexConfig) -and
                    $receipt.generated_sha256 -ceq (FileSha256 $hostConfig) -and
                    $receipt.agent_guidance_sha256 -ceq (FileSha256 $guidancePath) -and
                    $receipt.skill_sha256 -ceq (FileSha256 $skill) -and
                    $receipt.language_support_sha256 -ceq (FileSha256 $languageSupport) -and
                    $receipt.short_cli_sha256 -ceq (FileSha256 $shortCli) -and
                    (& $bindingReady $registry $generated)) {
                    Write-Output $guidance.TrimEnd()
                    Write-Output ('Read the complete installed ProjectAtlas skill now: {0}' -f (IdentityValue $skill))
                    Write-Output ('ProjectAtlas integration ready: plugin, direct CLI, generated config, and Codex MCP match {0} for this project. Use the version-matched ProjectAtlas skill and repository instructions.' -f (IdentityValue $expected))
                    exit 0
                }
    }
} catch {
    # Missing commands and malformed records are incomplete, never ready.
}

Write-Output "ProjectAtlas integration incomplete: $reason. Plugin installation alone does not update the native runtime or MCP registry."
$registryArgs = @($registry.transport.args)
$generatedArgs = @($generated.mcpServers.projectatlas.args)
Write-Output ('Expected: plugin_version={0} project_db={1} project_config={2} project_root={3} codex_mcp_enabled=true codex_mcp_transport=stdio' -f (IdentityValue $expected), (IdentityValue $db), (IdentityValue $config), (IdentityValue $projectRoot))
Write-Output ('Observed: direct_cli_version={0} direct_cli_executable={1}; codex_mcp_version={2} codex_mcp_executable={3} codex_mcp_db={4} codex_mcp_config={5} codex_mcp_enabled={6} codex_mcp_transport={7}; generated_mcp_version={8} generated_mcp_executable={9} generated_mcp_db={10} generated_mcp_config={11} generated_mcp_cwd={12}' -f
    (IdentityValue $runtime.version), (IdentityValue $runtime.executable),
    (IdentityValue $registryArgs[1]), (IdentityValue $registry.transport.command), (IdentityValue $registryArgs[3]), (IdentityValue $registryArgs[5]), (IdentityValue $registry.enabled), (IdentityValue $registry.transport.type),
    (IdentityValue $generatedArgs[1]), (IdentityValue $generated.mcpServers.projectatlas.command), (IdentityValue $generatedArgs[3]), (IdentityValue $generatedArgs[5]), (IdentityValue $generated.mcpServers.projectatlas.cwd))
Write-Output 'Use the version-matched ProjectAtlas skill. Do not reset a database.'
function Quote-PowerShellLiteral([string]$value) { return "'" + $value.Replace("'", "''") + "'" }
$installerPath = Join-Path $pluginRoot 'scripts/install-runtime.ps1'
if (-not $expected) {
    Write-Output 'Repair command unavailable: reinstall the version-matched ProjectAtlas plugin, then run its packaged installer for the selected project.'
} elseif ((IdentityValue $installerPath) -cne $installerPath -or
    (IdentityValue $projectRoot) -cne $projectRoot -or
    (IdentityValue $expected) -cne $expected) {
    Write-Output 'Repair command unavailable: a path contains control characters; select the exact project root manually when invoking the version-matched installer.'
} else {
    Write-Output 'Repair command:'
    Write-Output ('& {0} -ProjectRoot {1} -ProjectAtlasVersion {2}' -f (Quote-PowerShellLiteral $installerPath), (Quote-PowerShellLiteral $projectRoot), (Quote-PowerShellLiteral "v$expected"))
}
Write-Output 'Verification commands:'
Write-Output 'projectatlas --format json runtime-info'
Write-Output 'codex mcp get projectatlas --json'
