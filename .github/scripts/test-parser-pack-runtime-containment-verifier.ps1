[CmdletBinding()]
param(
    [string]$Builder =
        (Join-Path $PSScriptRoot "build-parser-pack-runtime-containment.ps1"),
    [string]$Verifier =
        (Join-Path $PSScriptRoot "verify-parser-pack-runtime-containment.ps1")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

if ($env:OS -ne "Windows_NT" -or -not [System.Environment]::Is64BitProcess) {
    throw "Runtime-containment verifier self-test requires 64-bit Windows."
}

$testBase = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::Combine(
        [System.IO.Path]::GetTempPath(),
        "projectatlas-runtime-containment-verifier-tests"
    )
)
[System.IO.Directory]::CreateDirectory($testBase) | Out-Null
$testRoot = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::Combine($testBase, [Guid]::NewGuid().ToString("N"))
)
$expectedPrefix =
    $testBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar) +
    [System.IO.Path]::DirectorySeparatorChar
if (-not $testRoot.StartsWith($expectedPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Runtime-containment verifier self-test root escaped its temporary base."
}

try {
    $output = [System.IO.Path]::Combine($testRoot, "output")
    $build = [System.IO.Path]::Combine($output, "build", "release")
    $stageA = [System.IO.Path]::Combine($output, "work", "staged-a")
    $stageB = [System.IO.Path]::Combine($output, "work", "staged-b")
    foreach ($directory in @($build, $stageA, $stageB)) {
        [System.IO.Directory]::CreateDirectory($directory) | Out-Null
    }

    $broker = [System.IO.Path]::Combine(
        $build,
        "projectatlas-parser-containment.exe"
    )
    $windowsPowerShell = [System.IO.Path]::Combine(
        $env:SystemRoot,
        "System32",
        "WindowsPowerShell",
        "v1.0",
        "powershell.exe"
    )
    & $windowsPowerShell `
        -NoLogo `
        -NoProfile `
        -NonInteractive `
        -ExecutionPolicy Bypass `
        -File $Builder `
        -OutputPath $broker
    if ($LASTEXITCODE -ne 0) {
        throw "Runtime-containment verifier fixture build failed."
    }

    $brokerItem = Get-Item -LiteralPath $broker -Force
    $brokerDigest = (
        Get-FileHash -Algorithm SHA256 -LiteralPath $brokerItem.FullName
    ).Hash.ToLowerInvariant()
    $buildContract = @(& $brokerItem.FullName --build-contract)
    $buildContractPattern =
        '\Aprojectatlas-parser-containment-build-contract-v1' +
        '\|runtime=(windows-net-framework-clr-v4)' +
        '\|architecture=(x86_64)' +
        '\|modules=(advapi32\.dll,kernel32\.dll,userenv\.dll)' +
        '\|methods=([1-9][0-9]*)' +
        '\|imports_sha256=([0-9a-f]{64})\z'
    if ($LASTEXITCODE -ne 0 -or
        $buildContract.Count -ne 1 -or
        [string]$buildContract[0] -cnotmatch $buildContractPattern) {
        throw "Runtime-containment verifier fixture build contract failed."
    }
    $runtimeFamily = [string]$Matches[1]
    $architecture = [string]$Matches[2]
    $managedModules = @([string]$Matches[3] -split ',')
    $managedImportCount = [int]$Matches[4]
    $managedImportsSha256 = [string]$Matches[5]
    $utf8WithoutBom = [System.Text.UTF8Encoding]::new($false)

    foreach ($stage in @($stageA, $stageB)) {
        [System.IO.File]::Copy(
            $brokerItem.FullName,
            [System.IO.Path]::Combine(
                $stage,
                "projectatlas-parser-containment.exe"
            ),
            $false
        )
        $manifest = [ordered]@{
            platform = "x86_64-pc-windows-msvc"
            files = @(
                [ordered]@{
                    path = "projectatlas-parser-containment.exe"
                    role = [ordered]@{ kind = "containment-broker" }
                    bytes = [long]$brokerItem.Length
                    sha256 = $brokerDigest
                }
            )
        }
        $nativeAudit = [ordered]@{
            containment_broker = [ordered]@{
                file = [ordered]@{
                    path = "projectatlas-parser-containment.exe"
                    sha256 = $brokerDigest
                    byte_length = [long]$brokerItem.Length
                }
                runtime_family = $runtimeFamily
                architecture = $architecture
                managed_modules = $managedModules
                managed_import_count = $managedImportCount
                managed_imports_sha256 = $managedImportsSha256
            }
        }
        [System.IO.File]::WriteAllText(
            [System.IO.Path]::Combine($stage, "artifact-manifest.json"),
            (($manifest | ConvertTo-Json -Depth 8 -Compress) + "`n"),
            $utf8WithoutBom
        )
        [System.IO.File]::WriteAllText(
            [System.IO.Path]::Combine($stage, "native-audit-report.json"),
            (($nativeAudit | ConvertTo-Json -Depth 8 -Compress) + "`n"),
            $utf8WithoutBom
        )
    }

    $success = @(& $Verifier -OutputRoot $output -SelfTestTimeoutSeconds 180)
    if ($success.Count -ne 1 -or
        [string]$success[0] -cne
            "Runtime-containment broker artifact verification passed.") {
        throw "Runtime-containment verifier did not accept one exact bound fixture."
    }

    $tamperedManifestPath = [System.IO.Path]::Combine(
        $stageB,
        "artifact-manifest.json"
    )
    $tamperedManifest = [System.IO.File]::ReadAllText($tamperedManifestPath) |
        ConvertFrom-Json -Depth 8
    $tamperedManifest.files[0].sha256 = "0" * 64
    [System.IO.File]::WriteAllText(
        $tamperedManifestPath,
        (($tamperedManifest | ConvertTo-Json -Depth 8 -Compress) + "`n"),
        $utf8WithoutBom
    )
    $tamperRejected = $false
    try {
        & $Verifier -OutputRoot $output -SelfTestTimeoutSeconds 30 | Out-Null
    }
    catch {
        $tamperRejected = $_.Exception.Message -eq
            "Runtime-containment broker is not bound to the exact staged artifact."
    }
    if (-not $tamperRejected) {
        throw "Runtime-containment verifier accepted one tampered artifact binding."
    }

    Write-Output "Runtime-containment artifact verifier self-test passed."
}
finally {
    if ([System.IO.Directory]::Exists($testRoot)) {
        $resolvedTestRoot = [System.IO.Path]::GetFullPath($testRoot)
        if (-not $resolvedTestRoot.StartsWith(
            $expectedPrefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
            throw "Refused to clean a verifier test root outside its temporary base."
        }
        Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
    }
}
