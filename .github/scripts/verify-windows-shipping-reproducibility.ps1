param(
    [Parameter(Mandatory = $true)][string] $FirstTarget,
    [Parameter(Mandatory = $true)][string] $SecondTarget,
    [Parameter(Mandatory = $true)][string] $ProofRoot
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not $IsWindows) {
    throw "Windows shipping reproducibility requires a native Windows host."
}

$repository = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$FirstTarget = [System.IO.Path]::GetFullPath($FirstTarget)
$SecondTarget = [System.IO.Path]::GetFullPath($SecondTarget)
$ProofRoot = [System.IO.Path]::GetFullPath($ProofRoot)
if ($FirstTarget.Equals($SecondTarget, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Independent shipping constructions require different target directories."
}
foreach ($path in @($FirstTarget, $SecondTarget, $ProofRoot)) {
    if (Test-Path -LiteralPath $path) {
        throw "Shipping reproducibility requires a fresh owned path: $path"
    }
}

New-Item -ItemType Directory -Path $ProofRoot | Out-Null
$reportPath = Join-Path $ProofRoot "reproducibility.json"
$report = [ordered]@{
    schema = 1
    state = "constructing"
    target = "x86_64-pc-windows-msvc"
    source_revision = $null
    cargo_config_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $repository ".cargo/config.toml")).Hash.ToLowerInvariant()
    cargo_arguments = @("build", "--locked", "-p", "projectatlas-cli", "--release")
    first = $null
    second = $null
    matched = $false
}

function Save-Report {
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $reportPath -Encoding utf8NoBOM
}

Push-Location $repository
try {
    $report.source_revision = (& git rev-parse HEAD | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Could not resolve the shipping source revision." }
    Save-Report
    foreach ($role in @("first", "second")) {
        $targetPath = if ($role -eq "first") { $FirstTarget } else { $SecondTarget }
        & cargo build --locked -p projectatlas-cli --release --target-dir $targetPath
        if ($LASTEXITCODE -ne 0) { throw "The $role independent shipping construction failed." }
        $runtime = Join-Path $targetPath "release/projectatlas.exe"
        $report[$role] = [ordered]@{
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $runtime).Hash.ToLowerInvariant()
            bytes = (Get-Item -LiteralPath $runtime).Length
        }
        Save-Report
    }
    if ($report.first.sha256 -cne $report.second.sha256 -or $report.first.bytes -ne $report.second.bytes) {
        Copy-Item -LiteralPath (Join-Path $FirstTarget "release/projectatlas.exe") -Destination (Join-Path $ProofRoot "first-projectatlas.exe")
        Copy-Item -LiteralPath (Join-Path $SecondTarget "release/projectatlas.exe") -Destination (Join-Path $ProofRoot "second-projectatlas.exe")
        throw "Independent Windows shipping executables differ; both original bytes and identities were retained."
    }
    $report.matched = $true
    $report.state = "passed"
    Write-Output "Independent Windows shipping SHA256=$($report.first.sha256), bytes=$($report.first.bytes)"
}
finally {
    if (-not $report.matched) { $report.state = "failed" }
    Save-Report
    Pop-Location
}
