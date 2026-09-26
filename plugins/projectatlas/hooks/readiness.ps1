# Read-only session check. The installer, not a hook, owns runtime and MCP changes.
$ErrorActionPreference = 'Stop'
$pluginRoot = Split-Path -Parent $PSScriptRoot
$expected = (Get-Content -Raw -LiteralPath (Join-Path $pluginRoot '.codex-plugin/plugin.json') | ConvertFrom-Json).version
$reason = 'runtime unavailable or not version-matched'
$projectRoot = (Get-Location).Path
while (-not (Test-Path -LiteralPath (Join-Path $projectRoot '.projectatlas/projectatlas.db') -PathType Leaf)) {
    if ((Test-Path -LiteralPath (Join-Path $projectRoot '.git')) -or
        (Test-Path -LiteralPath (Join-Path $projectRoot '.projectatlas') -PathType Container)) { break }
    $parent = Split-Path -Parent $projectRoot
    if (-not $parent -or $parent -eq $projectRoot) { break }
    $projectRoot = $parent
}
Get-Content -LiteralPath (Join-Path $pluginRoot 'hooks/agent-instructions.txt')

try {
    $runtime = & projectatlas --format json runtime-info 2>$null | ConvertFrom-Json
    if ($runtime.project -eq 'ProjectAtlas' -and $runtime.version -ceq $expected -and $runtime.executable) {
        $reason = 'project database or generated host config unavailable'
        $atlasDir = Join-Path $projectRoot '.projectatlas'
        $db = Join-Path $atlasDir 'projectatlas.db'
        $config = Join-Path $atlasDir 'config.toml'
        $hostConfig = Join-Path $atlasDir 'projectatlas.mcp.json'
        if (Test-Path -LiteralPath $db -PathType Leaf) {
            if (-not (Test-Path -LiteralPath $config -PathType Leaf)) { $config = $null }
            if (Test-Path -LiteralPath $hostConfig -PathType Leaf) {
                $verifyArgs = @('--db', $db)
                if ($config) { $verifyArgs += @('--config', $config) }
                $verifyArgs += @('--format', 'json', 'root', 'verify')
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
                $sameArgs = { param($actual) @($actual).Count -eq $argsExpected.Count -and
                    -not (Compare-Object -CaseSensitive -ReferenceObject $argsExpected -DifferenceObject @($actual) -SyncWindow 0) }
                $samePath = { param($actual, $wanted) $actual -and
                    [IO.Path]::GetFullPath([string]$actual) -ieq [IO.Path]::GetFullPath([string]$wanted) }
                if ($registry.enabled -eq $true -and $registry.transport.type -eq 'stdio' -and
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
Write-Output 'Use the version-matched ProjectAtlas skill. Do not reset a database.'
Write-Output 'Repair command:'
function Quote-PowerShellLiteral([string]$value) { return "'" + $value.Replace("'", "''") + "'" }
Write-Output ('& {0} -ProjectRoot {1} -ProjectAtlasVersion {2}' -f (Quote-PowerShellLiteral (Join-Path $pluginRoot 'scripts/install-runtime.ps1')), (Quote-PowerShellLiteral $projectRoot), (Quote-PowerShellLiteral "v$expected"))
Write-Output 'Verification commands:'
Write-Output 'projectatlas --format json runtime-info'
Write-Output 'codex mcp get projectatlas --json'
