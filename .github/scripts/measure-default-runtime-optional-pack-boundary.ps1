[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$SourceRoot,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ArchivePath,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$CargoTargetRoot,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$OutputPath,

    [Parameter(Mandatory = $true)]
    [ValidateSet("x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc")]
    [string]$Target,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{40,64}$')]
    [string]$CandidateSha
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$archiveMaximumBytes = 64L * 1024L * 1024L
$runtimeInfoSampleCount = 9
$mcpReadySampleCount = 9
$idleRssSampleCount = 9
$idleStabilizationMilliseconds = 2000
$idleSampleIntervalMilliseconds = 200
# The absolute floors absorb hosted-runner scheduling and allocator noise; the
# relative allowances scale only when the control runtime is materially larger.
$startupAbsoluteToleranceMilliseconds = 15.0
$startupRelativeTolerancePercent = 25.0
$idleRssAbsoluteToleranceBytes = 8L * 1024L * 1024L
$idleRssRelativeTolerancePercent = 15.0
$isWindowsHost = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
    [System.Runtime.InteropServices.OSPlatform]::Windows
)

function Invoke-BoundedProcess {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FileName,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,

        [Parameter(Mandatory = $true)]
        [string]$WorkingDirectory,

        [hashtable]$Environment = @{},

        [ValidateRange(1, 3600)]
        [int]$TimeoutSeconds = 120
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FileName
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        [void]$startInfo.ArgumentList.Add($argument)
    }
    foreach ($entry in $Environment.GetEnumerator()) {
        $startInfo.Environment[[string]$entry.Key] = [string]$entry.Value
    }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    if (-not $process.Start()) {
        throw "Could not start $FileName."
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        $process.Kill($true)
        $process.WaitForExit()
        throw "$FileName exceeded its $TimeoutSeconds second operational timeout."
    }
    $stopwatch.Stop()
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $exitCode = $process.ExitCode
    $process.Dispose()

    [pscustomobject]@{
        exit_code = $exitCode
        stdout = $stdout
        stderr = $stderr
        elapsed_milliseconds = [Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 3)
    }
}

function Assert-ProcessSucceeded {
    param(
        [Parameter(Mandatory = $true)]
        [pscustomobject]$Result,

        [Parameter(Mandatory = $true)]
        [string]$Operation
    )

    if ($Result.exit_code -eq 0) {
        return
    }
    $diagnostic = $Result.stderr.Trim()
    if ($diagnostic.Length -gt 4096) {
        $diagnostic = $diagnostic.Substring(0, 4096)
    }
    throw "$Operation failed with exit code $($Result.exit_code): $diagnostic"
}

function Get-Median {
    param(
        [Parameter(Mandatory = $true)]
        [double[]]$Values
    )

    if ($Values.Count -eq 0) {
        throw "Cannot calculate a median without samples."
    }
    $ordered = @($Values | Sort-Object)
    $middle = [Math]::Floor($ordered.Count / 2)
    if (($ordered.Count % 2) -eq 1) {
        return [double]$ordered[$middle]
    }
    return ([double]$ordered[$middle - 1] + [double]$ordered[$middle]) / 2.0
}

function Get-OptionalParserProcessIds {
    if ($isWindowsHost) {
        return @(
            Get-Process -ErrorAction SilentlyContinue |
                Where-Object {
                    $_.ProcessName -in @(
                        "projectatlas-parser-worker",
                        "projectatlas-parser-containment"
                    )
                } |
                ForEach-Object { [int]$_.Id }
        )
    }

    return @(
        & /bin/ps -eo pid=,args= |
            ForEach-Object {
                if ($_ -match '^\s*(\d+)\s+(.+)$') {
                    $processId = [int]$Matches[1]
                    $commandLine = $Matches[2]
                    if ($commandLine -match '(?:^|/|\\)projectatlas-parser-(?:worker|containment)(?:\s|$)') {
                        $processId
                    }
                }
            }
    )
}

function Test-OptionalStorageAbsent {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [string]$Checkpoint
    )

    if (Test-Path -LiteralPath $Path) {
        throw "Optional parser-pack storage appeared during $Checkpoint."
    }
    return $true
}

function Get-MeasurementDistribution {
    param(
        [Parameter(Mandatory = $true)]
        [double[]]$Values,

        [Parameter(Mandatory = $true)]
        [ValidateRange(0, 6)]
        [int]$RoundDigits,

        [Parameter(Mandatory = $true)]
        [string]$Unit
    )

    if ($Values.Count -eq 0) {
        throw "Cannot describe a measurement distribution without samples."
    }
    $roundedSamples = @($Values | ForEach-Object { [Math]::Round($_, $RoundDigits) })
    [pscustomobject][ordered]@{
        sample_count = $Values.Count
        samples = $roundedSamples
        minimum = [Math]::Round(($Values | Measure-Object -Minimum).Minimum, $RoundDigits)
        median = [Math]::Round((Get-Median -Values $Values), $RoundDigits)
        maximum = [Math]::Round(($Values | Measure-Object -Maximum).Maximum, $RoundDigits)
        unit = $Unit
    }
}

function New-MaterialRegressionComparison {
    param(
        [Parameter(Mandatory = $true)]
        [double]$DefaultMedian,

        [Parameter(Mandatory = $true)]
        [double]$ControlMedian,

        [Parameter(Mandatory = $true)]
        [double]$AbsoluteTolerance,

        [Parameter(Mandatory = $true)]
        [double]$RelativeTolerancePercent,

        [Parameter(Mandatory = $true)]
        [string]$Unit,

        [Parameter(Mandatory = $true)]
        [ValidateRange(0, 6)]
        [int]$RoundDigits
    )

    if ($DefaultMedian -lt 0 -or $ControlMedian -le 0 -or $AbsoluteTolerance -lt 0 -or
        $RelativeTolerancePercent -lt 0) {
        throw "Material-regression comparison inputs are outside their valid range."
    }
    $relativeAllowance = $ControlMedian * ($RelativeTolerancePercent / 100.0)
    $effectiveAllowance = [Math]::Max($AbsoluteTolerance, $relativeAllowance)
    $maximumDefaultMedian = $ControlMedian + $effectiveAllowance
    $absoluteDelta = $DefaultMedian - $ControlMedian
    $relativeDeltaPercent = ($absoluteDelta / $ControlMedian) * 100.0
    [pscustomobject][ordered]@{
        default_median = [Math]::Round($DefaultMedian, $RoundDigits)
        control_median = [Math]::Round($ControlMedian, $RoundDigits)
        absolute_delta = [Math]::Round($absoluteDelta, $RoundDigits)
        relative_delta_percent = [Math]::Round($relativeDeltaPercent, 3)
        tolerance = [ordered]@{
            absolute = [Math]::Round($AbsoluteTolerance, $RoundDigits)
            relative_percent = [Math]::Round($RelativeTolerancePercent, 3)
            effective_allowed_increase = [Math]::Round($effectiveAllowance, $RoundDigits)
            maximum_default_median = [Math]::Round($maximumDefaultMedian, $RoundDigits)
            rule = "default_median <= control_median + max(absolute, control_median * (relative_percent / 100))"
        }
        unit = $Unit
        passed = $DefaultMedian -le $maximumDefaultMedian
    }
}

function Build-RuntimeProfile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [Parameter(Mandatory = $true)]
        [string]$CargoFeatures,

        [Parameter(Mandatory = $true)]
        [string[]]$FeatureArguments,

        [Parameter(Mandatory = $true)]
        [string]$TargetDirectory,

        [Parameter(Mandatory = $true)]
        [string]$SourceDirectory,

        [Parameter(Mandatory = $true)]
        [string]$TargetTriple,

        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$ExecutableSuffix
    )

    if (Test-Path -LiteralPath $TargetDirectory) {
        throw "$Name requires a fresh isolated Cargo target directory."
    }
    $arguments = @(
        "build",
        "--locked",
        "--release",
        "-p", "projectatlas-cli",
        "--bins"
    )
    $arguments += $FeatureArguments
    $arguments += @("--target", $TargetTriple, "--target-dir", $TargetDirectory)
    $result = Invoke-BoundedProcess -FileName "cargo" `
        -Arguments $arguments `
        -WorkingDirectory $SourceDirectory `
        -TimeoutSeconds 1800
    Assert-ProcessSucceeded -Result $result -Operation "$Name runtime build"

    $releaseDirectory = Join-Path (Join-Path $TargetDirectory $TargetTriple) "release"
    $runtimePath = Join-Path $releaseDirectory "projectatlas$ExecutableSuffix"
    if (-not (Test-Path -LiteralPath $runtimePath -PathType Leaf)) {
        throw "$Name projectatlas runtime was not built."
    }
    $runtimeBinary = Get-Item -LiteralPath $runtimePath
    if ($runtimeBinary.Length -lt 1) {
        throw "$Name projectatlas runtime is empty."
    }

    $workerBinaryName = "projectatlas-parser-worker$ExecutableSuffix"
    $workerBinaries = @(
        Get-ChildItem -LiteralPath $TargetDirectory -Recurse -File |
            Where-Object { $_.Name -eq $workerBinaryName }
    )
    if ($workerBinaries.Count -ne 0) {
        throw "$Name Cargo target unexpectedly shipped projectatlas-parser-worker."
    }

    $treeArguments = @(
        "tree",
        "--locked",
        "-p", "projectatlas-cli",
        "--edges", "normal,build"
    )
    $treeArguments += $FeatureArguments
    $treeArguments += @(
        "--target", $TargetTriple,
        "--prefix", "none",
        "--format", "{p}"
    )
    $treeResult = Invoke-BoundedProcess -FileName "cargo" `
        -Arguments $treeArguments `
        -WorkingDirectory $SourceDirectory `
        -TimeoutSeconds 300
    Assert-ProcessSucceeded -Result $treeResult -Operation "$Name dependency surface inspection"
    $forbiddenDependencies = @("tree-sitter-language-pack", "landlock", "seccompiler")
    foreach ($dependency in $forbiddenDependencies) {
        if ($treeResult.stdout -match "(?m)^$([Regex]::Escape($dependency)) v") {
            throw "$Name dependency surface unexpectedly includes $dependency."
        }
    }

    [pscustomobject][ordered]@{
        name = $Name
        cargo_features = $CargoFeatures
        feature_arguments = $FeatureArguments
        target_directory = $TargetDirectory
        runtime_path = $runtimePath
        runtime_binary_bytes = [long]$runtimeBinary.Length
        runtime_binary_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $runtimePath).Hash.ToLowerInvariant()
        parser_worker_binary_shipped = $false
        forbidden_dependencies_present = [ordered]@{
            tree_sitter_language_pack = $false
            landlock = $false
            seccompiler = $false
        }
    }
}

function New-IsolatedRuntimeState {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [Parameter(Mandatory = $true)]
        [string]$Root
    )

    if (Test-Path -LiteralPath $Root) {
        throw "$Name requires a fresh isolated runtime-state root."
    }
    $hostHome = Join-Path $Root "home"
    $localAppData = Join-Path $Root "local-app-data"
    $xdgDataHome = Join-Path $Root "xdg-data"
    $runtimeTemp = Join-Path $Root "temp"
    $repository = Join-Path $Root "repository"
    foreach ($directory in @($hostHome, $localAppData, $xdgDataHome, $runtimeTemp, $repository)) {
        [System.IO.Directory]::CreateDirectory($directory) | Out-Null
    }
    $optionalStorage = if ($isWindowsHost) {
        Join-Path $localAppData "ProjectAtlas/parser-packs"
    } else {
        Join-Path $xdgDataHome "projectatlas/parser-packs"
    }
    [pscustomobject][ordered]@{
        name = $Name
        root = $Root
        repository = $repository
        optional_storage = $optionalStorage
        environment = @{
            HOME = $hostHome
            USERPROFILE = $hostHome
            LOCALAPPDATA = $localAppData
            XDG_DATA_HOME = $xdgDataHome
            TMP = $runtimeTemp
            TEMP = $runtimeTemp
            TMPDIR = $runtimeTemp
        }
    }
}

function Initialize-RuntimeState {
    param(
        [Parameter(Mandatory = $true)]
        [pscustomobject]$Profile,

        [Parameter(Mandatory = $true)]
        [pscustomobject]$State
    )

    $result = Invoke-BoundedProcess -FileName $Profile.runtime_path `
        -Arguments @("--format", "json", "init", "--no-scan") `
        -WorkingDirectory $State.repository `
        -Environment $State.environment `
        -TimeoutSeconds 120
    Assert-ProcessSucceeded -Result $result -Operation "$($Profile.name) isolated repository initialization"
    $storageAbsent = Test-OptionalStorageAbsent `
        -Path $State.optional_storage `
        -Checkpoint "$($Profile.name) repository initialization"
    $databasePath = Join-Path $State.repository ".projectatlas/projectatlas.db"
    if (-not (Test-Path -LiteralPath $databasePath -PathType Leaf)) {
        throw "$($Profile.name) repository initialization did not create the project database."
    }
    [pscustomobject][ordered]@{
        database_path = $databasePath
        database_created = $true
        storage_absent_after_init = $storageAbsent
    }
}

function Complete-McpInitialization {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process,

        [Parameter(Mandatory = $true)]
        [string]$ProfileName
    )

    $Process.StandardInput.AutoFlush = $true
    $initialize = [ordered]@{
        jsonrpc = "2.0"
        id = 1
        method = "initialize"
        params = [ordered]@{
            protocolVersion = "2024-11-05"
            capabilities = [ordered]@{}
            clientInfo = [ordered]@{
                name = "projectatlas-release-measurement"
                version = "0.4.0"
            }
        }
    } | ConvertTo-Json -Compress -Depth 8
    $Process.StandardInput.WriteLine($initialize)
    $responseTask = $Process.StandardOutput.ReadLineAsync()
    if (-not $responseTask.Wait(30000)) {
        throw "The $ProfileName MCP runtime did not become initialize-ready within 30 seconds."
    }
    $responseLine = $responseTask.GetAwaiter().GetResult()
    if ([string]::IsNullOrWhiteSpace($responseLine)) {
        throw "The $ProfileName MCP runtime closed stdout before initialize-ready."
    }
    $response = $responseLine | ConvertFrom-Json -Depth 16
    if ($response.id -ne 1 -or $null -eq $response.result -or
        $null -eq $response.result.serverInfo -or
        [string]::IsNullOrWhiteSpace([string]$response.result.serverInfo.name)) {
        throw "The $ProfileName MCP initialize response was not a ready server response."
    }
    $initialized = [ordered]@{
        jsonrpc = "2.0"
        method = "notifications/initialized"
        params = [ordered]@{}
    } | ConvertTo-Json -Compress -Depth 4
    $Process.StandardInput.WriteLine($initialized)
}

function Measure-McpInitializeReady {
    param(
        [Parameter(Mandatory = $true)]
        [pscustomobject]$Profile,

        [Parameter(Mandatory = $true)]
        [pscustomobject]$State,

        [Parameter(Mandatory = $true)]
        [string]$DatabasePath
    )

    if (@(Get-OptionalParserProcessIds).Count -ne 0) {
        throw "The measurement host has an optional parser worker or broker before $($Profile.name) MCP readiness."
    }
    [void](Test-OptionalStorageAbsent `
        -Path $State.optional_storage `
        -Checkpoint "$($Profile.name) pre-MCP readiness")

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Profile.runtime_path
    $startInfo.WorkingDirectory = $State.repository
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in @("--db", $DatabasePath, "mcp")) {
        [void]$startInfo.ArgumentList.Add($argument)
    }
    foreach ($entry in $State.environment.GetEnumerator()) {
        $startInfo.Environment[[string]$entry.Key] = [string]$entry.Value
    }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $started = $false
    $stderrTask = $null
    $stdoutRemainderTask = $null
    $forcedTermination = $false
    $exitCode = $null
    try {
        if (-not $process.Start()) {
            throw "Could not start the $($Profile.name) MCP readiness sample."
        }
        $started = $true
        $stderrTask = $process.StandardError.ReadToEndAsync()
        Complete-McpInitialization -Process $process -ProfileName $Profile.name
        $stopwatch.Stop()
        if (@(Get-OptionalParserProcessIds).Count -ne 0) {
            throw "$($Profile.name) MCP initialize-ready launched an optional parser worker or broker."
        }
        [void](Test-OptionalStorageAbsent `
            -Path $State.optional_storage `
            -Checkpoint "$($Profile.name) MCP initialize-ready")
        $process.StandardInput.Close()
        $stdoutRemainderTask = $process.StandardOutput.ReadToEndAsync()
        if (-not $process.WaitForExit(10000)) {
            $forcedTermination = $true
            $process.Kill($true)
            $process.WaitForExit()
        }
        $exitCode = $process.ExitCode
    } finally {
        if ($started -and -not $process.HasExited) {
            $forcedTermination = $true
            $process.Kill($true)
            $process.WaitForExit()
            $exitCode = $process.ExitCode
        }
        if ($null -ne $stdoutRemainderTask) {
            [void]$stdoutRemainderTask.GetAwaiter().GetResult()
        }
        if ($null -ne $stderrTask) {
            [void]$stderrTask.GetAwaiter().GetResult()
        }
        $process.Dispose()
    }
    if ($forcedTermination) {
        throw "The $($Profile.name) MCP readiness sample required forced termination."
    }
    if ($exitCode -ne 0) {
        throw "The $($Profile.name) MCP readiness sample returned exit code $exitCode."
    }
    if (@(Get-OptionalParserProcessIds).Count -ne 0) {
        throw "$($Profile.name) MCP readiness shutdown retained an optional parser worker or broker."
    }
    [void](Test-OptionalStorageAbsent `
        -Path $State.optional_storage `
        -Checkpoint "$($Profile.name) MCP readiness shutdown")
    return [double][Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 3)
}

function Measure-McpIdleResidentSet {
    param(
        [Parameter(Mandatory = $true)]
        [pscustomobject]$Profile,

        [Parameter(Mandatory = $true)]
        [pscustomobject]$State,

        [Parameter(Mandatory = $true)]
        [string]$DatabasePath
    )

    $baselineProcesses = @(Get-OptionalParserProcessIds)
    if ($baselineProcesses.Count -ne 0) {
        throw "The measurement host has an optional parser worker or broker before $($Profile.name) MCP startup."
    }
    $storageAbsentBeforeStartup = Test-OptionalStorageAbsent `
        -Path $State.optional_storage `
        -Checkpoint "$($Profile.name) pre-MCP startup"

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Profile.runtime_path
    $startInfo.WorkingDirectory = $State.repository
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in @("--db", $DatabasePath, "mcp")) {
        [void]$startInfo.ArgumentList.Add($argument)
    }
    foreach ($entry in $State.environment.GetEnumerator()) {
        $startInfo.Environment[[string]$entry.Key] = [string]$entry.Value
    }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $started = $false
    $stdoutTask = $null
    $stderrTask = $null
    $forcedTermination = $false
    $exitCode = $null
    $samples = @()
    $storageAbsentDuringMcp = $false
    try {
        if (-not $process.Start()) {
            throw "Could not start the $($Profile.name) isolated MCP runtime."
        }
        $started = $true
        $stderrTask = $process.StandardError.ReadToEndAsync()
        Complete-McpInitialization -Process $process -ProfileName $Profile.name
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        Start-Sleep -Milliseconds $idleStabilizationMilliseconds
        if ($process.HasExited) {
            throw "The $($Profile.name) isolated MCP runtime exited before idle measurement."
        }

        $optionalProcessesDuringIdle = @(Get-OptionalParserProcessIds)
        if ($optionalProcessesDuringIdle.Count -ne 0) {
            throw "$($Profile.name) absent-pack MCP startup launched an optional parser worker or broker."
        }
        for ($sample = 0; $sample -lt $idleRssSampleCount; $sample++) {
            if ($process.HasExited) {
                throw "The $($Profile.name) isolated MCP runtime exited during idle measurement."
            }
            $process.Refresh()
            if ($process.WorkingSet64 -lt 1) {
                throw "The $($Profile.name) isolated MCP runtime reported an invalid resident set size."
            }
            $samples += [double]$process.WorkingSet64
            if ($sample + 1 -lt $idleRssSampleCount) {
                Start-Sleep -Milliseconds $idleSampleIntervalMilliseconds
            }
        }
        $storageAbsentDuringMcp = Test-OptionalStorageAbsent `
            -Path $State.optional_storage `
            -Checkpoint "$($Profile.name) idle MCP startup"
    } finally {
        if ($started) {
            if (-not $process.HasExited) {
                $process.StandardInput.Close()
                if (-not $process.WaitForExit(10000)) {
                    $forcedTermination = $true
                    $process.Kill($true)
                    $process.WaitForExit()
                }
            }
            $exitCode = $process.ExitCode
            if ($null -ne $stdoutTask) {
                [void]$stdoutTask.GetAwaiter().GetResult()
            }
            if ($null -ne $stderrTask) {
                [void]$stderrTask.GetAwaiter().GetResult()
            }
        }
        $process.Dispose()
    }
    if ($forcedTermination) {
        throw "The $($Profile.name) isolated MCP runtime did not stop after stdin closed."
    }
    if ($exitCode -ne 0) {
        throw "The $($Profile.name) isolated MCP runtime returned exit code $exitCode after stdin closed."
    }

    $optionalProcessesAfterShutdown = @(Get-OptionalParserProcessIds)
    if ($optionalProcessesAfterShutdown.Count -ne 0) {
        throw "$($Profile.name) absent-pack MCP shutdown retained an optional parser worker or broker."
    }
    $storageAbsentAfterShutdown = Test-OptionalStorageAbsent `
        -Path $State.optional_storage `
        -Checkpoint "$($Profile.name) MCP shutdown"
    [pscustomobject][ordered]@{
        samples = [double[]]$samples
        optional_worker_or_broker_processes_present_before_startup = 0
        optional_worker_or_broker_processes_present_during_idle_mcp = 0
        optional_worker_or_broker_processes_present_after_mcp_shutdown = 0
        storage_absent_before_startup = $storageAbsentBeforeStartup
        storage_absent_during_mcp = $storageAbsentDuringMcp
        storage_absent_after_shutdown = $storageAbsentAfterShutdown
    }
}

$resolvedSourceRoot = (Resolve-Path -LiteralPath $SourceRoot).Path
$resolvedArchivePath = (Resolve-Path -LiteralPath $ArchivePath).Path
$candidateShaNormalized = $CandidateSha.ToLowerInvariant()
$expectedWindowsTarget = $Target -eq "x86_64-pc-windows-msvc"
if ($isWindowsHost -ne $expectedWindowsTarget) {
    throw "The requested target does not match the measurement host."
}
if (Test-Path -LiteralPath $CargoTargetRoot) {
    throw "Default-runtime comparison requires a fresh Cargo target root."
}
$resolvedCargoTargetRoot = [System.IO.Path]::GetFullPath($CargoTargetRoot)
[System.IO.Directory]::CreateDirectory($resolvedCargoTargetRoot) | Out-Null
$defaultCargoTargetDirectory = Join-Path $resolvedCargoTargetRoot "default-features"
$controlCargoTargetDirectory = Join-Path $resolvedCargoTargetRoot "no-default-features-control"

$revisionResult = Invoke-BoundedProcess -FileName "git" `
    -Arguments @("rev-parse", "HEAD") `
    -WorkingDirectory $resolvedSourceRoot
Assert-ProcessSucceeded -Result $revisionResult -Operation "candidate revision lookup"
if ($revisionResult.stdout.Trim().ToLowerInvariant() -ne $candidateShaNormalized) {
    throw "The measurement candidate does not equal the checked-out revision."
}
$statusResult = Invoke-BoundedProcess -FileName "git" `
    -Arguments @("status", "--porcelain=v1", "--untracked-files=all") `
    -WorkingDirectory $resolvedSourceRoot
Assert-ProcessSucceeded -Result $statusResult -Operation "candidate cleanliness check"
if (-not [string]::IsNullOrWhiteSpace($statusResult.stdout)) {
    throw "Default-runtime measurement requires an exact clean candidate."
}

$archive = Get-Item -LiteralPath $resolvedArchivePath
if ($archive.Length -lt 1 -or $archive.Length -gt $archiveMaximumBytes) {
    throw "Optional parser-pack archive size is outside its declared 1..$archiveMaximumBytes byte boundary."
}
$archiveSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $resolvedArchivePath).Hash.ToLowerInvariant()

$buildResult = Invoke-BoundedProcess -FileName "cargo" `
    -Arguments @(
        "build",
        "--locked",
        "--release",
        "-p", "projectatlas-cli",
        "--bins",
        "--target", $Target,
        "--target-dir", $defaultCargoTargetDirectory
    ) `
    -WorkingDirectory $resolvedSourceRoot `
    -TimeoutSeconds 1800
Assert-ProcessSucceeded -Result $buildResult -Operation "default-feature runtime build"

$executableSuffix = if ($isWindowsHost) { ".exe" } else { "" }
$releaseDirectory = Join-Path (Join-Path $defaultCargoTargetDirectory $Target) "release"
$runtimePath = Join-Path $releaseDirectory "projectatlas$executableSuffix"
if (-not (Test-Path -LiteralPath $runtimePath -PathType Leaf)) {
    throw "The default-feature projectatlas runtime was not built."
}
$runtimeBinary = Get-Item -LiteralPath $runtimePath
if ($runtimeBinary.Length -lt 1) {
    throw "The default-feature projectatlas runtime is empty."
}
$runtimeBinarySha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $runtimePath).Hash.ToLowerInvariant()

$workerBinaryName = "projectatlas-parser-worker$executableSuffix"
$workerBinaries = @(
    Get-ChildItem -LiteralPath $defaultCargoTargetDirectory -Recurse -File |
        Where-Object { $_.Name -eq $workerBinaryName }
)
if ($workerBinaries.Count -ne 0) {
    throw "The default Cargo target unexpectedly shipped projectatlas-parser-worker."
}

$treeResult = Invoke-BoundedProcess -FileName "cargo" `
    -Arguments @(
        "tree",
        "--locked",
        "-p", "projectatlas-cli",
        "--edges", "normal,build",
        "--target", $Target,
        "--prefix", "none",
        "--format", "{p}"
    ) `
    -WorkingDirectory $resolvedSourceRoot `
    -TimeoutSeconds 300
Assert-ProcessSucceeded -Result $treeResult -Operation "default dependency surface inspection"
$forbiddenDependencies = @("tree-sitter-language-pack", "landlock", "seccompiler")
foreach ($dependency in $forbiddenDependencies) {
    if ($treeResult.stdout -match "(?m)^$([Regex]::Escape($dependency)) v") {
        throw "Default dependency surface unexpectedly includes $dependency."
    }
}

$defaultProfile = [pscustomobject][ordered]@{
    name = "default_features"
    cargo_features = "default"
    feature_arguments = @()
    target_directory = $defaultCargoTargetDirectory
    runtime_path = $runtimePath
    runtime_binary_bytes = [long]$runtimeBinary.Length
    runtime_binary_sha256 = $runtimeBinarySha256
    parser_worker_binary_shipped = $false
    forbidden_dependencies_present = [ordered]@{
        tree_sitter_language_pack = $false
        landlock = $false
        seccompiler = $false
    }
}
$controlProfile = Build-RuntimeProfile `
    -Name "no_default_features_control" `
    -CargoFeatures "cli-core" `
    -FeatureArguments @("--no-default-features", "--features", "cli-core") `
    -TargetDirectory $controlCargoTargetDirectory `
    -SourceDirectory $resolvedSourceRoot `
    -TargetTriple $Target `
    -ExecutableSuffix $executableSuffix

$rustcResult = Invoke-BoundedProcess -FileName "rustc" `
    -Arguments @("-vV") `
    -WorkingDirectory $resolvedSourceRoot
Assert-ProcessSucceeded -Result $rustcResult -Operation "Rust toolchain identity"
$cargoResult = Invoke-BoundedProcess -FileName "cargo" `
    -Arguments @("-V") `
    -WorkingDirectory $resolvedSourceRoot
Assert-ProcessSucceeded -Result $cargoResult -Operation "Cargo toolchain identity"
$rustcRelease = ([regex]::Match($rustcResult.stdout, '(?m)^release: (.+)$')).Groups[1].Value.Trim()
$rustcCommit = ([regex]::Match($rustcResult.stdout, '(?m)^commit-hash: (.+)$')).Groups[1].Value.Trim()
$rustcHost = ([regex]::Match($rustcResult.stdout, '(?m)^host: (.+)$')).Groups[1].Value.Trim()
if ([string]::IsNullOrWhiteSpace($rustcRelease) -or
    [string]::IsNullOrWhiteSpace($rustcCommit) -or
    $rustcHost -ne $Target) {
    throw "Pinned Rust toolchain identity is incomplete or does not match the target."
}

$outputDirectory = [System.IO.Path]::GetDirectoryName($OutputPath)
if ([string]::IsNullOrWhiteSpace($outputDirectory)) {
    throw "OutputPath must include a parent directory."
}
[System.IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$measurementRoot = Join-Path $outputDirectory "default-runtime-state"
if (Test-Path -LiteralPath $measurementRoot) {
    throw "Default-feature measurement requires a fresh isolated runtime-state root."
}
$hostHome = Join-Path $measurementRoot "home"
$localAppData = Join-Path $measurementRoot "local-app-data"
$xdgDataHome = Join-Path $measurementRoot "xdg-data"
$runtimeTemp = Join-Path $measurementRoot "temp"
$repository = Join-Path $measurementRoot "repository"
foreach ($directory in @($hostHome, $localAppData, $xdgDataHome, $runtimeTemp, $repository)) {
    [System.IO.Directory]::CreateDirectory($directory) | Out-Null
}
$optionalStorage = if ($isWindowsHost) {
    Join-Path $localAppData "ProjectAtlas/parser-packs"
} else {
    Join-Path $xdgDataHome "projectatlas/parser-packs"
}
$runtimeEnvironment = @{
    HOME = $hostHome
    USERPROFILE = $hostHome
    LOCALAPPDATA = $localAppData
    XDG_DATA_HOME = $xdgDataHome
    TMP = $runtimeTemp
    TEMP = $runtimeTemp
    TMPDIR = $runtimeTemp
}
$defaultState = [pscustomobject][ordered]@{
    name = $defaultProfile.name
    root = $measurementRoot
    repository = $repository
    optional_storage = $optionalStorage
    environment = $runtimeEnvironment
}
$controlState = New-IsolatedRuntimeState `
    -Name $controlProfile.name `
    -Root (Join-Path $outputDirectory "no-default-features-control-runtime-state")

$storageAbsentBeforeStartup = Test-OptionalStorageAbsent `
    -Path $optionalStorage `
    -Checkpoint "pre-startup baseline"
$baselineOptionalProcesses = @(Get-OptionalParserProcessIds)
if ($baselineOptionalProcesses.Count -ne 0) {
    throw "The measurement host already has an optional parser worker or broker process."
}

$runtimeInfoSamples = @()
$runtimeVersion = $null
for ($sample = 0; $sample -lt $runtimeInfoSampleCount; $sample++) {
    $runtimeInfoResult = Invoke-BoundedProcess -FileName $runtimePath `
        -Arguments @("--format", "json", "runtime-info") `
        -WorkingDirectory $repository `
        -Environment $runtimeEnvironment `
        -TimeoutSeconds 30
    Assert-ProcessSucceeded -Result $runtimeInfoResult -Operation "runtime-info fresh-process sample"
    $runtimeInfo = $runtimeInfoResult.stdout | ConvertFrom-Json -Depth 32
    if ($null -eq $runtimeInfo.version -or [string]::IsNullOrWhiteSpace([string]$runtimeInfo.version)) {
        throw "runtime-info did not report the runtime version."
    }
    if ($null -eq $runtimeVersion) {
        $runtimeVersion = [string]$runtimeInfo.version
    } elseif ($runtimeVersion -ne [string]$runtimeInfo.version) {
        throw "runtime-info version changed between fresh-process samples."
    }
    $runtimeInfoSamples += [double]$runtimeInfoResult.elapsed_milliseconds
}
$storageAbsentAfterRuntimeInfo = Test-OptionalStorageAbsent `
    -Path $optionalStorage `
    -Checkpoint "runtime-info samples"
$defaultOptionalProcessesAfterRuntimeInfo = @(Get-OptionalParserProcessIds)
if ($defaultOptionalProcessesAfterRuntimeInfo.Count -ne 0) {
    throw "Default runtime-info retained an optional parser worker or broker."
}

$initResult = Invoke-BoundedProcess -FileName $runtimePath `
    -Arguments @("--format", "json", "init", "--no-scan") `
    -WorkingDirectory $repository `
    -Environment $runtimeEnvironment `
    -TimeoutSeconds 120
Assert-ProcessSucceeded -Result $initResult -Operation "isolated repository initialization"
$storageAbsentAfterInit = Test-OptionalStorageAbsent `
    -Path $optionalStorage `
    -Checkpoint "repository initialization"

$databasePath = Join-Path $repository ".projectatlas/projectatlas.db"
if (-not (Test-Path -LiteralPath $databasePath -PathType Leaf)) {
    throw "Repository initialization did not create the project database."
}

$defaultIdleMeasurement = Measure-McpIdleResidentSet `
    -Profile $defaultProfile `
    -State $defaultState `
    -DatabasePath $databasePath
$idleRssSamples = $defaultIdleMeasurement.samples
$storageAbsentDuringMcp = $defaultIdleMeasurement.storage_absent_during_mcp
$storageAbsentAfterShutdown = $defaultIdleMeasurement.storage_absent_after_shutdown

$controlStorageAbsentBeforeRuntimeInfo = Test-OptionalStorageAbsent `
    -Path $controlState.optional_storage `
    -Checkpoint "no-default-features control pre-runtime-info baseline"
$controlBaselineOptionalProcesses = @(Get-OptionalParserProcessIds)
if ($controlBaselineOptionalProcesses.Count -ne 0) {
    throw "The measurement host has an optional parser worker or broker before control runtime-info."
}
$controlRuntimeInfoSamples = @()
$controlRuntimeVersion = $null
for ($sample = 0; $sample -lt $runtimeInfoSampleCount; $sample++) {
    $controlRuntimeInfoResult = Invoke-BoundedProcess -FileName $controlProfile.runtime_path `
        -Arguments @("--format", "json", "runtime-info") `
        -WorkingDirectory $controlState.repository `
        -Environment $controlState.environment `
        -TimeoutSeconds 30
    Assert-ProcessSucceeded `
        -Result $controlRuntimeInfoResult `
        -Operation "no-default-features control runtime-info fresh-process sample"
    $controlRuntimeInfo = $controlRuntimeInfoResult.stdout | ConvertFrom-Json -Depth 32
    if ($null -eq $controlRuntimeInfo.version -or
        [string]::IsNullOrWhiteSpace([string]$controlRuntimeInfo.version)) {
        throw "Control runtime-info did not report the runtime version."
    }
    if ($null -eq $controlRuntimeVersion) {
        $controlRuntimeVersion = [string]$controlRuntimeInfo.version
    } elseif ($controlRuntimeVersion -ne [string]$controlRuntimeInfo.version) {
        throw "Control runtime-info version changed between fresh-process samples."
    }
    $controlRuntimeInfoSamples += [double]$controlRuntimeInfoResult.elapsed_milliseconds
}
if ($controlRuntimeVersion -ne $runtimeVersion) {
    throw "Default and control runtime versions differ."
}
$controlStorageAbsentAfterRuntimeInfo = Test-OptionalStorageAbsent `
    -Path $controlState.optional_storage `
    -Checkpoint "no-default-features control runtime-info samples"
$controlOptionalProcessesAfterRuntimeInfo = @(Get-OptionalParserProcessIds)
if ($controlOptionalProcessesAfterRuntimeInfo.Count -ne 0) {
    throw "Control runtime-info retained an optional parser worker or broker."
}
$controlInitialization = Initialize-RuntimeState `
    -Profile $controlProfile `
    -State $controlState
$controlIdleMeasurement = Measure-McpIdleResidentSet `
    -Profile $controlProfile `
    -State $controlState `
    -DatabasePath $controlInitialization.database_path

$defaultMcpReadySamples = @()
$controlMcpReadySamples = @()
for ($sample = 0; $sample -lt $mcpReadySampleCount; $sample++) {
    $order = if (($sample % 2) -eq 0) {
        @("default", "control")
    } else {
        @("control", "default")
    }
    foreach ($profileName in $order) {
        if ($profileName -eq "default") {
            $defaultMcpReadySamples += Measure-McpInitializeReady `
                -Profile $defaultProfile `
                -State $defaultState `
                -DatabasePath $databasePath
        } else {
            $controlMcpReadySamples += Measure-McpInitializeReady `
                -Profile $controlProfile `
                -State $controlState `
                -DatabasePath $controlInitialization.database_path
        }
    }
}

$finalRevisionResult = Invoke-BoundedProcess -FileName "git" `
    -Arguments @("rev-parse", "HEAD") `
    -WorkingDirectory $resolvedSourceRoot
Assert-ProcessSucceeded -Result $finalRevisionResult -Operation "final candidate revision lookup"
$finalStatusResult = Invoke-BoundedProcess -FileName "git" `
    -Arguments @("status", "--porcelain=v1", "--untracked-files=all") `
    -WorkingDirectory $resolvedSourceRoot
Assert-ProcessSucceeded -Result $finalStatusResult -Operation "final candidate cleanliness check"
if ($finalRevisionResult.stdout.Trim().ToLowerInvariant() -ne $candidateShaNormalized -or
    -not [string]::IsNullOrWhiteSpace($finalStatusResult.stdout)) {
    throw "Candidate source changed during default-runtime measurement."
}

$defaultStartupDistribution = Get-MeasurementDistribution `
    -Values ([double[]]$runtimeInfoSamples) `
    -RoundDigits 3 `
    -Unit "milliseconds"
$controlStartupDistribution = Get-MeasurementDistribution `
    -Values ([double[]]$controlRuntimeInfoSamples) `
    -RoundDigits 3 `
    -Unit "milliseconds"
$defaultMcpReadyDistribution = Get-MeasurementDistribution `
    -Values ([double[]]$defaultMcpReadySamples) `
    -RoundDigits 3 `
    -Unit "milliseconds"
$controlMcpReadyDistribution = Get-MeasurementDistribution `
    -Values ([double[]]$controlMcpReadySamples) `
    -RoundDigits 3 `
    -Unit "milliseconds"
$defaultIdleRssDistribution = Get-MeasurementDistribution `
    -Values ([double[]]$idleRssSamples) `
    -RoundDigits 0 `
    -Unit "bytes"
$controlIdleRssDistribution = Get-MeasurementDistribution `
    -Values $controlIdleMeasurement.samples `
    -RoundDigits 0 `
    -Unit "bytes"
$runtimeInfoStartupComparison = New-MaterialRegressionComparison `
    -DefaultMedian $defaultStartupDistribution.median `
    -ControlMedian $controlStartupDistribution.median `
    -AbsoluteTolerance $startupAbsoluteToleranceMilliseconds `
    -RelativeTolerancePercent $startupRelativeTolerancePercent `
    -Unit "milliseconds" `
    -RoundDigits 3
$mcpReadyStartupComparison = New-MaterialRegressionComparison `
    -DefaultMedian $defaultMcpReadyDistribution.median `
    -ControlMedian $controlMcpReadyDistribution.median `
    -AbsoluteTolerance $startupAbsoluteToleranceMilliseconds `
    -RelativeTolerancePercent $startupRelativeTolerancePercent `
    -Unit "milliseconds" `
    -RoundDigits 3
$idleRssComparison = New-MaterialRegressionComparison `
    -DefaultMedian $defaultIdleRssDistribution.median `
    -ControlMedian $controlIdleRssDistribution.median `
    -AbsoluteTolerance ([double]$idleRssAbsoluteToleranceBytes) `
    -RelativeTolerancePercent $idleRssRelativeTolerancePercent `
    -Unit "bytes" `
    -RoundDigits 0
$startupGatePassed = $runtimeInfoStartupComparison.passed -and $mcpReadyStartupComparison.passed
$materialRegressionGatePassed = $startupGatePassed -and $idleRssComparison.passed
$conclusion = if ($materialRegressionGatePassed) {
    "no_material_absent_pack_startup_or_idle_rss_regression"
} else {
    "material_absent_pack_regression_detected"
}

$report = [ordered]@{
    schema = "projectatlas-default-runtime-optional-pack-boundary-v3"
    candidate_sha = $candidateShaNormalized
    platform = [ordered]@{
        target = $Target
        operating_system = if ($isWindowsHost) { "windows" } else { "linux" }
        architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    }
    toolchain = [ordered]@{
        rustc_release = $rustcRelease
        rustc_commit = $rustcCommit
        rustc_host = $rustcHost
        cargo_version = $cargoResult.stdout.Trim()
    }
    comparison = [ordered]@{
        candidate = [ordered]@{
            name = $defaultProfile.name
            cargo_profile = "release"
            cargo_features = $defaultProfile.cargo_features
            isolated_target_directory = $true
            isolated_home_and_repository = $true
        }
        control = [ordered]@{
            name = $controlProfile.name
            cargo_profile = "release"
            cargo_features = $controlProfile.cargo_features
            cargo_arguments = $controlProfile.feature_arguments
            isolated_target_directory = $true
            isolated_home_and_repository = $true
        }
        same_host = $true
        same_candidate = $true
        same_target = $true
        same_toolchain = $true
    }
    runtime = [ordered]@{
        version = $runtimeVersion
        cargo_profile = "release"
    }
    measurements = [ordered]@{
        runtime_binary = [ordered]@{
            default_features = [ordered]@{
                value = $defaultProfile.runtime_binary_bytes
                sha256 = $defaultProfile.runtime_binary_sha256
            }
            no_default_features_control = [ordered]@{
                value = $controlProfile.runtime_binary_bytes
                sha256 = $controlProfile.runtime_binary_sha256
            }
            absolute_delta = $defaultProfile.runtime_binary_bytes - $controlProfile.runtime_binary_bytes
            unit = "bytes"
            required_minimum = 1
            declared_maximum = $null
        }
        optional_pack_archive = [ordered]@{
            value = [long]$archive.Length
            unit = "bytes"
            sha256 = $archiveSha256
            required_minimum = 1
            declared_maximum = $archiveMaximumBytes
        }
        runtime_info_fresh_process_startup_wall_clock = [ordered]@{
            default_features = $defaultStartupDistribution
            no_default_features_control = $controlStartupDistribution
            delta_and_gate = $runtimeInfoStartupComparison
            process_state = "new_process_per_sample"
            collection_order = "default_features_then_no_default_features_control"
            operating_system_cache_state = "uncontrolled"
        }
        mcp_initialize_ready_wall_clock = [ordered]@{
            default_features = $defaultMcpReadyDistribution
            no_default_features_control = $controlMcpReadyDistribution
            delta_and_gate = $mcpReadyStartupComparison
            process_state = "new_process_per_sample"
            readiness_boundary = "process_launch_to_valid_initialize_response"
            collection_order = "alternating_default_control_per_pair"
            operating_system_cache_state = "uncontrolled"
        }
        mcp_idle_resident_set = [ordered]@{
            default_features = $defaultIdleRssDistribution
            no_default_features_control = $controlIdleRssDistribution
            delta_and_gate = $idleRssComparison
            collection_order = "default_features_then_no_default_features_control"
            stabilization = $idleStabilizationMilliseconds
            stabilization_unit = "milliseconds"
            interval = $idleSampleIntervalMilliseconds
            interval_unit = "milliseconds"
            initialized_before_stabilization = $true
            stdin_open_during_samples = $true
        }
    }
    material_regression_gate = [ordered]@{
        preregistered = $true
        runtime_info_startup_passed = $runtimeInfoStartupComparison.passed
        mcp_initialize_ready_startup_passed = $mcpReadyStartupComparison.passed
        startup_passed = $startupGatePassed
        idle_rss_passed = $idleRssComparison.passed
        passed = $materialRegressionGatePassed
        conclusion = $conclusion
        claim = "no material absent-pack startup or idle-RSS regression"
    }
    runtime_surfaces = [ordered]@{
        dependency_edges_inspected = @("normal", "build")
        default_features = [ordered]@{
            forbidden_dependencies_present = $defaultProfile.forbidden_dependencies_present
            parser_worker_binary_shipped = $defaultProfile.parser_worker_binary_shipped
        }
        no_default_features_control = [ordered]@{
            forbidden_dependencies_present = $controlProfile.forbidden_dependencies_present
            parser_worker_binary_shipped = $controlProfile.parser_worker_binary_shipped
        }
    }
    absent_pack_observations = [ordered]@{
        default_features = [ordered]@{
            optional_worker_or_broker_processes_present_before_startup = 0
            optional_worker_or_broker_processes_present_after_runtime_info = 0
            optional_worker_or_broker_processes_present_during_idle_mcp = 0
            optional_worker_or_broker_processes_present_after_mcp_shutdown = 0
            storage_absent_before_startup = $storageAbsentBeforeStartup
            storage_absent_after_runtime_info = $storageAbsentAfterRuntimeInfo
            database_created_by_init = $true
            storage_absent_after_init = $storageAbsentAfterInit
            storage_absent_during_idle_mcp = $storageAbsentDuringMcp
            storage_absent_after_mcp_shutdown = $storageAbsentAfterShutdown
        }
        no_default_features_control = [ordered]@{
            optional_worker_or_broker_processes_present_before_startup = 0
            optional_worker_or_broker_processes_present_after_runtime_info = 0
            optional_worker_or_broker_processes_present_during_idle_mcp = $controlIdleMeasurement.optional_worker_or_broker_processes_present_during_idle_mcp
            optional_worker_or_broker_processes_present_after_mcp_shutdown = $controlIdleMeasurement.optional_worker_or_broker_processes_present_after_mcp_shutdown
            storage_absent_before_runtime_info = $controlStorageAbsentBeforeRuntimeInfo
            storage_absent_after_runtime_info = $controlStorageAbsentAfterRuntimeInfo
            database_created_by_init = $controlInitialization.database_created
            storage_absent_after_init = $controlInitialization.storage_absent_after_init
            storage_absent_before_mcp_startup = $controlIdleMeasurement.storage_absent_before_startup
            storage_absent_during_idle_mcp = $controlIdleMeasurement.storage_absent_during_mcp
            storage_absent_after_mcp_shutdown = $controlIdleMeasurement.storage_absent_after_shutdown
        }
    }
}

[System.IO.File]::WriteAllText(
    $OutputPath,
    (($report | ConvertTo-Json -Depth 16) + "`n"),
    [System.Text.UTF8Encoding]::new($false)
)
$report | ConvertTo-Json -Depth 16 -Compress
if (-not $materialRegressionGatePassed) {
    throw "Material absent-pack startup or idle-RSS regression exceeded the preregistered tolerance."
}
