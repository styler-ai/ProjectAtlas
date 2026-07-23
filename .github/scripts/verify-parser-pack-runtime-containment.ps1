[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$OutputRoot,

    [ValidateRange(30, 300)]
    [int]$SelfTestTimeoutSeconds = 180
)

# The contained construction principal intentionally has no loaded Windows profile. Compile and
# artifact-audit the broker there, then run its profile-owning self-test only after that principal,
# firewall rule, ACL grants, and journal have been removed. This verifier binds the tested bytes
# back to both deterministic assembly copies and gives the trusted broker no repository path or
# inherited environment while it exercises its bounded temporary profile and Job-object contract.
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$brokerFileName = "projectatlas-parser-containment.exe"
$artifactManifestFileName = "artifact-manifest.json"
$nativeAuditFileName = "native-audit-report.json"
$maximumManifestBytes = 4 * 1024 * 1024
$maximumCommandOutputBytes = 4 * 1024
$contractPattern =
    '\Aprojectatlas-parser-containment-build-contract-v1' +
    '\|runtime=(windows-net-framework-clr-v4)' +
    '\|architecture=(x86_64)' +
    '\|modules=(advapi32\.dll,kernel32\.dll,userenv\.dll)' +
    '\|methods=([1-9][0-9]*)' +
    '\|imports_sha256=([0-9a-f]{64})\z'

function Get-DirectOutputItem {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.DirectoryInfo]$Root,

        [Parameter(Mandatory = $true)]
        [ValidateNotNullOrEmpty()]
        [string]$RelativePath,

        [Parameter(Mandatory = $true)]
        [ValidateSet("File", "Directory")]
        [string]$Kind
    )

    if ([System.IO.Path]::IsPathRooted($RelativePath)) {
        throw "Runtime-containment verification path must be output-relative."
    }
    $segments = @($RelativePath -split '[\\/]' | Where-Object { $_.Length -gt 0 })
    if ($segments.Count -eq 0 -or
        @($segments | Where-Object { $_ -in @(".", "..") }).Count -ne 0) {
        throw "Runtime-containment verification path is invalid."
    }
    $current = $Root.FullName
    $item = $null
    foreach ($segment in $segments) {
        $current = [System.IO.Path]::Combine($current, $segment)
        $item = Get-Item -LiteralPath $current -Force
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Runtime-containment verification path traversed a reparse point."
        }
    }
    $expected = [System.IO.Path]::GetFullPath(
        [System.IO.Path]::Combine($Root.FullName, $RelativePath)
    )
    if ($null -eq $item -or
        -not $item.FullName.Equals($expected, [System.StringComparison]::OrdinalIgnoreCase) -or
        ($Kind -eq "Directory" -and -not $item.PSIsContainer) -or
        ($Kind -eq "File" -and ($item.PSIsContainer -or $item.Length -le 0))) {
        throw "Runtime-containment verification item has the wrong path or kind."
    }
    return $item
}

function Read-BoundedJson {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.FileInfo]$File
    )

    if ($File.Length -gt $maximumManifestBytes) {
        throw "Runtime-containment verification JSON exceeded its byte bound."
    }
    return [System.IO.File]::ReadAllText($File.FullName) | ConvertFrom-Json -Depth 32
}

function Stop-BoundedProcess {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process
    )

    if ($Process.HasExited) {
        return
    }
    try {
        $Process.Kill($true)
    }
    catch {
        $Process.Kill()
    }
    if (-not $Process.WaitForExit(5000)) {
        throw "Runtime-containment verification process could not be reaped."
    }
}

function Invoke-BoundedBrokerCommand {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.FileInfo]$Broker,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,

        [Parameter(Mandatory = $true)]
        [System.IO.DirectoryInfo]$WorkingDirectory,

        [Parameter(Mandatory = $true)]
        [ValidateRange(1, 300)]
        [int]$TimeoutSeconds
    )

    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Broker.FullName
    $start.WorkingDirectory = $WorkingDirectory.FullName
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $start.ArgumentList.Add($argument)
    }
    $start.Environment.Clear()
    foreach ($name in @("SystemRoot", "WINDIR", "LOCALAPPDATA", "USERPROFILE")) {
        $value = [string][System.Environment]::GetEnvironmentVariable($name)
        if ([string]::IsNullOrWhiteSpace($value) -or $value -match "`0|`r|`n") {
            throw "Runtime-containment verification requires one safe profile environment."
        }
        $start.Environment.Add($name, $value)
    }
    $start.Environment.Add("TEMP", $WorkingDirectory.FullName)
    $start.Environment.Add("TMP", $WorkingDirectory.FullName)

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $start
    $stdout = [System.IO.MemoryStream]::new()
    $stderr = [System.IO.MemoryStream]::new()
    $operationFailure = $null
    $commandResult = $null
    $processStarted = $false
    try {
        if (-not $process.Start()) {
            throw "Runtime-containment verification process did not start."
        }
        $processStarted = $true
        $process.StandardInput.Close()
        $stdoutBuffer = [byte[]]::new(1024)
        $stderrBuffer = [byte[]]::new(1024)
        $stdoutRead = $process.StandardOutput.BaseStream.ReadAsync(
            $stdoutBuffer,
            0,
            $stdoutBuffer.Length
        )
        $stderrRead = $process.StandardError.BaseStream.ReadAsync(
            $stderrBuffer,
            0,
            $stderrBuffer.Length
        )
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        while ($null -ne $stdoutRead -or $null -ne $stderrRead) {
            if ([DateTime]::UtcNow -ge $deadline) {
                Stop-BoundedProcess -Process $process
                throw "Runtime-containment verification process exceeded its deadline."
            }
            $readCompleted = $false
            if ($null -ne $stdoutRead -and $stdoutRead.IsCompleted) {
                $count = $stdoutRead.GetAwaiter().GetResult()
                if ($count -eq 0) {
                    $stdoutRead = $null
                }
                else {
                    if ($stdout.Length + $count -gt $maximumCommandOutputBytes) {
                        Stop-BoundedProcess -Process $process
                        throw "Runtime-containment verification stdout exceeded its byte bound."
                    }
                    $stdout.Write($stdoutBuffer, 0, $count)
                    $stdoutRead = $process.StandardOutput.BaseStream.ReadAsync(
                        $stdoutBuffer,
                        0,
                        $stdoutBuffer.Length
                    )
                }
                $readCompleted = $true
            }
            if ($null -ne $stderrRead -and $stderrRead.IsCompleted) {
                $count = $stderrRead.GetAwaiter().GetResult()
                if ($count -eq 0) {
                    $stderrRead = $null
                }
                else {
                    if ($stderr.Length + $count -gt $maximumCommandOutputBytes) {
                        Stop-BoundedProcess -Process $process
                        throw "Runtime-containment verification stderr exceeded its byte bound."
                    }
                    $stderr.Write($stderrBuffer, 0, $count)
                    $stderrRead = $process.StandardError.BaseStream.ReadAsync(
                        $stderrBuffer,
                        0,
                        $stderrBuffer.Length
                    )
                }
                $readCompleted = $true
            }
            if (-not $readCompleted) {
                Start-Sleep -Milliseconds 5
            }
        }
        $remaining = [Math]::Max(
            1,
            [Math]::Min(
                [int]::MaxValue,
                [int](($deadline - [DateTime]::UtcNow).TotalMilliseconds)
            )
        )
        if (-not $process.WaitForExit($remaining)) {
            Stop-BoundedProcess -Process $process
            throw "Runtime-containment verification process exceeded its deadline."
        }
        $commandResult = [pscustomobject]@{
            ExitCode = $process.ExitCode
            StandardOutput = [System.Text.Encoding]::ASCII.GetString($stdout.ToArray())
            StandardError = [System.Text.Encoding]::ASCII.GetString($stderr.ToArray())
        }
    }
    catch {
        $operationFailure = $_.Exception
    }
    finally {
        try {
            if ($processStarted) {
                Stop-BoundedProcess -Process $process
            }
        }
        catch {
            if ($null -eq $operationFailure) {
                $operationFailure = $_.Exception
            }
            else {
                $operationFailure = [System.AggregateException]::new(
                    "Runtime-containment verification and process cleanup both failed.",
                    @($operationFailure, $_.Exception)
                )
            }
        }
        $stdout.Dispose()
        $stderr.Dispose()
        $process.Dispose()
    }
    if ($null -ne $operationFailure) {
        throw $operationFailure
    }
    return $commandResult
}

if ($env:OS -ne "Windows_NT" -or -not [System.Environment]::Is64BitProcess) {
    throw "Runtime-containment artifact verification requires 64-bit Windows."
}
$output = Get-Item -LiteralPath ([System.IO.Path]::GetFullPath($OutputRoot)) -Force
if (-not $output.PSIsContainer -or
    (($output.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
    throw "Runtime-containment output root must be one direct directory."
}

$broker = Get-DirectOutputItem `
    -Root $output `
    -RelativePath "build/release/$brokerFileName" `
    -Kind File
$brokerDigest = (Get-FileHash -Algorithm SHA256 -LiteralPath $broker.FullName).Hash.ToLowerInvariant()
$brokerBytes = [long]$broker.Length
$artifactManifestDigest = $null

foreach ($assembly in @("staged-a", "staged-b")) {
    $stagedBroker = Get-DirectOutputItem `
        -Root $output `
        -RelativePath "work/$assembly/$brokerFileName" `
        -Kind File
    $artifactManifestFile = Get-DirectOutputItem `
        -Root $output `
        -RelativePath "work/$assembly/$artifactManifestFileName" `
        -Kind File
    $nativeAuditFile = Get-DirectOutputItem `
        -Root $output `
        -RelativePath "work/$assembly/$nativeAuditFileName" `
        -Kind File
    $manifest = Read-BoundedJson -File $artifactManifestFile
    $nativeAudit = Read-BoundedJson -File $nativeAuditFile
    $brokerRows = @($manifest.files | Where-Object {
        [string]$_.path -ceq $brokerFileName
    })
    if ([string]$manifest.platform -cne "x86_64-pc-windows-msvc" -or
        $brokerRows.Count -ne 1 -or
        [string]$brokerRows[0].role.kind -cne "containment-broker" -or
        [string]$brokerRows[0].sha256 -cne $brokerDigest -or
        [long]$brokerRows[0].bytes -ne $brokerBytes -or
        [string]$nativeAudit.containment_broker.file.path -cne $brokerFileName -or
        [string]$nativeAudit.containment_broker.file.sha256 -cne $brokerDigest -or
        [long]$nativeAudit.containment_broker.file.byte_length -ne $brokerBytes -or
        $stagedBroker.Length -ne $brokerBytes -or
        (Get-FileHash -Algorithm SHA256 -LiteralPath $stagedBroker.FullName).Hash.ToLowerInvariant() -ne
            $brokerDigest) {
        throw "Runtime-containment broker is not bound to the exact staged artifact."
    }
    $observedManifestDigest = (
        Get-FileHash -Algorithm SHA256 -LiteralPath $artifactManifestFile.FullName
    ).Hash.ToLowerInvariant()
    if ($null -eq $artifactManifestDigest) {
        $artifactManifestDigest = $observedManifestDigest
    }
    elseif ($observedManifestDigest -ne $artifactManifestDigest) {
        throw "Independent runtime-containment artifact manifests are not byte-identical."
    }
}

$verificationRootPath = [System.IO.Path]::Combine(
    $output.FullName,
    "runtime-containment-verification"
)
if ([System.IO.File]::Exists($verificationRootPath) -or
    [System.IO.Directory]::Exists($verificationRootPath)) {
    throw "Runtime-containment verification root already exists."
}
$verificationRoot = [System.IO.Directory]::CreateDirectory($verificationRootPath)
$verificationFailure = $null
try {
    $version = Invoke-BoundedBrokerCommand `
        -Broker $broker `
        -Arguments @("--version") `
        -WorkingDirectory $verificationRoot `
        -TimeoutSeconds 15
    if ($version.ExitCode -ne 0 -or
        $version.StandardOutput -cne "projectatlas-parser-containment 1`r`n" -or
        $version.StandardError.Length -ne 0) {
        throw "Runtime-containment broker version contract failed."
    }

    $contract = Invoke-BoundedBrokerCommand `
        -Broker $broker `
        -Arguments @("--build-contract") `
        -WorkingDirectory $verificationRoot `
        -TimeoutSeconds 15
    $contractText = $contract.StandardOutput.TrimEnd("`r", "`n")
    if ($contract.ExitCode -ne 0 -or
        $contract.StandardError.Length -ne 0 -or
        $contractText -cnotmatch $contractPattern) {
        throw "Runtime-containment broker build contract failed."
    }
    $runtimeFamily = [string]$Matches[1]
    $architecture = [string]$Matches[2]
    $managedModules = @($Matches[3] -split ',')
    $managedImportCount = [int]$Matches[4]
    $managedImportsSha256 = [string]$Matches[5]
    foreach ($assembly in @("staged-a", "staged-b")) {
        $nativeAuditFile = Get-DirectOutputItem `
            -Root $output `
            -RelativePath "work/$assembly/$nativeAuditFileName" `
            -Kind File
        $nativeAudit = Read-BoundedJson -File $nativeAuditFile
        $auditBroker = $nativeAudit.containment_broker
        if ([string]$auditBroker.runtime_family -cne $runtimeFamily -or
            [string]$auditBroker.architecture -cne $architecture -or
            (@($auditBroker.managed_modules) -join ',') -cne ($managedModules -join ',') -or
            [int]$auditBroker.managed_import_count -ne $managedImportCount -or
            [string]$auditBroker.managed_imports_sha256 -cne $managedImportsSha256) {
            throw "Runtime-containment broker build contract is not bound to the native audit."
        }
    }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $broker.FullName).Hash.ToLowerInvariant() -ne
        $brokerDigest) {
        throw "Runtime-containment broker changed during contract audit."
    }

    $selfTest = Invoke-BoundedBrokerCommand `
        -Broker $broker `
        -Arguments @("self-test") `
        -WorkingDirectory $verificationRoot `
        -TimeoutSeconds $SelfTestTimeoutSeconds
    if ($selfTest.ExitCode -ne 0 -or
        $selfTest.StandardOutput -cne "[parser-containment] self-test passed`r`n" -or
        $selfTest.StandardError.Length -ne 0) {
        $stderr = $selfTest.StandardError.TrimEnd("`r", "`n")
        throw "Runtime-containment broker self-test failed with exit code $($selfTest.ExitCode): $stderr"
    }
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $broker.FullName).Hash.ToLowerInvariant() -ne
        $brokerDigest -or
        @(Get-ChildItem `
            -LiteralPath $broker.DirectoryName `
            -Directory `
            -Filter "projectatlas-parser-containment-*" `
            -Force).Count -ne 0) {
        throw "Runtime-containment broker changed or retained self-test state."
    }
}
catch {
    $verificationFailure = $_.Exception
}
finally {
    try {
        if ([System.IO.Directory]::Exists($verificationRootPath)) {
            [System.IO.Directory]::Delete($verificationRootPath, $true)
        }
    }
    catch {
        if ($null -eq $verificationFailure) {
            $verificationFailure = $_.Exception
        }
        else {
            $verificationFailure = [System.AggregateException]::new(
                "Runtime-containment verification and temporary cleanup both failed.",
                @($verificationFailure, $_.Exception)
            )
        }
    }
}
if ($null -ne $verificationFailure) {
    throw $verificationFailure
}

Write-Output "Runtime-containment broker artifact verification passed."
