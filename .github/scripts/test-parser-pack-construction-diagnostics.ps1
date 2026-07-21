[CmdletBinding()]
param(
    [string]$ProductionScript = (Join-Path $PSScriptRoot "run-parser-pack-contained-construction.ps1")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Require {
    param(
        [Parameter(Mandatory = $true)]
        [bool]$Condition,

        [Parameter(Mandatory = $true)]
        [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

$production = Get-Item -LiteralPath $ProductionScript -Force
Require `
    (-not $production.PSIsContainer -and
        (($production.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0)) `
    "Production construction script is not one regular file."
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
    $production.FullName,
    [ref]$tokens,
    [ref]$parseErrors
)
Require ($parseErrors.Count -eq 0) "Production construction script did not parse."
foreach ($name in @(
    "Add-BoundedDiagnosticTail",
    "Invoke-Checked",
    "Write-ConstructionStatus"
)) {
    $definitions = @($ast.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq $name
        },
        $true
    ))
    Require ($definitions.Count -eq 1) "Expected one $name definition."
    Invoke-Expression $definitions[0].Extent.Text
}

$testBase = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::Combine(
        [System.IO.Path]::GetTempPath(),
        "projectatlas-construction-diagnostic-tests"
    )
)
[System.IO.Directory]::CreateDirectory($testBase) | Out-Null
$testRoot = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::Combine($testBase, [guid]::NewGuid().ToString("N"))
)
Require `
    ($testRoot.StartsWith(
        "$($testBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar))$([System.IO.Path]::DirectorySeparatorChar)",
        [System.StringComparison]::OrdinalIgnoreCase
    )) `
    "Diagnostic test root escaped its temporary base."

try {
    $source = [System.IO.Directory]::CreateDirectory(
        [System.IO.Path]::Combine($testRoot, "source")
    ).FullName
    $inputs = [System.IO.Directory]::CreateDirectory(
        [System.IO.Path]::Combine($testRoot, "inputs")
    ).FullName
    $output = [System.IO.Directory]::CreateDirectory(
        [System.IO.Path]::Combine($testRoot, "output")
    ).FullName
    $Target = "x86_64-pc-windows-msvc"
    $commandDiagnosticTailBytes = 24 * 1024
    $constructionDiagnosticMaxBytes = 64 * 1024
    $script:constructionDiagnosticPath = [System.IO.Path]::Combine(
        $output,
        "construction-diagnostic.txt"
    )
    $script:constructionStatusPath = [System.IO.Path]::Combine(
        $output,
        "construction-status.json"
    )
    $script:constructionStage = "optional-parser-worker-build"
    $script:constructionFailureRecorded = $false
    $pwsh = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName

    Invoke-Checked `
        -Executable $pwsh `
        -Arguments @(
            "-NoProfile",
            "-Command",
            '[Console]::Out.Write("success"); [Console]::Error.Write("success-error"); exit 0'
        ) `
        -Role "success self-test"
    Require `
        (-not [System.IO.File]::Exists($script:constructionDiagnosticPath)) `
        "Successful construction wrote a diagnostic."

    $failureCommand = @'
[Console]::Out.Write("stdout-head" + ("o" * 30000) + "stdout-tail")
[Console]::Error.Write("stderr-head" + ("e" * 30000) + "stderr-tail")
exit 7
'@
    $failed = $false
    try {
        Invoke-Checked `
            -Executable $pwsh `
            -Arguments @("-NoProfile", "-Command", $failureCommand) `
            -Role "failure self-test"
    }
    catch {
        $failed = $true
    }
    Require $failed "Failing construction command succeeded."
    $diagnostic = Get-Item -LiteralPath $script:constructionDiagnosticPath -Force
    Require `
        (-not $diagnostic.PSIsContainer -and
            (($diagnostic.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0) -and
            $diagnostic.Length -gt 0 -and
            $diagnostic.Length -le $constructionDiagnosticMaxBytes) `
        "Failure diagnostic was not one bounded regular file."
    $diagnosticText = [System.IO.File]::ReadAllText($diagnostic.FullName)
    Require `
        ($diagnosticText.Contains("stdout-tail") -and
            $diagnosticText.Contains("stderr-tail")) `
        "Failure diagnostic lost a stream tail."
    Require `
        (-not $diagnosticText.Contains("stdout-head") -and
            -not $diagnosticText.Contains("stderr-head")) `
        "Failure diagnostic retained overflowed stream heads."
    $status = [System.IO.File]::ReadAllText($script:constructionStatusPath) |
        ConvertFrom-Json -Depth 4
    Require `
        ($status.stage -eq "optional-parser-worker-build" -and
            $status.state -eq "failed" -and
            $status.exit_code -eq 7) `
        "Failure diagnostic changed the authoritative status record."
    Write-Output "Parser-pack construction diagnostic self-test passed."
}
finally {
    if ([System.IO.Directory]::Exists($testRoot)) {
        $resolvedTestRoot = [System.IO.Path]::GetFullPath($testRoot)
        Require `
            ($resolvedTestRoot.StartsWith(
                "$($testBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar))$([System.IO.Path]::DirectorySeparatorChar)",
                [System.StringComparison]::OrdinalIgnoreCase
            )) `
            "Refused to clean a diagnostic test root outside its temporary base."
        Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
    }
}
