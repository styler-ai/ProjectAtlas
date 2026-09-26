# Read-only session check. The installer, not a hook, owns runtime and MCP changes.
$ErrorActionPreference = 'Stop'
$pluginRoot = Split-Path -Parent $PSScriptRoot
$skill = Join-Path $pluginRoot 'skills/projectatlas/SKILL.md'
$expected = (Get-Content -Raw -LiteralPath (Join-Path $pluginRoot '.codex-plugin/plugin.json') | ConvertFrom-Json).version
$reason = 'runtime unavailable or not version-matched'
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
Get-Content -LiteralPath (Join-Path $pluginRoot 'hooks/agent-instructions.txt')
Write-Output "Read the complete installed ProjectAtlas skill now: $skill"
if (-not (Test-Path -LiteralPath $skill -PathType Leaf)) {
    Write-Output 'ProjectAtlas integration incomplete: bundled skill is missing; reinstall the version-matched plugin before Atlas use.'
    exit 0
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
try {
    $runtime = & projectatlas --format json runtime-info 2>$null | ConvertFrom-Json
    if ($runtime.project -ceq 'ProjectAtlas' -and $runtime.version -ceq $expected -and $runtime.executable) {
        $reason = 'project database or generated host config unavailable'
        $atlasDir = Join-Path $projectRoot '.projectatlas'
        $hostConfig = Join-Path $atlasDir 'projectatlas.mcp.json'
        if (Test-Path -LiteralPath $db -PathType Leaf) {
            if (Test-Path -LiteralPath $hostConfig -PathType Leaf) {
                $verifyArgs = @('--db', $db)
                if ($config) { $verifyArgs += @('--config', $config) }
                $verifyArgs += @('--format', 'json', 'root', 'verify', '--binding-only', '--project-root', $projectRoot)
                $reason = 'project database is incompatible or bound to another root'
                & projectatlas @verifyArgs 2>$null | Out-Null
                if ($LASTEXITCODE -ne 0) {
                    throw 'not ready'
                }
                $reason = 'Codex MCP or generated config does not match this project/runtime'
                $registry = & codex mcp get projectatlas --json 2>$null | ConvertFrom-Json
                $generated = Get-Content -Raw -LiteralPath $hostConfig | ConvertFrom-Json
                $argsExpected = @('--require-version', $expected, '--db', $db)
                if ($config) { $argsExpected += @('--config', $config) }
                $argsExpected += 'mcp'
                $samePath = { param($actual, $wanted) $actual -and
                    [IO.Path]::GetFullPath([string]$actual) -ieq [IO.Path]::GetFullPath([string]$wanted) }
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
                if ($registry.enabled -is [bool] -and $registry.enabled -and $registry.transport.type -ceq 'stdio' -and
                    (& $samePath $registry.transport.command $runtime.executable) -and
                    (& $sameArgs $registry.transport.args) -and
                    (& $samePath $generated.mcpServers.projectatlas.command $runtime.executable) -and
                    (& $sameArgs $generated.mcpServers.projectatlas.args) -and
                    (& $samePath $generated.mcpServers.projectatlas.cwd $projectRoot)) {
                    Write-Output "ProjectAtlas integration ready: plugin, direct CLI, generated config, and Codex MCP match $expected for this project. Use the version-matched ProjectAtlas skill and repository instructions."
                    exit 0
                }
            }
        }
    }
} catch {
    # Missing commands and malformed records are incomplete, never ready.
}

Write-Output "ProjectAtlas integration incomplete: $reason. Plugin installation alone does not update the native runtime or MCP registry."
function IdentityValue($value) {
    if ($null -eq $value -or [string]::IsNullOrWhiteSpace([string]$value)) { return 'unavailable' }
    if ($value -is [bool]) { return $value.ToString().ToLowerInvariant() }
    return [string]$value
}
$registryArgs = @($registry.transport.args)
$generatedArgs = @($generated.mcpServers.projectatlas.args)
Write-Output ('Expected: plugin_version={0} project_db={1} project_config={2} project_root={3} codex_mcp_enabled=true codex_mcp_transport=stdio' -f $expected, $db, (IdentityValue $config), $projectRoot)
Write-Output ('Observed: direct_cli_version={0} direct_cli_executable={1}; codex_mcp_version={2} codex_mcp_executable={3} codex_mcp_db={4} codex_mcp_config={5} codex_mcp_enabled={6} codex_mcp_transport={7}; generated_mcp_version={8} generated_mcp_executable={9} generated_mcp_db={10} generated_mcp_config={11} generated_mcp_cwd={12}' -f
    (IdentityValue $runtime.version), (IdentityValue $runtime.executable),
    (IdentityValue $registryArgs[1]), (IdentityValue $registry.transport.command), (IdentityValue $registryArgs[3]), (IdentityValue $registryArgs[5]), (IdentityValue $registry.enabled), (IdentityValue $registry.transport.type),
    (IdentityValue $generatedArgs[1]), (IdentityValue $generated.mcpServers.projectatlas.command), (IdentityValue $generatedArgs[3]), (IdentityValue $generatedArgs[5]), (IdentityValue $generated.mcpServers.projectatlas.cwd))
Write-Output 'Use the version-matched ProjectAtlas skill. Do not reset a database.'
Write-Output 'Repair command:'
function Quote-PowerShellLiteral([string]$value) { return "'" + $value.Replace("'", "''") + "'" }
Write-Output ('& {0} -ProjectRoot {1} -ProjectAtlasVersion {2}' -f (Quote-PowerShellLiteral (Join-Path $pluginRoot 'scripts/install-runtime.ps1')), (Quote-PowerShellLiteral $projectRoot), (Quote-PowerShellLiteral "v$expected"))
Write-Output 'Verification commands:'
Write-Output 'projectatlas --format json runtime-info'
Write-Output 'codex mcp get projectatlas --json'
