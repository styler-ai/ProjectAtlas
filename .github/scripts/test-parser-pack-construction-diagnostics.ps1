[CmdletBinding()]
param(
    [string]$ProductionScript = (Join-Path $PSScriptRoot "run-parser-pack-contained-construction.ps1"),
    [string]$WindowsWrapper = (Join-Path $PSScriptRoot "invoke-parser-pack-windows-construction.ps1"),
    [string]$WindowsRunnerJobBroker =
        (Join-Path $PSScriptRoot "invoke-parser-pack-windows-runner-job-broker.ps1"),
    [string]$WindowsRecovery =
        (Join-Path $PSScriptRoot "test-parser-pack-windows-recovery.ps1")
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

function Invoke-BoundedDiagnosticChild {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.ProcessStartInfo]$StartInfo,

        [Parameter(Mandatory = $true)]
        [ValidateRange(1, 60000)]
        [int]$OperationTimeoutMilliseconds
    )

    Require `
        (-not $StartInfo.UseShellExecute -and
            $StartInfo.RedirectStandardOutput -and
            $StartInfo.RedirectStandardError) `
        "Bounded diagnostic child requires redirected pipes."
    $receipt = [pscustomobject]@{
        TimedOut = $false
        ReapedBeforePipeCollection = $false
        PipeCompleted = $false
        Disposed = $false
        ExitCode = -1
        StandardOutput = ''
        StandardError = ''
    }
    $process = $null
    $operationFailure = $null
    try {
        $process = [System.Diagnostics.Process]::Start($StartInfo)
        if ($null -eq $process) {
            throw "Could not start bounded diagnostic child."
        }
        $outputTask = $process.StandardOutput.ReadToEndAsync()
        $errorTask = $process.StandardError.ReadToEndAsync()
        $finished = $process.WaitForExit($OperationTimeoutMilliseconds)
        if (-not $finished) {
            $receipt.TimedOut = $true
            $process.Kill($true)
            if (-not $process.WaitForExit(5000)) {
                throw "Timed-out diagnostic child could not be reaped."
            }
        }
        $receipt.ReapedBeforePipeCollection = $process.HasExited
        if (-not $receipt.ReapedBeforePipeCollection) {
            throw "Diagnostic child was not reaped before pipe collection."
        }
        $pipeTasks = [System.Threading.Tasks.Task[]]@($outputTask, $errorTask)
        $receipt.PipeCompleted = [System.Threading.Tasks.Task]::WaitAll($pipeTasks, 5000)
        if (-not $receipt.PipeCompleted) {
            throw "Diagnostic child pipes did not close inside the fixed deadline."
        }
        $receipt.StandardOutput = $outputTask.Result
        $receipt.StandardError = $errorTask.Result
        if ($receipt.StandardOutput.Length -gt 4096 -or
            $receipt.StandardError.Length -gt 4096) {
            throw "Diagnostic child exceeded its fixed pipe-output limit."
        }
        $receipt.ExitCode = $process.ExitCode
    }
    catch {
        $operationFailure = $_.Exception
    }
    finally {
        if ($null -ne $process) {
            try {
                if (-not $process.HasExited) {
                    $process.Kill($true)
                    if (-not $process.WaitForExit(5000)) {
                        throw "Fallback diagnostic child termination could not be reaped."
                    }
                }
            }
            finally {
                $process.Dispose()
                $receipt.Disposed = $true
            }
        }
    }
    if ($null -ne $operationFailure) {
        throw $operationFailure
    }
    return $receipt
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
    "Write-BoundedConstructionDiagnostic",
    "Invoke-Checked",
    "Write-ConstructionStatus",
    "Assert-CargoConstructionEnvironment"
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
    $hadBuildJobs = Test-Path -LiteralPath Env:CARGO_BUILD_JOBS
    $previousBuildJobs = [string]$env:CARGO_BUILD_JOBS
    $hadMakeflags = Test-Path -LiteralPath Env:CARGO_MAKEFLAGS
    $previousMakeflags = [string]$env:CARGO_MAKEFLAGS
    try {
        foreach ($invalidEnvironment in @(
            [pscustomobject]@{ Jobs = $null; Makeflags = $null },
            [pscustomobject]@{ Jobs = '2'; Makeflags = $null },
            [pscustomobject]@{ Jobs = '1'; Makeflags = '--jobserver-auth=obsolete' }
        )) {
            if ($null -eq $invalidEnvironment.Jobs) {
                Remove-Item -LiteralPath Env:CARGO_BUILD_JOBS -ErrorAction SilentlyContinue
            }
            else {
                $env:CARGO_BUILD_JOBS = $invalidEnvironment.Jobs
            }
            if ($null -eq $invalidEnvironment.Makeflags) {
                Remove-Item -LiteralPath Env:CARGO_MAKEFLAGS -ErrorAction SilentlyContinue
            }
            else {
                $env:CARGO_MAKEFLAGS = $invalidEnvironment.Makeflags
            }
            $rejected = $false
            try {
                Assert-CargoConstructionEnvironment -Target 'x86_64-pc-windows-msvc'
            }
            catch {
                $rejected = $_.Exception.Message -eq
                    'Windows construction must create its Cargo jobserver inside the contained child.'
            }
            Require $rejected "Windows construction accepted an invalid Cargo environment."
        }
        $env:CARGO_BUILD_JOBS = '1'
        Remove-Item -LiteralPath Env:CARGO_MAKEFLAGS -ErrorAction SilentlyContinue
        Assert-CargoConstructionEnvironment -Target 'x86_64-pc-windows-msvc'
        Remove-Item -LiteralPath Env:CARGO_BUILD_JOBS -ErrorAction SilentlyContinue
        Assert-CargoConstructionEnvironment -Target 'x86_64-unknown-linux-gnu'
    }
    finally {
        if ($hadBuildJobs) { $env:CARGO_BUILD_JOBS = $previousBuildJobs }
        else { Remove-Item -LiteralPath Env:CARGO_BUILD_JOBS -ErrorAction SilentlyContinue }
        if ($hadMakeflags) { $env:CARGO_MAKEFLAGS = $previousMakeflags }
        else { Remove-Item -LiteralPath Env:CARGO_MAKEFLAGS -ErrorAction SilentlyContinue }
    }
    if ($env:OS -eq "Windows_NT") {
        $wrapperTokens = $null
        $wrapperParseErrors = $null
        $wrapperAst = [System.Management.Automation.Language.Parser]::ParseFile(
            (Get-Item -LiteralPath $WindowsWrapper -Force).FullName,
            [ref]$wrapperTokens,
            [ref]$wrapperParseErrors
        )
        Require ($wrapperParseErrors.Count -eq 0) "Windows construction wrapper did not parse."
        $wrapperText = $wrapperAst.Extent.Text
        $brokerTokens = $null
        $brokerParseErrors = $null
        $brokerAst = [System.Management.Automation.Language.Parser]::ParseFile(
            (Get-Item -LiteralPath $WindowsRunnerJobBroker -Force).FullName,
            [ref]$brokerTokens,
            [ref]$brokerParseErrors
        )
        Require ($brokerParseErrors.Count -eq 0) "Windows runner Job broker did not parse."
        $brokerText = $brokerAst.Extent.Text
        $recoveryTokens = $null
        $recoveryParseErrors = $null
        $recoveryAst = [System.Management.Automation.Language.Parser]::ParseFile(
            (Get-Item -LiteralPath $WindowsRecovery -Force).FullName,
            [ref]$recoveryTokens,
            [ref]$recoveryParseErrors
        )
        Require ($recoveryParseErrors.Count -eq 0) "Windows recovery script did not parse."
        $probeAssignments = @($recoveryAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$namespaceProbeSource'
            },
            $true
        ))
        Require ($probeAssignments.Count -eq 1) "Expected one Windows named-object probe source."
        $probeSource = [string]$probeAssignments[0].Right.Expression.Value
        $probeTokens = $null
        $probeParseErrors = $null
        [void][System.Management.Automation.Language.Parser]::ParseInput(
            $probeSource,
            [ref]$probeTokens,
            [ref]$probeParseErrors
        )
        Require ($probeParseErrors.Count -eq 0) "Windows named-object probe did not parse."
        $canaryAssignments = @($recoveryAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$namespaceCanarySource'
            },
            $true
        ))
        Require ($canaryAssignments.Count -eq 1) "Expected one Windows named-object canary source."
        $canarySource = [string]$canaryAssignments[0].Right.Expression.Value
        $canaryTokens = $null
        $canaryParseErrors = $null
        [void][System.Management.Automation.Language.Parser]::ParseInput(
            $canarySource,
            [ref]$canaryTokens,
            [ref]$canaryParseErrors
        )
        Require ($canaryParseErrors.Count -eq 0) "Windows named-object canary did not parse."
        foreach ($functionName in @(
            'Test-ExactJsonInteger',
            'Test-ExactJsonString',
            'Test-ExactJsonBoolean',
            'Test-BoundedProbeError',
            'Test-BoundedProbeErrorsEqual',
            'Test-DefaultSecuritySemaphoreProbe',
            'Read-NamedObjectProbeRecord',
            'Format-NamedObjectProbeFailure',
            'Remove-NamedObjectProbeTemporaryRecords'
        )) {
            $functionDefinitions = @($recoveryAst.FindAll(
                {
                    param($node)
                    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                        $node.Name -eq $functionName
                },
                $true
            ))
            Require `
                ($functionDefinitions.Count -eq 1) `
                "Expected one $functionName recovery diagnostic function."
            Invoke-Expression $functionDefinitions[0].Extent.Text
        }
        $brokerParameters = @($brokerAst.ParamBlock.Parameters |
            ForEach-Object { $_.Name.VariablePath.UserPath })
        $brokerJoinIndex = $brokerText.IndexOf(
            '[ProjectAtlasWindowsRunnerJob]::Join($BrokerJobName)',
            [System.StringComparison]::Ordinal
        )
        $brokerReadyIndex = $brokerText.IndexOf(
            "kind = 'ready'",
            $brokerJoinIndex,
            [System.StringComparison]::Ordinal
        )
        $brokerRequestIndex = $brokerText.IndexOf(
            '$request = Read-BrokerFrame',
            $brokerReadyIndex,
            [System.StringComparison]::Ordinal
        )
        $brokerTargetIndex = $brokerText.IndexOf(
            '& $targetItem.FullName @targetParameters',
            $brokerRequestIndex,
            [System.StringComparison]::Ordinal
        )
        Require `
            ('ScriptPath' -notin $brokerParameters -and
                'Command' -notin $brokerParameters -and
                'Arguments' -notin $brokerParameters -and
                $brokerText.Contains("[ValidateSet('construction', 'recovery')]") -and
                $brokerText.Contains('CreateFlags = [uint32](0x01000000 -bor 0x08000000)') -and
                $brokerText.Contains('JobObjectLimitKillOnJobClose | JobObjectLimitBreakawayOk') -and
                $brokerText.Contains('limits.BasicLimitInformation.LimitFlags != expected') -and
                $brokerText.Contains('JobObjectLimitSilentBreakawayOk') -and
                $brokerText.Contains('JobObjectBasicUiRestrictions = 4') -and
                $brokerText.Contains('if (uiRestrictions != 0)') -and
                $brokerText.Contains('ValidatePolicy(handle);') -and
                $brokerText.Contains('GetNamedPipeClientProcessId') -and
                $brokerText.Contains('GetNamedPipeServerProcessId') -and
                $brokerText.Contains('$maximumDiagnosticCharacters = 12 * 1024') -and
                $brokerText.Contains('[char]::IsHighSurrogate(') -and
                $brokerText.Contains('TerminateExactProcess(') -and
                $brokerText.Contains('[System.IO.Pipes.NamedPipeServerStreamAcl]::Create(') -and
                $brokerText.Contains('$security.SetAccessRuleProtection($true, $false)') -and
                $brokerJoinIndex -ge 0 -and
                $brokerReadyIndex -gt $brokerJoinIndex -and
                $brokerRequestIndex -gt $brokerReadyIndex -and
                $brokerTargetIndex -gt $brokerRequestIndex) `
            "Windows runner Job broker lost its fixed authenticated admission boundary."

        $boundedDiagnosticText =
            ${function:Invoke-BoundedDiagnosticChild}.Ast.Extent.Text
        $reapBeforePipeIndex = $boundedDiagnosticText.IndexOf(
            '$receipt.ReapedBeforePipeCollection = $process.HasExited',
            [System.StringComparison]::Ordinal
        )
        $pipeDeadlineIndex = $boundedDiagnosticText.IndexOf(
            '[System.Threading.Tasks.Task]::WaitAll($pipeTasks, 5000)',
            $reapBeforePipeIndex,
            [System.StringComparison]::Ordinal
        )
        $pipeCollectionIndex = $boundedDiagnosticText.IndexOf(
            '$receipt.StandardOutput = $outputTask.Result',
            $pipeDeadlineIndex,
            [System.StringComparison]::Ordinal
        )
        $disposeIndex = $boundedDiagnosticText.IndexOf(
            '$process.Dispose()',
            $pipeCollectionIndex,
            [System.StringComparison]::Ordinal
        )
        Require `
            ($reapBeforePipeIndex -ge 0 -and
                $pipeDeadlineIndex -gt $reapBeforePipeIndex -and
                $pipeCollectionIndex -gt $pipeDeadlineIndex -and
                $disposeIndex -gt $pipeCollectionIndex -and
                $boundedDiagnosticText.Contains('finally {')) `
            "Bounded diagnostic child changed reap, pipe, or disposal ordering."

        $timeoutStart = [System.Diagnostics.ProcessStartInfo]::new()
        $timeoutStart.FileName =
            [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
        $timeoutStart.UseShellExecute = $false
        $timeoutStart.CreateNoWindow = $true
        $timeoutStart.RedirectStandardOutput = $true
        $timeoutStart.RedirectStandardError = $true
        foreach ($argument in @(
            '-NoLogo', '-NoProfile', '-NonInteractive',
            '-Command', 'Start-Sleep -Seconds 30'
        )) {
            $timeoutStart.ArgumentList.Add($argument)
        }
        $timeoutReceipt = Invoke-BoundedDiagnosticChild `
            -StartInfo $timeoutStart `
            -OperationTimeoutMilliseconds 100
        Require `
            ($timeoutReceipt.TimedOut -and
                $timeoutReceipt.ReapedBeforePipeCollection -and
                $timeoutReceipt.PipeCompleted -and
                $timeoutReceipt.Disposed -and
                $timeoutReceipt.StandardOutput.Length -eq 0 -and
                $timeoutReceipt.StandardError.Length -eq 0) `
            "Diagnostic timeout did not reap, close pipes, and dispose in order."

        $probeId = [Guid]::NewGuid().ToString('N')
        $probeScript = [System.IO.Path]::Combine(
            $testRoot,
            "projectatlas-object-namespace-probe-$probeId.ps1"
        )
        $probeResult = [System.IO.Path]::Combine(
            $testRoot,
            "projectatlas-object-namespace-probe-$probeId.json"
        )
        $probeCanary = [System.IO.Path]::Combine(
            $testRoot,
            "projectatlas-object-namespace-canary-$probeId.ps1"
        )
        [System.IO.File]::WriteAllText(
            $probeScript,
            $probeSource,
            [System.Text.UTF8Encoding]::new($false)
        )
        [System.IO.File]::WriteAllText(
            $probeCanary,
            $canarySource,
            [System.Text.UTF8Encoding]::new($false)
        )
        $diagnosticSeedName =
            "Local\ProjectAtlasParserPack-$([Guid]::NewGuid().ToString('N'))"
        $diagnosticSeedRights =
            [System.Security.AccessControl.SemaphoreRights]::Synchronize -bor
            [System.Security.AccessControl.SemaphoreRights]::Modify
        $diagnosticSeedSecurity =
            [System.Security.AccessControl.SemaphoreSecurity]::new()
        $diagnosticSeedSecurity.SetAccessRuleProtection($true, $false)
        $diagnosticSeedSecurity.AddAccessRule(
            [System.Security.AccessControl.SemaphoreAccessRule]::new(
                [System.Security.Principal.WindowsIdentity]::GetCurrent().User,
                $diagnosticSeedRights,
                [System.Security.AccessControl.AccessControlType]::Allow
            )
        )
        $diagnosticSeedCreatedNew = $false
        $diagnosticSeed = [System.Threading.SemaphoreAcl]::Create(
            1,
            1,
            $diagnosticSeedName,
            [ref]$diagnosticSeedCreatedNew,
            $diagnosticSeedSecurity
        )
        Require $diagnosticSeedCreatedNew "Named-object diagnostic seed collided."
        $probeStart = [System.Diagnostics.ProcessStartInfo]::new()
        $probeStart.FileName = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
        $probeStart.UseShellExecute = $false
        $probeStart.CreateNoWindow = $true
        $probeStart.RedirectStandardOutput = $true
        $probeStart.RedirectStandardError = $true
        $probeStart.Environment['CARGO_MAKEFLAGS'] = '--jobserver-auth=forbidden'
        foreach ($argument in @(
            '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
            '-File', $probeScript,
            '-ExpectedPrincipalSid',
            [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value,
            '-ExpectedOwnerSid',
            [System.Security.Principal.WindowsIdentity]::GetCurrent().Owner.Value,
            '-ResultPath', $probeResult,
            '-CanaryPath', $probeCanary,
            '-SeededSemaphoreName', $diagnosticSeedName
        )) {
            $probeStart.ArgumentList.Add($argument)
        }
        $probeReceipt = Invoke-BoundedDiagnosticChild `
            -StartInfo $probeStart `
            -OperationTimeoutMilliseconds 15000
        Require `
            (-not $probeReceipt.TimedOut -and
                $probeReceipt.ReapedBeforePipeCollection -and
                $probeReceipt.PipeCompleted -and
                $probeReceipt.Disposed -and
                $probeReceipt.ExitCode -eq 122 -and
                $probeReceipt.StandardOutput.Length -eq 0 -and
                $probeReceipt.StandardError.Length -le 1024) `
            "Named-object diagnostic fault did not return its stable bounded exit."
        $probeResultItem = Get-Item -LiteralPath $probeResult -Force
        Require `
            (-not $probeResultItem.PSIsContainer -and
                (($probeResultItem.Attributes -band
                    [System.IO.FileAttributes]::ReparsePoint) -eq 0) -and
                $probeResultItem.Length -ge 1 -and
                $probeResultItem.Length -le 4096) `
            "Named-object diagnostic fault did not atomically publish one bounded record."
        $probeRecord = Read-NamedObjectProbeRecord -Path $probeResultItem.FullName
        Require `
            ($probeRecord.schema_version -eq 6L -and
                $probeRecord.status -ceq 'failure' -and
                $probeRecord.stage -ceq 'ambient-environment' -and
                $probeRecord.exit_code -eq 122L -and
                $probeRecord.error.type -ceq 'InvalidOperationException' -and
                $null -eq $probeRecord.error.native_code -and
                $probeRecord.error.message -ceq 'ambient-cargo-makeflags' -and
                $probeRecord.operation_stage -ceq 'ambient-environment' -and
                $probeRecord.operation_error.message -ceq
                    'ambient-cargo-makeflags' -and
                $null -eq $probeRecord.cleanup_error -and
                @(Get-ChildItem -LiteralPath $testRoot -Force -File |
                    Where-Object Name -like "$([System.IO.Path]::GetFileName($probeResult)).tmp-*").Count -eq 0) `
            "Named-object diagnostic fault omitted its bounded stage or error."

        $stringSchemaRecord = [System.IO.Path]::Combine(
            $testRoot,
            "projectatlas-object-namespace-probe-$([Guid]::NewGuid().ToString('N')).json"
        )
        $stringSchemaPayload = [System.IO.File]::ReadAllText($probeResult) |
            ConvertFrom-Json -Depth 8
        $stringSchemaPayload.schema_version = '6'
        [System.IO.File]::WriteAllText(
            $stringSchemaRecord,
            ($stringSchemaPayload | ConvertTo-Json -Depth 8 -Compress)
        )
        $stringSchemaRejected = $false
        try {
            [void](Read-NamedObjectProbeRecord -Path $stringSchemaRecord)
        }
        catch {
            $stringSchemaRejected = $_.Exception.Message -eq
                'Named-object probe diagnostic field types or values were invalid.'
        }
        Require `
            $stringSchemaRejected `
            "Named-object diagnostic reader coerced a string schema version."

        $stringBooleanRecord = [System.IO.Path]::Combine(
            $testRoot,
            "projectatlas-object-namespace-probe-$([Guid]::NewGuid().ToString('N')).json"
        )
        $stringBooleanPayload = [ordered]@{
            schema_version = 6
            status = 'success'
            stage = 'complete'
            exit_code = 0
            error = $null
            operation_stage = $null
            operation_error = $null
            cleanup_error = $null
            current_private_namespace_enabled = 'false'
            session_id = 1
            directory_path = '\Sessions\1\BaseNamedObjects'
            directory_traverse_ntstatus = 0
            directory_create_object_ntstatus = 0
            directory_traverse_create_ntstatus = 0
            session_directory_traverse_ntstatus = 0
            native_semaphore_name =
                "Local\ProjectAtlasParserPack-$([Guid]::NewGuid().ToString('N'))"
            post_job_native_create_win32 = 0
            post_job_native_created_new = $true
            post_job_native_close_win32 = 0
            seeded_semaphore_name = $diagnosticSeedName
            seeded_direct_open_ntstatus = 0
            seeded_direct_open_close_ntstatus = 0
            seeded_relative_open_ntstatus = 0
            seeded_relative_open_close_ntstatus = 0
            seeded_relative_directory_close_ntstatus = 0
            seeded_open_win32 = 0
            seeded_open_close_win32 = 0
            seeded_create_win32 = 183
            seeded_create_created_new = $false
            seeded_create_close_win32 = 0
            semaphore_name = $diagnosticSeedName
            created_new = $false
            descendant_exit_code = 0
        }
        [System.IO.File]::WriteAllText(
            $stringBooleanRecord,
            ($stringBooleanPayload | ConvertTo-Json -Depth 8 -Compress)
        )
        $stringBooleanRejected = $false
        try {
            [void](Read-NamedObjectProbeRecord -Path $stringBooleanRecord)
        }
        catch {
            $stringBooleanRejected = $_.Exception.Message -eq
                'Named-object probe diagnostic field types or values were invalid.'
        }
        Require `
            $stringBooleanRejected `
            "Named-object diagnostic reader coerced a string private namespace Boolean."

        $mismatchedRelationshipRecord = [System.IO.Path]::Combine(
            $testRoot,
            "projectatlas-object-namespace-probe-$([Guid]::NewGuid().ToString('N')).json"
        )
        $mismatchedRelationshipPayload = [System.IO.File]::ReadAllText($probeResult) |
            ConvertFrom-Json -Depth 8
        $mismatchedRelationshipPayload.operation_stage = 'identity'
        [System.IO.File]::WriteAllText(
            $mismatchedRelationshipRecord,
            ($mismatchedRelationshipPayload | ConvertTo-Json -Depth 8 -Compress)
        )
        $mismatchedRelationshipRejected = $false
        try {
            [void](Read-NamedObjectProbeRecord -Path $mismatchedRelationshipRecord)
        }
        catch {
            $mismatchedRelationshipRejected = $_.Exception.Message -eq
                'Named-object probe diagnostic stage, exit, or error relationship was invalid.'
        }
        Require `
            $mismatchedRelationshipRejected `
            "Named-object diagnostic reader accepted mismatched stage ownership."

        foreach ($faultRow in @(
            [pscustomobject]@{
                Name = 'combined'
                Fault = 'operation-and-cleanup'
                ExitCode = 129
            },
            [pscustomobject]@{
                Name = 'native'
                Fault = 'descendant-open-not-found'
                ExitCode = 127
            }
        )) {
            $faultId = [Guid]::NewGuid().ToString('N')
            $faultResult = [System.IO.Path]::Combine(
                $testRoot,
                "projectatlas-object-namespace-probe-$faultId.json"
            )
            $faultStart = [System.Diagnostics.ProcessStartInfo]::new()
            $faultStart.FileName =
                [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
            $faultStart.UseShellExecute = $false
            $faultStart.CreateNoWindow = $true
            $faultStart.RedirectStandardOutput = $true
            $faultStart.RedirectStandardError = $true
            [void]$faultStart.Environment.Remove('CARGO_MAKEFLAGS')
            foreach ($argument in @(
                '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                '-File', $probeScript,
                '-ExpectedPrincipalSid',
                [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value,
                '-ExpectedOwnerSid',
                [System.Security.Principal.WindowsIdentity]::GetCurrent().Owner.Value,
                '-ResultPath', $faultResult,
                '-CanaryPath', $probeCanary,
                '-SeededSemaphoreName', $diagnosticSeedName,
                '-DiagnosticFault', $faultRow.Fault
            )) {
                $faultStart.ArgumentList.Add($argument)
            }
            $faultReceipt = Invoke-BoundedDiagnosticChild `
                -StartInfo $faultStart `
                -OperationTimeoutMilliseconds 15000
            Require `
                (-not $faultReceipt.TimedOut -and
                    $faultReceipt.ReapedBeforePipeCollection -and
                    $faultReceipt.PipeCompleted -and
                    $faultReceipt.Disposed -and
                    $faultReceipt.ExitCode -eq $faultRow.ExitCode) `
                "Named-object $($faultRow.Name) fault lost its stable process boundary."
            $faultRecord = Read-NamedObjectProbeRecord -Path $faultResult
            if ($faultRow.Name -eq 'combined') {
                $combinedMessage = Format-NamedObjectProbeFailure `
                    -Record $faultRecord `
                    -ProcessExitCode $faultReceipt.ExitCode
                Require `
                    ($faultRecord.stage -ceq 'cleanup' -and
                        $faultRecord.operation_stage -ceq 'semaphore-acl' -and
                        $faultRecord.operation_error.message -ceq
                            'diagnostic-operation-fault ordinary/token <path> <path>' -and
                        $faultRecord.cleanup_error.message -ceq
                            'diagnostic-cleanup-fault ordinary\token <path> <path> <path> <path>' -and
                        $combinedMessage -match
                            'operation_error_type=InvalidOperationException' -and
                        $combinedMessage -match
                            'cleanup_error_type=InvalidOperationException' -and
                        $combinedMessage -match
                            'operation_message=diagnostic-operation-fault ordinary/token <path> <path>' -and
                        $combinedMessage -match
                            'cleanup_message=diagnostic-cleanup-fault ordinary\\token <path> <path> <path> <path>' -and
                        $combinedMessage -notmatch
                            '(?i)(?:[A-Z]:[\\/]|[\\/]{2})') `
                    "Combined probe failure did not preserve both redacted causes."
            }
            else {
                Require `
                    ($faultRecord.stage -ceq 'descendant-open' -and
                        $faultRecord.error.type -ceq 'Win32Exception' -and
                        $faultRecord.error.native_code -eq 2L -and
                        $faultRecord.error.message -match
                            '^descendant-open-exit-143') `
                    "Descendant OpenSemaphore failure lost its native error code."
            }
        }

        $diagnosticSeed.Dispose()

        $temporarySurvivor = "$probeResult.tmp-$([Guid]::NewGuid().ToString('N'))"
        $temporaryDecoy = "$probeResult.tmp-not-owned"
        [System.IO.File]::WriteAllText($temporarySurvivor, '{}')
        [System.IO.File]::WriteAllText($temporaryDecoy, '{}')
        Remove-NamedObjectProbeTemporaryRecords `
            -ResultPath $probeResult `
            -ExpectedParent $testRoot
        Require `
            (-not [System.IO.File]::Exists($temporarySurvivor) -and
                [System.IO.File]::Exists($temporaryDecoy)) `
            "Named-object temporary cleanup removed outside its exact owned prefix."

        $bootstrapFailure = $null
        try {
            & $WindowsRunnerJobBroker `
                -TargetKind recovery `
                -TargetParameters @{
                    ProductionWrapper = $WindowsWrapper
                    StaticOnly = $true
                } `
                -TimeoutSeconds 60 `
                -BootstrapTestFault hold-before-join
            throw 'Broker pre-Join fault unexpectedly succeeded.'
        }
        catch {
            $bootstrapFailure = $_.Exception.Message
        }
        Require `
            ($bootstrapFailure -eq 'broker-ready-receipt') `
            "Windows runner Job broker returned the wrong pre-Join fault."
        $bootstrapSurvivors = @(
            Get-CimInstance Win32_Process -Filter "Name = 'pwsh.exe'" |
                Where-Object {
                    [int]$_.ProcessId -ne $PID -and
                    [string]$_.CommandLine -like '*-BrokerChild*' -and
                    [string]$_.CommandLine -like
                        '*-BootstrapTestFault hold-before-join*'
                }
        )
        Require `
            ($bootstrapSurvivors.Count -eq 0) `
            "Windows runner Job broker retained its pre-Join WMI process."

        $escapedPathTail = (('路径\"' * 700) -join '')
        $escapedMissingWrapper = "$env:TEMP\missing-$escapedPathTail.ps1"
        $targetFailure = $null
        try {
            & $WindowsRunnerJobBroker `
                -TargetKind recovery `
                -TargetParameters @{
                    ProductionWrapper = $escapedMissingWrapper
                    StaticOnly = $true
                } `
                -TimeoutSeconds 60
            throw 'Broker escaped diagnostic fault unexpectedly succeeded.'
        }
        catch {
            $targetFailure = $_.Exception.Message
        }
        Require `
            ($targetFailure -match '^broker-target-failed:' -and
                $targetFailure -notmatch 'broker-frame-size|broker-pipe-closed') `
            "Windows runner Job broker lost one escaped Unicode target failure."
        $productionText = $ast.Extent.Text
        Require `
            ([System.Text.RegularExpressions.Regex]::Matches(
                $wrapperText,
                'CARGO_BUILD_JOBS = "1"'
            ).Count -eq 1 -and
                $productionText.Contains('[string]$env:CARGO_BUILD_JOBS -cne "1"') -and
                $productionText.Contains('Open-ContainedCargoJobserver') -and
                $productionText.Contains('Invoke-ContainedCargoJobserverCanary') -and
                $productionText.Contains('EntryPoint = "OpenSemaphoreW"') -and
                $productionText.Contains('SynchronizeAndModify = 0x00100002') -and
                $productionText.Contains('OpenExisting(string name)') -and
                $productionText.Contains('RequireExistingObject(string name)') -and
                $productionText.Contains('contained-cargo-jobserver-seed-missing') -and
                $productionText.Contains('$script:constructionJobserverName = $SeededSemaphoreName') -and
                $productionText.Contains('$env:CARGO_MAKEFLAGS =') -and
                $productionText.Contains('Remove-Item -LiteralPath Env:CARGO_MAKEFLAGS') -and
                $wrapperText.Contains('CreateSeededSemaphore(') -and
                $wrapperText.Contains('TransferSeededSemaphore(') -and
                $wrapperText.Contains('SeededSemaphorePlaceholder') -and
                $wrapperText.Contains('DiagnosticSemaphorePrefix = "Local\\ProjectAtlasParserPack-";') -and
                -not $wrapperText.Contains('DiagnosticSemaphorePrefix = "Global\\ProjectAtlasParserPack-";') -and
                -not $wrapperText.Contains('DiagnosticSemaphorePrefix = "ProjectAtlasParserPack-";') -and
                $wrapperText.Contains('Add-ConstructionObjectDirectoryPrincipalAccess') -and
                $wrapperText.Contains('Assert-ConstructionObjectDirectoryPrincipalAbsent')) `
            "Windows construction did not use its transferred protected one-worker jobserver and exact namespace grant."
        Require `
            ($wrapperText.Contains('ValidateConstructionToken(') -and
                $wrapperText.Contains('RequiredIntegritySid = "S-1-16-8192";') -and
                $wrapperText.Contains('SeGroupLogonId = 0xC0000000;') -and
                $wrapperText.Contains('private sealed class TokenInformationBuffer : IDisposable') -and
                $wrapperText.Contains('private sealed class TokenNamespaceSnapshot') -and
                $wrapperText.Contains('internal bool PrivateNamespaceEnabled { get; set; }') -and
                $wrapperText.Contains('TokenBnoIsolationInformation') -and
                $wrapperText.Contains('[MarshalAs(UnmanagedType.U1)]') -and
                $wrapperText.Contains('MaximumTokenInformationBytes = 64 * 1024;') -and
                $wrapperText.Contains('MaximumBnoIsolationPrefixCharacters = 256;') -and
                $wrapperText.Contains('ReadBoundedUnicodeString(') -and
                $wrapperText -match
                    'snapshot\.HasRestrictions\s*=\s*ReadExactTokenBoolean\(' -and
                $wrapperText.Contains('if (information.Length != sizeof(byte))') -and
                $wrapperText.Contains('byte value = Marshal.ReadByte(information.Pointer);') -and
                $wrapperText.Contains('if (value > 1)') -and
                $wrapperText -match
                    'snapshot\.IsAppContainer\s*=\s*ReadExactTokenDword\(' -and
                $wrapperText -match
                    'snapshot\.IsSandboxed\s*=\s*ReadExactTokenDword\(' -and
                $wrapperText -match
                    'snapshot\.IsAppSilo\s*=\s*ReadExactTokenDword\(' -and
                $wrapperText.Contains('DecodeExactTokenDwordBoolean(') -and
                $wrapperText.Contains('informationLength != sizeof(uint)') -and
                $wrapperText.Contains('availableCharacters') -and
                $wrapperText.Contains('requiredGroupBytes > groupsInformation.Length') -and
                $wrapperText.Contains('!IsValidSid(sid)') -and
                $wrapperText.Contains('checked(offset + sidBytes) > information.Length')) `
            "Windows wrapper did not retain exact construction-token validation."
        $nativeSourceAssignments = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$nativeSource'
            },
            $true
        ))
        Require ($nativeSourceAssignments.Count -eq 1) "Expected one native adapter source assignment."
        Invoke-Expression $nativeSourceAssignments[0].Extent.Text
        Require `
            ($nativeSource.Contains('EntryPoint = "LogonUserW"') -and
                $nativeSource.Contains('EntryPoint = "CreateProcessWithTokenW"') -and
                $nativeSource.Contains('private static extern bool OpenProcessToken(') -and
                -not $nativeSource.Contains('SeDebugPrivilege') -and
                -not $nativeSource.Contains('AdjustTokenPrivileges') -and
                -not $nativeSource.Contains('EntryPoint = "CreateProcessWithLogonW"') -and
                -not $nativeSource.Contains('EntryPoint = "CreateProcessAsUserW"') -and
                -not $nativeSource.Contains('CreateBreakawayFromJob = 0x01000000;') -and
                $nativeSource.Contains('JobObjectLimitBreakawayOk = 0x00000800;') -and
                $nativeSource.Contains('JobObjectLimitSilentBreakawayOk = 0x00001000;') -and
                $nativeSource.Contains('construction-broker-job-required') -and
                $nativeSource.Contains('construction-broker-job-membership') -and
                $nativeSource.Contains('construction-broker-job-policy') -and
                $nativeSource.Contains('ValidateBrokerJobMembership(') -and
                $nativeSource.Contains('construction-process-retained-inherited-job') -and
                $wrapperText.Contains(
                    '[ProjectAtlasConstructionProcess]::ConfigureBrokerJob($BrokerJobName)'
                ) -and
                $nativeSource -match
                    'LogonUser\(\s*username,\s*"\.",\s*passwordPointer,\s*Logon32LogonInteractive,\s*Logon32ProviderDefault,\s*out logonToken\)' -and
                $nativeSource -match
                    'CreateProcessWithToken\(\s*logonToken,\s*0,\s*executable,' -and
                $nativeSource.Contains(
                    'Marshal.ZeroFreeGlobalAllocUnicode(passwordPointer);'
                ) -and
                $nativeSource.Contains('logon-construction-principal') -and
                $nativeSource.Contains('create-process-with-construction-token') -and
                $nativeSource.Contains('ambient-construction-jobserver') -and
                $nativeSource.Contains('SemaphoreSynchronizeAndModify = 0x00100002;') -and
                -not $nativeSource.Contains('SemaphoreAllAccess') -and
                -not $nativeSource.Contains('0x001F0003') -and
                $nativeSource.Contains('EntryPoint = "CreateSemaphoreExW"') -and
                $nativeSource.Contains('string sddl = "D:P(A;;0x00100002;;;"') -and
                $nativeSource.Contains('")S:(ML;;NW;;;" + RequiredIntegritySid + ")";') -and
                $nativeSource.Contains('EntryPoint = "QueryInformationJobObject"') -and
                $nativeSource.Contains('DuplicateHandle(') -and
                $nativeSource.Contains('false,') -and
                $nativeSource.Contains('DuplicateSameAccess') -and
                $nativeSource.Contains('CaptureTokenNamespaceSnapshot(') -and
                $nativeSource.Contains('RequireEquivalentTokenNamespaces(') -and
                -not $nativeSource.Contains('JobObjectCreateSilo') -and
                -not $nativeSource.Contains('CreateRestrictedToken') -and
                -not $nativeSource.Contains('SetTokenInformation') -and
                -not $nativeSource.Contains('PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES') -and
                -not $nativeSource.Contains('JOB_OBJECT_SECURITY_')) `
            "Windows construction adapter did not use the bounded alternate-logon process boundary."
        $principalLogonStart = $nativeSource.IndexOf(
            'if (!LogonUser(',
            [System.StringComparison]::Ordinal
        )
        $principalTokenValidationStart = $nativeSource.IndexOf(
            'admittedLogonSid = ValidateConstructionToken(',
            [System.StringComparison]::Ordinal
        )
        $processCreationStart = $nativeSource.IndexOf(
            'created = CreateProcessWithToken(',
            [System.StringComparison]::Ordinal
        )
        $principalTokenCloseStart = $nativeSource.IndexOf(
            'logonToken.Dispose();',
            [System.StringComparison]::Ordinal
        )
        $processTokenOpenStart = $nativeSource.IndexOf(
            'if (!OpenProcessToken(process.Process, TokenQuery, out constructionToken))',
            [System.StringComparison]::Ordinal
        )
        $tokenValidationStart = $nativeSource.IndexOf(
            'string processLogonSid = ValidateConstructionToken(',
            [System.StringComparison]::Ordinal
        )
        Require `
            ($principalLogonStart -ge 0 -and
                $principalTokenValidationStart -gt $principalLogonStart -and
                $processCreationStart -gt $principalTokenValidationStart -and
                $principalTokenCloseStart -gt $processCreationStart -and
                $processTokenOpenStart -gt $processCreationStart -and
                $tokenValidationStart -gt $processTokenOpenStart) `
            "Construction token ownership boundaries were missing."
        if (-not ('ProjectAtlasConstructionProcess' -as [type])) {
            Add-Type -TypeDefinition $nativeSource -Language CSharp
        }

        $adapterType = [ProjectAtlasConstructionProcess]
        $nestedTypeFlags = [System.Reflection.BindingFlags]'NonPublic,Static'
        $instanceMemberFlags = [System.Reflection.BindingFlags]'NonPublic,Instance'
        $publicInstanceMemberFlags = [System.Reflection.BindingFlags]'Public,Instance'
        $admissionScenarioType = $adapterType.GetNestedType(
            'AdmissionScenario',
            [System.Reflection.BindingFlags]::NonPublic
        )
        $admissionReceiptType = $adapterType.GetNestedType(
            'AdmissionReceipt',
            [System.Reflection.BindingFlags]::NonPublic
        )
        $processInformationType = $adapterType.GetNestedType(
            'ProcessInformation',
            [System.Reflection.BindingFlags]::NonPublic
        )
        Require ($null -ne $admissionScenarioType) "Construction admission scenario was missing."
        Require ($null -ne $admissionReceiptType) "Construction admission receipt was missing."
        Require ($null -ne $processInformationType) "Construction process information was missing."
        Require `
            (([enum]::GetNames($admissionScenarioType) -join ',') -eq
                'Normal,RetainedJobBeforeAdmission,FailBeforeJobAssignment,FailBeforeJobAssignmentAndCleanupFailure') `
            "Construction admission failure domain was not closed."

        $publicRunMethods = @($adapterType.GetMethods(
            [System.Reflection.BindingFlags]'Public,Static'
        ) | Where-Object Name -eq 'Run')
        Require `
            ($publicRunMethods.Count -eq 1 -and
                $publicRunMethods[0].GetParameters().Count -eq 8) `
            "Construction adapter exposed an admission fault through its public launch API."
        $privateRunCore = $adapterType.GetMethod('RunCore', $nestedTypeFlags)
        Require `
            ($null -ne $privateRunCore -and
                $privateRunCore.GetParameters().Count -eq 10) `
            "Construction adapter private recovery boundary changed."
        $namespaceSnapshotType = $adapterType.GetNestedType(
            'TokenNamespaceSnapshot',
            [System.Reflection.BindingFlags]::NonPublic
        )
        $namespaceComparator = $adapterType.GetMethod(
            'TokenNamespaceSnapshotsEqual',
            $nestedTypeFlags
        )
        Require `
            ($null -ne $namespaceSnapshotType -and $null -ne $namespaceComparator) `
            "Construction token namespace comparator was missing."
        $namespaceCapture = $adapterType.GetMethod(
            'CaptureTokenNamespaceSnapshot',
            $nestedTypeFlags
        )
        Require ($null -ne $namespaceCapture) "Construction token namespace capture was missing."
        $privateNamespaceDecoder = $adapterType.GetMethod(
            'DecodeExactTokenDwordBoolean',
            $nestedTypeFlags
        )
        Require ($null -ne $privateNamespaceDecoder) `
            "Construction private namespace decoder was missing."
        $privateNamespaceFixture = [System.Runtime.InteropServices.Marshal]::AllocHGlobal(4)
        try {
            [System.Runtime.InteropServices.Marshal]::WriteInt32(
                $privateNamespaceFixture,
                0
            )
            Require `
                ($privateNamespaceDecoder.Invoke(
                        $null,
                        [object[]]@($privateNamespaceFixture, 4, 'private-fixture')
                    ) -eq $false) `
                "Canonical disabled private namespace fixture did not decode."
            [System.Runtime.InteropServices.Marshal]::WriteInt32(
                $privateNamespaceFixture,
                1
            )
            Require `
                ($privateNamespaceDecoder.Invoke(
                        $null,
                        [object[]]@($privateNamespaceFixture, 4, 'private-fixture')
                    ) -eq $true) `
                "Canonical enabled private namespace fixture did not decode."
            foreach ($invalidFixture in @(
                [pscustomobject]@{ Length = 1; Value = 1 },
                [pscustomobject]@{ Length = 4; Value = 2 }
            )) {
                [System.Runtime.InteropServices.Marshal]::WriteInt32(
                    $privateNamespaceFixture,
                    [int]$invalidFixture.Value
                )
                $invalidFailure = $null
                try {
                    [void]$privateNamespaceDecoder.Invoke(
                        $null,
                        [object[]]@(
                            $privateNamespaceFixture,
                            [int]$invalidFixture.Length,
                            'private-fixture'
                        )
                    )
                }
                catch {
                    $invalidFailure = $_.Exception
                    while (($invalidFailure -is
                                [System.Reflection.TargetInvocationException] -or
                            $invalidFailure -is
                                [System.Management.Automation.MethodInvocationException]) -and
                        $null -ne $invalidFailure.InnerException) {
                        $invalidFailure = $invalidFailure.InnerException
                    }
                }
                Require `
                    ($invalidFailure -is [System.InvalidOperationException] -and
                        $invalidFailure.Message -ceq 'private-fixture') `
                    "Invalid private namespace fixture was accepted."
            }
        }
        finally {
            [System.Runtime.InteropServices.Marshal]::FreeHGlobal(
                $privateNamespaceFixture
            )
        }
        $currentIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
        try {
            $currentPrincipalSid = $currentIdentity.User.Value
            $currentSnapshot = $namespaceCapture.Invoke(
                $null,
                [object[]]@($currentIdentity.AccessToken)
            )
            Require ($null -ne $currentSnapshot) `
                "Current Windows token namespace capture returned no snapshot."
            $currentHasRestrictions = $namespaceSnapshotType.GetProperty(
                'HasRestrictions',
                $instanceMemberFlags
            ).GetValue($currentSnapshot)
            Require ($currentHasRestrictions -is [bool]) `
                "Current Windows TokenHasRestrictions payload was not decoded as a Boolean."
            $currentPrivateNamespace = $namespaceSnapshotType.GetProperty(
                'PrivateNamespaceEnabled',
                $instanceMemberFlags
            ).GetValue($currentSnapshot)
            $currentPrivateNamespaceLength = $namespaceSnapshotType.GetProperty(
                'PrivateNamespaceInformationLength',
                $instanceMemberFlags
            ).GetValue($currentSnapshot)
            Require `
                ($currentPrivateNamespace -is [bool] -and
                    [int]$currentPrivateNamespaceLength -eq 4) `
                "Current Windows TokenPrivateNameSpace payload was not canonical."
        }
        finally {
            $currentIdentity.Dispose()
        }
        $seedCreator = $adapterType.GetMethod('CreateSeededSemaphore', $nestedTypeFlags)
        $localFree = $adapterType.GetMethod('LocalFree', $nestedTypeFlags)
        Require ($null -ne $seedCreator -and $null -ne $localFree) `
            "Construction seed ownership boundary was missing."
        $seedName = "Local\ProjectAtlasParserPack-$([Guid]::NewGuid().ToString('N'))"
        $seedArguments = [object[]]@($seedName, $currentPrincipalSid, [IntPtr]::Zero)
        $seedRawHandle = [IntPtr]::Zero
        $seedSecurityDescriptor = [IntPtr]::Zero
        $seedOwner = $null
        $seedOpen = $null
        $forbiddenSeedOpen = $null
        try {
            $seedRawHandle = [IntPtr]$seedCreator.Invoke($null, $seedArguments)
            $seedSecurityDescriptor = [IntPtr]$seedArguments[2]
            Require `
                ($seedRawHandle -ne [IntPtr]::Zero -and
                    $seedSecurityDescriptor -ne [IntPtr]::Zero) `
                "Construction seed creation returned invalid ownership."
            $seedOwner = [Microsoft.Win32.SafeHandles.SafeWaitHandle]::new(
                $seedRawHandle,
                $true
            )
            $seedRawHandle = [IntPtr]::Zero
            $seedRights = [System.Security.AccessControl.SemaphoreRights]::Synchronize -bor
                [System.Security.AccessControl.SemaphoreRights]::Modify
            Require `
                ([System.Threading.SemaphoreAcl]::TryOpenExisting(
                    $seedName,
                    $seedRights,
                    [ref]$seedOpen
                )) `
                "Mandatory-labeled construction seed could not be reopened."
            $fullControlDenied = $false
            try {
                [void][System.Threading.SemaphoreAcl]::TryOpenExisting(
                    $seedName,
                    [System.Security.AccessControl.SemaphoreRights]::FullControl,
                    [ref]$forbiddenSeedOpen
                )
            }
            catch [System.UnauthorizedAccessException] {
                $fullControlDenied = $true
            }
            Require $fullControlDenied `
                "Construction seed DACL granted more than synchronize and modify."
        }
        finally {
            if ($null -ne $forbiddenSeedOpen) { $forbiddenSeedOpen.Dispose() }
            if ($null -ne $seedOpen) { $seedOpen.Dispose() }
            if ($null -ne $seedOwner) { $seedOwner.Dispose() }
            if ($seedRawHandle -ne [IntPtr]::Zero) {
                [Microsoft.Win32.SafeHandles.SafeWaitHandle]::new(
                    $seedRawHandle,
                    $true
                ).Dispose()
            }
            if ($seedSecurityDescriptor -ne [IntPtr]::Zero) {
                Require `
                    ([IntPtr]$localFree.Invoke(
                        $null,
                        [object[]]@($seedSecurityDescriptor)
                    ) -eq [IntPtr]::Zero) `
                    "Construction seed security descriptor was not freed."
            }
        }
        $survivingSeed = $null
        $seedAbsent = -not [System.Threading.SemaphoreAcl]::TryOpenExisting(
            $seedName,
            [System.Security.AccessControl.SemaphoreRights]::Synchronize,
            [ref]$survivingSeed
        )
        if ($null -ne $survivingSeed) { $survivingSeed.Dispose() }
        Require $seedAbsent "Construction seed survived owning-handle cleanup."
        $leftSnapshot = [Activator]::CreateInstance($namespaceSnapshotType, $true)
        $rightSnapshot = [Activator]::CreateInstance($namespaceSnapshotType, $true)
        foreach ($snapshot in @($leftSnapshot, $rightSnapshot)) {
            $namespaceSnapshotType.GetProperty(
                'BnoIsolationPrefix',
                $instanceMemberFlags
            ).SetValue($snapshot, '')
        }
        Require `
            ([bool]$namespaceComparator.Invoke(
                $null,
                [object[]]@($leftSnapshot, $rightSnapshot)
            )) `
            "Equivalent construction token namespaces did not compare equal."
        foreach ($mutation in @(
            @{ Name = 'BnoIsolationEnabled'; Value = $true; Reset = $false },
            @{ Name = 'BnoIsolationPrefix'; Value = 'isolated'; Reset = '' },
            @{ Name = 'IsAppContainer'; Value = $true; Reset = $false },
            @{ Name = 'IsSandboxed'; Value = $true; Reset = $false },
            @{ Name = 'IsAppSilo'; Value = $true; Reset = $false },
            @{ Name = 'HasRestrictions'; Value = $true; Reset = $false },
            @{ Name = 'RestrictedSidCount'; Value = [uint32]1; Reset = [uint32]0 }
        )) {
            $property = $namespaceSnapshotType.GetProperty(
                $mutation.Name,
                $instanceMemberFlags
            )
            Require ($null -ne $property) "Construction namespace snapshot field was missing."
            $property.SetValue($rightSnapshot, $mutation.Value)
            Require `
                (-not [bool]$namespaceComparator.Invoke(
                    $null,
                    [object[]]@($leftSnapshot, $rightSnapshot)
                )) `
                "Construction namespace comparator ignored $($mutation.Name)."
            $property.SetValue($rightSnapshot, $mutation.Reset)
        }
        $jobPolicyValidator = $adapterType.GetMethod(
            'ValidateJobPolicyValues',
            $nestedTypeFlags
        )
        Require ($null -ne $jobPolicyValidator) "Construction Job policy validator was missing."
        foreach ($expectedFlags in @([uint32]0x2000, [uint32]0x2800)) {
            $jobPolicyValidator.Invoke(
                $null,
                [object[]]@($expectedFlags, [uint32]0, $expectedFlags, 'policy')
            ) | Out-Null
            foreach ($invalid in @(
                [object[]]@([uint32]($expectedFlags -bor 1), [uint32]0),
                [object[]]@($expectedFlags, [uint32]1)
            )) {
                $policyRejected = $false
                try {
                    $jobPolicyValidator.Invoke(
                        $null,
                        [object[]]@($invalid[0], $invalid[1], $expectedFlags, 'policy')
                    ) | Out-Null
                }
                catch {
                    $policyRejected = $true
                }
                Require $policyRejected "Construction Job policy accepted one extra bit."
            }
        }
        $processCreationIndex = $nativeSource.IndexOf(
            'created = CreateProcessWithToken(',
            [System.StringComparison]::Ordinal
        )
        $creationFlagsIndex = $nativeSource.IndexOf(
            'uint flags = GetConstructionCreationFlags();',
            [System.StringComparison]::Ordinal
        )
        $processCreatedIndex = $nativeSource.IndexOf(
            'processCreated = true;',
            [System.StringComparison]::Ordinal
        )
        $processTokenOpenIndex = $nativeSource.IndexOf(
            'if (!OpenProcessToken(process.Process, TokenQuery, out constructionToken))',
            [System.StringComparison]::Ordinal
        )
        $tokenValidationIndex = $nativeSource.IndexOf(
            'string processLogonSid = ValidateConstructionToken(',
            [System.StringComparison]::Ordinal
        )
        $logonNamespaceIndex = $nativeSource.IndexOf(
            'CaptureTokenNamespaceSnapshot(logonToken)',
            [System.StringComparison]::Ordinal
        )
        $seedCreateIndex = $nativeSource.IndexOf(
            'seededSemaphore = CreateSeededSemaphore(',
            [System.StringComparison]::Ordinal
        )
        $seedTransferIndex = $nativeSource.IndexOf(
            'TransferSeededSemaphore(',
            $processCreatedIndex,
            [System.StringComparison]::Ordinal
        )
        $duplicateSeedIndex = $nativeSource.IndexOf(
            'if (!DuplicateHandle(',
            [System.StringComparison]::Ordinal
        )
        $closeParentSeedIndex = $nativeSource.IndexOf(
            'if (!CloseHandle(semaphore))',
            $duplicateSeedIndex,
            [System.StringComparison]::Ordinal
        )
        $childBeforeNamespaceIndex = $nativeSource.IndexOf(
            'CaptureTokenNamespaceSnapshot(constructionToken)',
            $tokenValidationIndex,
            [System.StringComparison]::Ordinal
        )
        $retainedJobInjectionIndex = $nativeSource.IndexOf(
            'if (admissionScenario == AdmissionScenario.RetainedJobBeforeAdmission)',
            [System.StringComparison]::Ordinal
        )
        $inheritedJobCheckIndex = $nativeSource.IndexOf(
            'IsProcessInJob(process.Process, IntPtr.Zero, out inheritedJob)',
            [System.StringComparison]::Ordinal
        )
        $admissionFailureIndex = $nativeSource.IndexOf(
            'if (admissionScenario == AdmissionScenario.FailBeforeJobAssignment ||',
            [System.StringComparison]::Ordinal
        )
        $jobAssignmentIndex = $nativeSource.IndexOf(
            'if (!AssignProcessToJobObject(job, process.Process))',
            $admissionFailureIndex,
            [System.StringComparison]::Ordinal
        )
        $childAfterNamespaceIndex = $nativeSource.IndexOf(
            'CaptureTokenNamespaceSnapshot(constructionToken)',
            $jobAssignmentIndex,
            [System.StringComparison]::Ordinal
        )
        $resumeIndex = $nativeSource.IndexOf(
            'if (ResumeThread(process.Thread) == UInt32.MaxValue)',
            [System.StringComparison]::Ordinal
        )
        $admissionCleanupIndex = $nativeSource.IndexOf(
            'if (processCreated && !assignedToJob)',
            [System.StringComparison]::Ordinal
        )
        $tokenCloseIndex = $nativeSource.IndexOf(
            'constructionToken.Dispose();',
            [System.StringComparison]::Ordinal
        )
        Require `
            ($creationFlagsIndex -ge 0 -and
                $processCreationIndex -gt $creationFlagsIndex -and
                $processCreatedIndex -gt $processCreationIndex -and
                $logonNamespaceIndex -gt $creationFlagsIndex -and
                $logonNamespaceIndex -lt $processCreationIndex -and
                $seedCreateIndex -gt $logonNamespaceIndex -and
                $seedCreateIndex -lt $processCreationIndex -and
                $seedTransferIndex -gt $processCreatedIndex -and
                $duplicateSeedIndex -ge 0 -and
                $closeParentSeedIndex -gt $duplicateSeedIndex -and
                $processTokenOpenIndex -gt $processCreatedIndex -and
                $tokenValidationIndex -gt $processTokenOpenIndex -and
                $childBeforeNamespaceIndex -gt $tokenValidationIndex -and
                $retainedJobInjectionIndex -gt $tokenValidationIndex -and
                $inheritedJobCheckIndex -gt $retainedJobInjectionIndex -and
                $admissionFailureIndex -gt $inheritedJobCheckIndex -and
                $jobAssignmentIndex -gt $admissionFailureIndex -and
                $childAfterNamespaceIndex -gt $jobAssignmentIndex -and
                $resumeIndex -gt $childAfterNamespaceIndex -and
                $admissionCleanupIndex -gt $jobAssignmentIndex -and
                $tokenCloseIndex -gt $admissionCleanupIndex -and
                $nativeSource.Contains('return CreateSuspended | CreateNoWindow | CreateUnicodeEnvironment;') -and
                $nativeSource.Contains('ValidateCurrentBrokerJob(brokerJobName);') -and
                $nativeSource.Contains('process.Process,') -and
                $nativeSource.Contains('ValidateJobPolicyValues(') -and
                $nativeSource.Contains('JobObjectLimitKillOnJobClose | JobObjectLimitBreakawayOk') -and
                $nativeSource.Contains('MaximumLogonCommandLineCharacters = 1023;') -and
                $nativeSource.Contains('construction-command-line-too-long') -and
                $nativeSource.Contains('LogonTokenHandleOwned') -and
                $nativeSource.Contains('LogonTokenHandleClosed') -and
                $nativeSource.Contains('ConstructionTokenHandleOwned') -and
                $nativeSource.Contains('ConstructionTokenHandleClosed') -and
                $nativeSource.Contains('SeededSemaphoreCreatedNew') -and
                $nativeSource.Contains('SeededSemaphoreDuplicated') -and
                $nativeSource.Contains('SeededSemaphoreParentHandleClosed') -and
                $nativeSource.Contains('ValidateOwnedConstructionJobPolicy(job);') -and
                $nativeSource.Contains('JobObjectBasicUiRestrictions') -and
                $nativeSource.Contains('uiRestrictions != 0') -and
                $nativeSource.Contains('inheritHandle,') -and
                $nativeSource.Contains('false,') -and
                $nativeSource.Contains('AdmissionCleanupWaitMilliseconds = 5000;') -and
                $nativeSource.Contains(
                    'int waitError = wait == WaitFailed ? Marshal.GetLastWin32Error() : 0;'
                ) -and
                $nativeSource.Contains(
                    'admissionScenario == AdmissionScenario.FailBeforeJobAssignmentAndCleanupFailure &&'
                ) -and
                $nativeSource.Contains('cleanupFailure == null')) `
            "Construction admission fault did not remain private, suspended, pre-Job, and bounded."

        $composeRunFailures = $adapterType.GetMethod(
            'ComposeRunFailures',
            $nestedTypeFlags
        )
        Require ($null -ne $composeRunFailures) "Construction failure composition was missing."
        $composedRunFailure = $composeRunFailures.Invoke(
            $null,
            [object[]]@(
                [System.InvalidOperationException]::new('operation-failure'),
                [System.InvalidOperationException]::new('cleanup-failure')
            )
        )
        Require `
            ($composedRunFailure -is [System.AggregateException] -and
                $composedRunFailure.InnerExceptions.Count -eq 2 -and
                $composedRunFailure.InnerExceptions[0].Message -eq 'operation-failure' -and
                $composedRunFailure.InnerExceptions[1].Message -eq 'cleanup-failure') `
            "Construction adapter did not preserve operation and cleanup failures together."

        $buildCommandLine = $adapterType.GetMethod('BuildCommandLine', $nestedTypeFlags)
        Require ($null -ne $buildCommandLine) "Construction command-line boundary was missing."
        $maximumCommandLine = $buildCommandLine.Invoke(
            $null,
            [object[]]@('x', [string[]]@('a' * 1021))
        )
        Require `
            ($maximumCommandLine.Length -eq 1023) `
            "Construction command-line boundary rejected its documented maximum."
        $oversizedCommandFailure = $null
        try {
            $buildCommandLine.Invoke(
                $null,
                [object[]]@('x', [string[]]@('a' * 1022))
            ) | Out-Null
        }
        catch {
            $oversizedCommandFailure = $_.Exception
            while (($oversizedCommandFailure -is
                    [System.Management.Automation.MethodInvocationException] -or
                    $oversizedCommandFailure -is
                    [System.Reflection.TargetInvocationException]) -and
                $null -ne $oversizedCommandFailure.InnerException) {
                $oversizedCommandFailure = $oversizedCommandFailure.InnerException
            }
        }
        Require `
            ($oversizedCommandFailure -is [System.InvalidOperationException] -and
                $oversizedCommandFailure.Message -eq 'construction-command-line-too-long') `
            "Construction command-line boundary accepted an oversized alternate-logon request."

        $admissionFixtureSource = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class ProjectAtlasConstructionAdmissionFixture
{
    private const uint Synchronize = 0x00100000;
    private const uint DuplicateSameAccess = 0x00000002;

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DuplicateHandle(
        IntPtr sourceProcess,
        IntPtr sourceHandle,
        IntPtr targetProcess,
        out IntPtr targetHandle,
        uint desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool inheritHandle,
        uint options);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr OpenThread(
        uint desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool inheritHandle,
        uint threadId);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObject(IntPtr attributes, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    public static IntPtr DuplicateProcessHandle(IntPtr process)
    {
        IntPtr duplicate;
        IntPtr current = GetCurrentProcess();
        if (!DuplicateHandle(
            current,
            process,
            current,
            out duplicate,
            0,
            false,
            DuplicateSameAccess))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "duplicate-process-handle");
        }
        return duplicate;
    }

    public static IntPtr OpenThreadForWait(uint threadId)
    {
        IntPtr thread = OpenThread(Synchronize, false, threadId);
        if (thread == IntPtr.Zero)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "open-thread-for-wait");
        }
        return thread;
    }

    public static IntPtr CreateUnassignedJob()
    {
        IntPtr job = CreateJobObject(IntPtr.Zero, null);
        if (job == IntPtr.Zero)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "create-unassigned-job");
        }
        return job;
    }

    public static void Close(IntPtr handle)
    {
        if (handle != IntPtr.Zero)
        {
            CloseHandle(handle);
        }
    }
}
'@
        if (-not ('ProjectAtlasConstructionAdmissionFixture' -as [type])) {
            Add-Type -TypeDefinition $admissionFixtureSource -Language CSharp
        }
        $recoverUnassignedProcess = $adapterType.GetMethod(
            'RecoverUnassignedProcess',
            $nestedTypeFlags
        )
        Require ($null -ne $recoverUnassignedProcess) "Construction admission recovery was missing."
        $selfTestProcess = $null
        $duplicateProcessHandle = [IntPtr]::Zero
        $threadHandle = [IntPtr]::Zero
        $jobHandle = [IntPtr]::Zero
        try {
            $selfTestStart = [System.Diagnostics.ProcessStartInfo]::new()
            $selfTestStart.FileName =
                [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
            $selfTestStart.UseShellExecute = $false
            $selfTestStart.CreateNoWindow = $true
            $selfTestStart.ArgumentList.Add('-NoProfile')
            $selfTestStart.ArgumentList.Add('-Command')
            $selfTestStart.ArgumentList.Add('Start-Sleep -Seconds 30')
            $selfTestProcess = [System.Diagnostics.Process]::Start($selfTestStart)
            Require ($null -ne $selfTestProcess) "Could not start admission recovery canary."
            $selfTestProcess.Refresh()
            Require `
                ($selfTestProcess.Threads.Count -gt 0) `
                "Admission recovery canary exposed no primary thread."
            $duplicateProcessHandle =
                [ProjectAtlasConstructionAdmissionFixture]::DuplicateProcessHandle(
                    $selfTestProcess.Handle
                )
            $threadHandle = [ProjectAtlasConstructionAdmissionFixture]::OpenThreadForWait(
                [uint32]$selfTestProcess.Threads[0].Id
            )
            $jobHandle = [ProjectAtlasConstructionAdmissionFixture]::CreateUnassignedJob()

            $processInformation = [Activator]::CreateInstance($processInformationType, $true)
            $processInformationType.GetField(
                'Process',
                $publicInstanceMemberFlags
            ).SetValue($processInformation, $duplicateProcessHandle)
            $processInformationType.GetField(
                'Thread',
                $publicInstanceMemberFlags
            ).SetValue($processInformation, $threadHandle)
            $processInformationType.GetField(
                'ProcessId',
                $publicInstanceMemberFlags
            ).SetValue($processInformation, [uint32]$selfTestProcess.Id)
            $admissionReceipt = [Activator]::CreateInstance($admissionReceiptType, $true)
            $admissionCleanupFailure = $recoverUnassignedProcess.Invoke(
                $null,
                [object[]]@($jobHandle, $processInformation, $admissionReceipt)
            )
            $duplicateProcessHandle = [IntPtr]::Zero
            $threadHandle = [IntPtr]::Zero
            $jobHandle = [IntPtr]::Zero
            Require `
                ($null -eq $admissionCleanupFailure) `
                "Construction admission recovery reported a cleanup failure."
            $receiptValue = {
                param([string]$Name)
                return $admissionReceiptType.GetProperty(
                    $Name,
                    $instanceMemberFlags
                ).GetValue($admissionReceipt)
            }
            Require `
                ((& $receiptValue 'ProcessId') -eq $selfTestProcess.Id -and
                    (& $receiptValue 'TerminationAttempted') -and
                    (& $receiptValue 'WaitResult') -eq 0 -and
                    (& $receiptValue 'Reaped') -and
                    (& $receiptValue 'JobHandleOwned') -and
                    (& $receiptValue 'JobHandleClosed') -and
                    (& $receiptValue 'ProcessHandleOwned') -and
                    (& $receiptValue 'ProcessHandleClosed') -and
                    (& $receiptValue 'ThreadHandleOwned') -and
                    (& $receiptValue 'ThreadHandleClosed')) `
                "Construction admission recovery receipt was incomplete."
            Require `
                $selfTestProcess.WaitForExit(5000) `
                "Construction admission recovery did not reap the exact canary PID."
            $selfTestPid = $selfTestProcess.Id
            $selfTestProcess.Dispose()
            $selfTestProcess = $null
            $survivor = Get-Process -Id $selfTestPid -ErrorAction SilentlyContinue
            Require ($null -eq $survivor) "Construction admission recovery left its canary alive."
        }
        finally {
            [ProjectAtlasConstructionAdmissionFixture]::Close($jobHandle)
            [ProjectAtlasConstructionAdmissionFixture]::Close($threadHandle)
            [ProjectAtlasConstructionAdmissionFixture]::Close($duplicateProcessHandle)
            if ($null -ne $selfTestProcess) {
                if (-not $selfTestProcess.HasExited) {
                    $selfTestProcess.Kill($true)
                    $selfTestProcess.WaitForExit(5000) | Out-Null
                }
                $selfTestProcess.Dispose()
            }
        }

        foreach ($removedDiagnosticText in @(
            'ProjectAtlasJobserverSynchronizeAccessCheckResult',
            'EvaluateJobserverSynchronizeAccess',
            'LastJobserverSynchronizeAccessCheck',
            'CreateAdmittedJobserver',
            'CanCreateFreshJobserverName',
            'AccessCheck(',
            'ProjectAtlasCurrentProcessTokenRestrictionProbe',
            'ProjectAtlasObjectDirectoryProbe'
        )) {
            Require `
                (-not $wrapperAst.Extent.Text.Contains($removedDiagnosticText)) `
                "Windows construction wrapper retained obsolete jobserver diagnostic machinery."
        }
        $probeSourceAssignments = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$probeSource'
            },
            $true
        ))
        Require ($probeSourceAssignments.Count -eq 1) "Expected one boundary probe source assignment."
        Invoke-Expression $probeSourceAssignments[0].Extent.Text
        Require `
            ($probeSource.Contains('$ExpectedSessionId') -and
                $probeSource.Contains('S-1-16-8192') -and
                $probeSource.Contains('$principal.IsInRole($expectedSecurityIdentifier)') -and
                $probeSource.Contains('Test-Path -LiteralPath Env:CARGO_MAKEFLAGS') -and
                $probeSource.Contains('exit 24') -and
                $probeSource.Contains('[Environment]::GetEnvironmentVariables().Keys')) `
            "Construction boundary probe did not retain identity, session, integrity, and sanitized-environment checks."
        Require `
            ($wrapperAst.Extent.Text.Contains('24 { "unexpected-jobserver-environment" }') -and
                $wrapperAst.Extent.Text.Contains('37 { "target-sid-membership-query" }') -and
                $wrapperAst.Extent.Text.Contains('38 { "target-sid-not-effective" }')) `
            "Construction boundary probe did not retain closed identity and environment diagnostics."
        $wrapperText = $wrapperAst.Extent.Text
        $sessionCheckIndex = $probeSource.IndexOf(
            '[System.Diagnostics.Process]::GetCurrentProcess().SessionId -ne $ExpectedSessionId',
            [System.StringComparison]::Ordinal
        )
        $cargoJobBudgetCheckIndex = $probeSource.IndexOf(
            '[string]$env:CARGO_BUILD_JOBS -cne ''1''',
            [System.StringComparison]::Ordinal
        )
        $cargoMakeflagsCheckIndex = $probeSource.IndexOf(
            'Test-Path -LiteralPath Env:CARGO_MAKEFLAGS',
            [System.StringComparison]::Ordinal
        )
        Require `
            ($sessionCheckIndex -ge 0 -and
                $cargoJobBudgetCheckIndex -gt $sessionCheckIndex -and
                $cargoMakeflagsCheckIndex -gt $sessionCheckIndex -and
                -not $probeSource.Contains('[System.Threading.SemaphoreAcl]::OpenExisting(') -and
                -not $probeSource.Contains('$JobserverName') -and
                -not $probeSource.Contains('$rustcStart') -and
                -not $probeSource.Contains('$ObjectDirectoryProbePath') -and
                -not $probeSource.Contains('$TokenRestrictionProbePath')) `
            "Parent boundary probe lost its Cargo budget or retained obsolete jobserver diagnostics."

        $principalProcessDefinitions = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                    $node.Name -eq 'Get-PrincipalProcesses'
            },
            $true
        ))
        Require `
            ($principalProcessDefinitions.Count -eq 1) `
            "Expected one Get-PrincipalProcesses definition."
        $principalProcessDefinition = $principalProcessDefinitions[0]
        $principalProcessParameters = @(
            $principalProcessDefinition.Body.ParamBlock.Parameters |
                ForEach-Object { $_.Name.VariablePath.UserPath }
        )
        Require `
            ($principalProcessParameters.Count -eq 2 -and
                $principalProcessParameters[0] -eq 'Sid' -and
                $principalProcessParameters[1] -eq 'Deadline') `
            "Principal-process scans did not receive the caller's cleanup deadline."
        $principalAccountQueries = @($principalProcessDefinition.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.CommandAst] -and
                    $node.GetCommandName() -eq 'Get-CimInstance'
            },
            $true
        ))
        $principalAssociationQueries = @($principalProcessDefinition.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.CommandAst] -and
                    $node.GetCommandName() -eq 'Get-CimAssociatedInstance'
            },
            $true
        ))
        Require `
            ($principalAccountQueries.Count -eq 1 -and
                $principalAccountQueries[0].Extent.Text.Contains(
                    '-ClassName Win32_UserAccount',
                    [System.StringComparison]::Ordinal
                ) -and
                $principalAccountQueries[0].Extent.Text.Contains(
                    'LocalAccount=TRUE',
                    [System.StringComparison]::Ordinal
                ) -and
                $principalAssociationQueries.Count -eq 2 -and
                @($principalAccountQueries + $principalAssociationQueries |
                    Where-Object {
                    @($_.CommandElements | Where-Object {
                        $_ -is [System.Management.Automation.Language.CommandParameterAst] -and
                            $_.ParameterName -eq 'OperationTimeoutSec'
                    }).Count -ne 1
                }).Count -eq 0 -and
                $principalProcessDefinition.Extent.Text.Contains(
                    'Win32_LoggedOnUser',
                    [System.StringComparison]::Ordinal
                ) -and
                $principalProcessDefinition.Extent.Text.Contains(
                    'Win32_SessionProcess',
                    [System.StringComparison]::Ordinal
                ) -and
                -not $principalProcessDefinition.Extent.Text.Contains(
                    'GetOwnerSid',
                    [System.StringComparison]::Ordinal
                )) `
            "Principal-process discovery was not exact-account associated and deadline bounded."

        $principalNativeAssignments = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$principalProcessNativeSource'
            },
            $true
        ))
        Require `
            ($principalNativeAssignments.Count -eq 1) `
            "Expected one cleanup-owned principal-process native source."
        $principalNativeText = $principalNativeAssignments[0].Extent.Text
        foreach ($requiredNativeContract in @(
            'OpenProcess(', 'OpenProcessToken(', 'GetTokenInformation(',
            'TerminateProcess(', 'WaitForSingleObject(', 'CloseHandle(',
            'TokenUser', 'operationFailure', 'closeFailures'
        )) {
            Require `
                ($principalNativeText.Contains(
                    $requiredNativeContract,
                    [System.StringComparison]::Ordinal
                )) `
                "Principal-process native cleanup lost $requiredNativeContract."
        }
        Require `
            (-not $principalNativeText.Contains(
                'GetProcessTimes',
                [System.StringComparison]::Ordinal
            )) `
            "Principal-process cleanup reintroduced a stale PID creation-time split."

        $cleanupDefinitions = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                    $node.Name -eq 'Invoke-Cleanup'
            },
            $true
        ))
        Require ($cleanupDefinitions.Count -eq 1) "Expected one Invoke-Cleanup definition."
        $cleanupDefinition = $cleanupDefinitions[0]
        $cleanupParameters = @($cleanupDefinition.Body.ParamBlock.Parameters)
        Require `
            ($cleanupParameters.Count -eq 2 -and
                $cleanupParameters[0].Name.VariablePath.UserPath -eq
                    'AfterProcessTermination' -and
                $cleanupParameters[1].Name.VariablePath.UserPath -eq
                    'AfterAccountRemoval') `
            "Construction cleanup checkpoints were not internal optional function parameters."
        $topLevelParameters = @($wrapperAst.ParamBlock.Parameters |
            ForEach-Object { $_.Name.VariablePath.UserPath })
        Require `
            ('AfterProcessTermination' -notin $topLevelParameters -and
                'AfterAccountRemoval' -notin $topLevelParameters) `
            "Construction cleanup checkpoint leaked into the production command line."
        $ordinaryCleanupCalls = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.CommandAst] -and
                    $node.GetCommandName() -eq 'Invoke-Cleanup'
            },
            $true
        ))
        Require `
            ($ordinaryCleanupCalls.Count -eq 2 -and
                @($ordinaryCleanupCalls | Where-Object {
                    $_.CommandElements.Count -ne 1
                }).Count -eq 0) `
            "Normal construction cleanup calls unexpectedly selected the recovery checkpoint."
        $cleanupText = $cleanupDefinition.Extent.Text
        $principalProcessCalls = @($cleanupDefinition.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.CommandAst] -and
                    $node.GetCommandName() -eq 'Get-PrincipalProcesses'
            },
            $true
        ))
        Require `
            ($principalProcessCalls.Count -eq 2 -and
                @($principalProcessCalls | Where-Object {
                    -not $_.Extent.Text.Contains(
                        '-Deadline $processDeadline',
                        [System.StringComparison]::Ordinal
                    )
                }).Count -eq 0) `
            "Cleanup did not thread one fixed deadline through every principal-process scan."
        $zeroProcessIndex = $cleanupText.IndexOf(
            '$zeroProcesses = @(',
            [System.StringComparison]::Ordinal
        )
        $checkpointIndex = $cleanupText.IndexOf(
            '$null -ne $AfterProcessTermination)',
            [System.StringComparison]::Ordinal
        )
        $processAbsenceWriteIndex = $cleanupText.IndexOf(
            '$state.stage = "processes_absent"',
            [System.StringComparison]::Ordinal
        )
        $accountRemovalIndex = $cleanupText.IndexOf(
            '$account | Remove-LocalUser -ErrorAction Stop',
            [System.StringComparison]::Ordinal
        )
        $accountCheckpointIndex = $cleanupText.IndexOf(
            'if ($accountAbsent -and $null -ne $AfterAccountRemoval)',
            [System.StringComparison]::Ordinal
        )
        $firewallCleanupIndex = $cleanupText.IndexOf(
            'if ($zeroProcesses -and $accountAbsent)',
            [System.StringComparison]::Ordinal
        )
        $aclCleanupIndex = $cleanupText.IndexOf(
            'foreach ($path in @($state.acl_paths))',
            [System.StringComparison]::Ordinal
        )
        $objectDirectoryAclCleanupIndex = $cleanupText.IndexOf(
            'Remove-ConstructionObjectDirectoryPrincipalAccess',
            [System.StringComparison]::Ordinal
        )
        Require `
            ($zeroProcessIndex -ge 0 -and
                $processAbsenceWriteIndex -gt $zeroProcessIndex -and
                $checkpointIndex -gt $processAbsenceWriteIndex -and
                $objectDirectoryAclCleanupIndex -gt $checkpointIndex -and
                $aclCleanupIndex -gt $objectDirectoryAclCleanupIndex -and
                $accountRemovalIndex -gt $aclCleanupIndex -and
                $accountCheckpointIndex -gt $accountRemovalIndex -and
                $firewallCleanupIndex -gt $accountCheckpointIndex -and
                $cleanupText.Contains('$cleanupErrors.Add("remove-object-directory-acl")') -and
                $cleanupText.Contains('$cleanupErrors.Add("verify-object-directory-acl")')) `
            "Construction cleanup checkpoint was not after exact-SID process absence and before durable cleanup."
        $dotSourceGuards = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.IfStatementAst] -and
                    $node.Clauses.Count -eq 1 -and
                    $node.Clauses[0].Item1.Extent.Text -eq
                        "`$MyInvocation.InvocationName -eq '.'" -and
                    @($node.Clauses[0].Item2.FindAll(
                        {
                            param($child)
                            $child -is [System.Management.Automation.Language.ReturnStatementAst]
                        },
                        $true
                    )).Count -eq 1
            },
            $true
        ))
        Require `
            ($dotSourceGuards.Count -eq 1) `
            "Construction wrapper did not retain its exact dot-source-only load guard."

        Invoke-Expression $principalNativeAssignments[0].Extent.Text
        if (-not ('ProjectAtlasPrincipalProcess' -as [type])) {
            Add-Type -TypeDefinition $principalProcessNativeSource -Language CSharp
        }
        $objectDirectoryAssignments = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$constructionObjectDirectoryAclSource'
            },
            $true
        ))
        Require `
            ($objectDirectoryAssignments.Count -eq 1) `
            "Expected one construction object-directory ACL adapter source."
        Invoke-Expression $objectDirectoryAssignments[0].Extent.Text
        if (-not ('ProjectAtlasConstructionObjectDirectoryAcl' -as [type])) {
            Add-Type -TypeDefinition $constructionObjectDirectoryAclSource -Language CSharp
        }
        $currentProcess = [System.Diagnostics.Process]::GetCurrentProcess()
        $expectedObjectDirectory = if ($currentProcess.SessionId -eq 0) {
            '\BaseNamedObjects'
        }
        else {
            "\Sessions\$($currentProcess.SessionId)\BaseNamedObjects"
        }
        $currentObjectDirectory =
            [ProjectAtlasConstructionObjectDirectoryAcl]::GetCurrentPath()
        Require `
            ($currentObjectDirectory -ceq $expectedObjectDirectory) `
            "Construction object-directory ACL adapter did not target the current session namespace."
        [ProjectAtlasConstructionObjectDirectoryAcl]::AssertExactPrincipalAbsent(
            $currentObjectDirectory,
            'S-1-5-21-0-0-0-0'
        )
        $invalidObjectDirectoryRejected = $false
        try {
            [ProjectAtlasConstructionObjectDirectoryAcl]::AssertExactPrincipalAbsent(
                '\Sessions\01\BaseNamedObjects',
                'S-1-5-21-0-0-0-0'
            )
        }
        catch {
            $invalidObjectDirectoryRejected = $_.Exception.Message -match
                'invalid construction object directory'
        }
        Require `
            $invalidObjectDirectoryRejected `
            "Construction object-directory ACL adapter accepted a non-canonical path."
        $currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $mismatchedSid = if ($currentSid -eq 'S-1-5-18') { 'S-1-5-19' } else { 'S-1-5-18' }
        $currentProcess.Refresh()
        $initialHandleCount = $currentProcess.HandleCount
        foreach ($iteration in 1..32) {
            $terminated = [ProjectAtlasPrincipalProcess]::TerminateExact($currentProcess.Id, $mismatchedSid, 1000)
            if ($terminated -or $currentProcess.HasExited) {
                throw "Token-mismatched pinned handle terminated an unrelated process."
            }
        }
        $currentProcess.Refresh()
        if (($currentProcess.HandleCount - $initialHandleCount) -gt 4) {
            throw "Pinned process termination leaked native handles."
        }

        $terminationStart = [System.Diagnostics.ProcessStartInfo]::new()
        $terminationStart.FileName = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
        $terminationStart.UseShellExecute = $false
        $terminationStart.CreateNoWindow = $true
        foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-Command', 'Start-Sleep -Seconds 30')) {
            $terminationStart.ArgumentList.Add($argument)
        }
        $terminationProcess = $null
        try {
            $terminationProcess = [System.Diagnostics.Process]::Start($terminationStart)
            if ($null -eq $terminationProcess) {
                throw "Could not start the pinned-handle termination canary."
            }
            $terminated = [ProjectAtlasPrincipalProcess]::TerminateExact($terminationProcess.Id, $currentSid, 5000)
            if (-not $terminated -or -not $terminationProcess.WaitForExit(5000) -or
                $null -ne (Get-Process -Id $terminationProcess.Id -ErrorAction SilentlyContinue)) {
                throw "Pinned exact-SID handle did not terminate and reap its process."
            }
        }
        finally {
            if ($null -ne $terminationProcess) {
                if (-not $terminationProcess.HasExited) {
                    $terminationProcess.Kill($true)
                    if (-not $terminationProcess.WaitForExit(5000)) {
                        throw "Fallback pinned-handle termination canary could not be reaped."
                    }
                }
                $terminationProcess.Dispose()
            }
        }

        $timeoutDefinitions = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                    $node.Name -eq 'Get-CimOperationTimeoutSeconds'
            },
            $true
        ))
        if ($timeoutDefinitions.Count -ne 1) {
            throw "Expected one principal-process CIM deadline helper."
        }
        $associationProof = & {
            param([string]$TimeoutDefinition, [string]$DiscoveryDefinition, [string]$ExpectedSid)
            Invoke-Expression $TimeoutDefinition
            Invoke-Expression $DiscoveryDefinition
            $probeState = [pscustomobject]@{
                Mode = 'exact'
                AccountCalls = 0
                AssociationCalls = [System.Collections.Generic.List[string]]::new()
            }
            $account = [pscustomobject]@{ SID = $ExpectedSid; LocalAccount = $true }
            $sessionOne = [pscustomobject]@{ LogonId = '101' }
            $sessionTwo = [pscustomobject]@{ LogonId = '102' }
            $processOne = [pscustomobject]@{ ProcessId = 401 }
            $processTwo = [pscustomobject]@{ ProcessId = 402 }
            function Get-CimInstance {
                [CmdletBinding()]
                param([string]$ClassName, [string]$Filter, [uint32]$OperationTimeoutSec)
                $probeState.AccountCalls += 1
                if ($OperationTimeoutSec -lt 1 -or $ClassName -ne 'Win32_UserAccount' -or
                    -not $Filter.Contains($ExpectedSid)) {
                    throw 'unexpected exact-account query'
                }
                if ($probeState.Mode -eq 'missing') { return @() }
                if ($probeState.Mode -eq 'ambiguous') { return @($account, $account) }
                return $account
            }
            function Get-CimAssociatedInstance {
                [CmdletBinding()]
                param([object]$InputObject, [string]$Association, [string]$ResultClassName, [uint32]$OperationTimeoutSec)
                if ($OperationTimeoutSec -lt 1) { throw 'unbounded association query' }
                $probeState.AssociationCalls.Add($Association)
                if ($Association -eq 'Win32_LoggedOnUser') { return @($sessionOne, $sessionTwo) }
                if ($Association -eq 'Win32_SessionProcess' -and $InputObject.LogonId -eq '101') {
                    return @($processOne)
                }
                if ($Association -eq 'Win32_SessionProcess' -and $InputObject.LogonId -eq '102') {
                    return @($processOne, $processTwo)
                }
                throw 'unexpected association query'
            }
            $exact = @(Get-PrincipalProcesses -Sid $ExpectedSid -Deadline ([DateTime]::UtcNow.AddSeconds(10)))
            $probeState.Mode = 'missing'
            try {
                Get-PrincipalProcesses -Sid $ExpectedSid -Deadline ([DateTime]::UtcNow.AddSeconds(10)) | Out-Null
                $missingFailure = $null
            }
            catch { $missingFailure = $_.Exception.Message }
            $probeState.Mode = 'ambiguous'
            try {
                Get-PrincipalProcesses -Sid $ExpectedSid -Deadline ([DateTime]::UtcNow.AddSeconds(10)) | Out-Null
                $ambiguousFailure = $null
            }
            catch { $ambiguousFailure = $_.Exception.Message }
            $callsBeforeExpired = $probeState.AccountCalls
            try {
                Get-PrincipalProcesses -Sid $ExpectedSid -Deadline ([DateTime]::UtcNow.AddSeconds(-1)) | Out-Null
                $deadlineFailure = $null
            }
            catch { $deadlineFailure = $_.Exception.Message }
            return [pscustomobject]@{
                Exact = @($exact)
                AccountCalls = $probeState.AccountCalls
                CallsBeforeExpired = $callsBeforeExpired
                AssociationCalls = @($probeState.AssociationCalls)
                MissingFailure = $missingFailure
                AmbiguousFailure = $ambiguousFailure
                DeadlineFailure = $deadlineFailure
            }
        } $timeoutDefinitions[0].Extent.Text $principalProcessDefinition.Extent.Text $currentSid
        if ($associationProof.Exact.Count -ne 2 -or
            [int]$associationProof.Exact[0].ProcessId -ne 401 -or
            [int]$associationProof.Exact[1].ProcessId -ne 402 -or
            $associationProof.AssociationCalls.Count -ne 3 -or
            @($associationProof.AssociationCalls | Where-Object {
                $_ -notin @('Win32_LoggedOnUser', 'Win32_SessionProcess')
            }).Count -ne 0 -or
            $associationProof.MissingFailure -ne
                'Could not resolve one exact local account for process cleanup.' -or
            $associationProof.AmbiguousFailure -ne
                'Could not resolve one exact local account for process cleanup.' -or
            $associationProof.DeadlineFailure -ne
                'Principal process discovery reached its cleanup deadline.' -or
            $associationProof.AccountCalls -ne $associationProof.CallsBeforeExpired) {
            throw "Exact-account association weakened identity, bounds, or fail-closed behavior."
        }

        $dotSourceStateDirectory = [System.IO.Directory]::CreateDirectory(
            [System.IO.Path]::Combine(
                $testRoot,
                'dot-source-load',
                'parser-pack-windows-construction-state'
            )
        ).FullName
        $dotSourceStatePath = [System.IO.Path]::Combine(
            $dotSourceStateDirectory,
            'state.json'
        )
        $dotSourceLoaded = & {
            param([string]$ScriptPath, [string]$CleanupStatePath)
            . $ScriptPath -Mode cleanup -StatePath $CleanupStatePath
            return $null -ne (Get-Command Invoke-Cleanup -ErrorAction SilentlyContinue)
        } $WindowsWrapper $dotSourceStatePath
        Require `
            ($dotSourceLoaded -and
                [System.IO.Directory]::Exists($dotSourceStateDirectory)) `
            "Dot-sourcing construction cleanup did not load definitions without cleanup or exit."

        $ordinaryStateDirectory = [System.IO.Directory]::CreateDirectory(
            [System.IO.Path]::Combine(
                $testRoot,
                'ordinary-cleanup',
                'parser-pack-windows-construction-state'
            )
        ).FullName
        $ordinaryStatePath = [System.IO.Path]::Combine(
            $ordinaryStateDirectory,
            'state.json'
        )
        $currentPwsh = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
        & $currentPwsh -NoProfile -File $WindowsWrapper `
            -Mode cleanup `
            -StatePath $ordinaryStatePath
        Require `
            ($LASTEXITCODE -eq 0 -and
                -not [System.IO.Directory]::Exists($ordinaryStateDirectory)) `
            "Ordinary construction cleanup invocation did not reach cleanup unchanged."

        $legacyStatePath = [System.IO.Path]::Combine(
            $testRoot,
            'legacy-journal',
            'parser-pack-windows-construction-state',
            'state.json'
        )
        $readCleanupDefinitions = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                    $node.Name -eq 'Read-CleanupState'
            },
            $true
        ))
        Require ($readCleanupDefinitions.Count -eq 1) "Expected one cleanup-state reader."
        $legacyJournalProof = & {
            param(
                [System.Management.Automation.Language.FunctionDefinitionAst]$Definition,
                [string]$JournalPath
            )
            $stateSchemaVersion = 2
            $StatePath = $JournalPath
            $stateDirectory = Split-Path -Parent $StatePath
            $usernamePattern = '\Apa[0-9a-f]{12}\z'
            $ruleNamePattern = '\AProjectAtlas-ParserPack-Construction-[0-9a-f]{12}\z'
            $sidPattern = '\AS-1-5-21-[0-9]+-[0-9]+-[0-9]+-[0-9]+\z'
            function Assert-StateAcl { param([string]$Path) }
            Invoke-Expression $Definition.Extent.Text
            [System.IO.Directory]::CreateDirectory($stateDirectory) | Out-Null
            $legacy = @{
                schema_version = 1
                username = 'pa0123456789ab'
                sid = 'S-1-5-21-0-0-0-0'
                firewall_rule = 'ProjectAtlas-ParserPack-Construction-0123456789ab'
                acl_paths = @()
                stage = 'identity'
            }
            [System.IO.File]::WriteAllText(
                $StatePath,
                ($legacy | ConvertTo-Json -Compress),
                [System.Text.UTF8Encoding]::new($false)
            )
            $normalized = Read-CleanupState
            $invalid = @{
                schema_version = 2
                username = [string]$normalized.username
                sid = [string]$normalized.sid
                firewall_rule = [string]$normalized.firewall_rule
                object_directory = '\Sessions\01\BaseNamedObjects'
                acl_paths = @()
                stage = 'identity'
            }
            [System.IO.File]::WriteAllText(
                $StatePath,
                ($invalid | ConvertTo-Json -Compress),
                [System.Text.UTF8Encoding]::new($false)
            )
            $invalidFailure = $null
            try {
                Read-CleanupState | Out-Null
            }
            catch {
                $invalidFailure = $_.Exception.Message
            }
            [System.IO.Directory]::Delete($stateDirectory, $true)
            return [pscustomobject]@{
                SchemaVersion = [int]$normalized.schema_version
                ObjectDirectory = [string]$normalized.object_directory
                InvalidFailure = $invalidFailure
            }
        } $readCleanupDefinitions[0] $legacyStatePath
        Require `
            ($legacyJournalProof.SchemaVersion -eq 2 -and
                $legacyJournalProof.ObjectDirectory -eq '' -and
                $legacyJournalProof.InvalidFailure -eq
                    'Construction cleanup state contains invalid values.' -and
                -not [System.IO.Directory]::Exists((Split-Path -Parent $legacyStatePath))) `
            "Construction cleanup journal lost v1 compatibility or exact object-directory validation."

        $checkpointProof = & {
            $placeholderSid = 'S-1-5-21-0-0-0-0'
            $checkpointState = [pscustomobject]@{
                InvokeCheckpoint = $false
                RemoveState = $false
                DurableCleanup = $false
                ReturnMissingState = $false
            }
            $fixtureState = [pscustomobject]@{
                schema_version = 2
                sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
                username = 'projectatlas-cleanup-checkpoint-fixture'
                firewall_rule = 'ProjectAtlas-cleanup-checkpoint-fixture'
                object_directory = ''
                acl_paths = @()
                stage = 'identity'
            }
            $fixtureAccount = [pscustomobject]@{
                Name = $fixtureState.username
                Sid = [System.Security.Principal.SecurityIdentifier]::new(
                    $fixtureState.sid
                )
                Description = 'ProjectAtlas optional parser pack construction'
                Enabled = $false
            }
            function Read-CleanupState {
                if ($checkpointState.ReturnMissingState) {
                    return $null
                }
                return $fixtureState
            }
            function Remove-StateStorage {
                $checkpointState.RemoveState = $true
            }
            function Write-ProtectedState {
                param([hashtable]$State)
                $fixtureState.stage = [string]$State.stage
            }
            function Find-LocalUserBySid { param([string]$Sid) return $fixtureAccount }
            function Find-LocalUserByName { param([string]$Name) return $fixtureAccount }
            function Disable-LocalUser { param($SID) }
            function Initialize-PrincipalProcessNative { return $null }
            function Get-PrincipalProcesses {
                param([string]$Sid, [DateTime]$Deadline)
                return @()
            }
            function Get-CimInstance {
                $checkpointState.DurableCleanup = $true
                throw 'durable-cleanup-started'
            }
            Invoke-Expression $cleanupDefinition.Extent.Text

            $checkpointFailure = $null
            try {
                Invoke-Cleanup -AfterProcessTermination {
                    $checkpointState.InvokeCheckpoint = $true
                    throw 'cleanup-checkpoint-injected'
                }
            }
            catch {
                $checkpointFailure = $_.Exception.Message
            }
            $injectedResult = [pscustomobject]@{
                Invoked = $checkpointState.InvokeCheckpoint
                Failure = $checkpointFailure
                StateRemoved = $checkpointState.RemoveState
                DurableCleanup = $checkpointState.DurableCleanup
            }

            $checkpointState.ReturnMissingState = $true
            $checkpointState.RemoveState = $false
            Invoke-Cleanup
            return [pscustomobject]@{
                Injected = $injectedResult
                NormalMissingStateRemoved = $checkpointState.RemoveState
            }
        }
        Require `
            ($checkpointProof.Injected.Invoked -and
                $checkpointProof.Injected.Failure -eq 'cleanup-checkpoint-injected' -and
                -not $checkpointProof.Injected.StateRemoved -and
                -not $checkpointProof.Injected.DurableCleanup -and
                $checkpointProof.NormalMissingStateRemoved) `
            "Construction cleanup checkpoint did not retain state or preserve normal cleanup behavior."

        $presentAccountDisableFailureProof = & {
            $placeholderSid = 'S-1-5-21-0-0-0-0'
            $proofState = [pscustomobject]@{
                ProcessCheckpoint = $false
                AccountCheckpoint = $false
                AccountRemoved = $false
                StateRewritten = $false
                StateRemoved = $false
            }
            $fixtureState = [pscustomobject]@{
                schema_version = 2
                sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
                username = 'projectatlas-present-account-fixture'
                firewall_rule = 'ProjectAtlas-present-account-fixture'
                object_directory = ''
                acl_paths = @()
                stage = 'processes_absent'
            }
            $fixtureAccount = [pscustomobject]@{
                Name = $fixtureState.username
                Sid = [System.Security.Principal.SecurityIdentifier]::new(
                    $fixtureState.sid
                )
                Description = 'ProjectAtlas optional parser pack construction'
                Enabled = $true
            }
            function Read-CleanupState { return $fixtureState }
            function Remove-StateStorage { $proofState.StateRemoved = $true }
            function Write-ProtectedState { $proofState.StateRewritten = $true }
            function Find-LocalUserBySid { param([string]$Sid) return $fixtureAccount }
            function Find-LocalUserByName { param([string]$Name) return $fixtureAccount }
            function Disable-LocalUser { throw 'disable-injected' }
            function Initialize-PrincipalProcessNative { return $null }
            function Get-PrincipalProcesses {
                param([string]$Sid, [DateTime]$Deadline)
                return @()
            }
            function Get-CimInstance {
                [CmdletBinding()]
                param([string]$ClassName, [string]$Filter)
                return @()
            }
            function Remove-LocalUser { $proofState.AccountRemoved = $true }
            Invoke-Expression $cleanupDefinition.Extent.Text

            $failure = $null
            try {
                Invoke-Cleanup `
                    -AfterProcessTermination {
                        $proofState.ProcessCheckpoint = $true
                    } `
                    -AfterAccountRemoval {
                        $proofState.AccountCheckpoint = $true
                    }
            }
            catch {
                $failure = $_.Exception.Message
            }
            return [pscustomobject]@{
                Failure = $failure
                State = $proofState
            }
        }
        Require `
            ($presentAccountDisableFailureProof.Failure -eq
                'Construction cleanup failed: disable-account,retain-account,retain-firewall.' -and
                -not $presentAccountDisableFailureProof.State.ProcessCheckpoint -and
                -not $presentAccountDisableFailureProof.State.AccountCheckpoint -and
                -not $presentAccountDisableFailureProof.State.AccountRemoved -and
                -not $presentAccountDisableFailureProof.State.StateRewritten -and
                -not $presentAccountDisableFailureProof.State.StateRemoved) `
            "Present account reused stale process-absence authority after disable failure."

        $presentAccountQueryFailureProof = & {
            $placeholderSid = 'S-1-5-21-0-0-0-0'
            $proofState = [pscustomobject]@{
                ProcessCheckpoint = $false
                AccountCheckpoint = $false
                AccountRemoved = $false
                StateRewritten = $false
                StateRemoved = $false
            }
            $fixtureState = [pscustomobject]@{
                schema_version = 2
                sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
                username = 'projectatlas-query-failure-fixture'
                firewall_rule = 'ProjectAtlas-query-failure-fixture'
                object_directory = ''
                acl_paths = @()
                stage = 'processes_absent'
            }
            $fixtureAccount = [pscustomobject]@{
                Name = $fixtureState.username
                Sid = [System.Security.Principal.SecurityIdentifier]::new(
                    $fixtureState.sid
                )
                Description = 'ProjectAtlas optional parser pack construction'
                Enabled = $true
            }
            function Read-CleanupState { return $fixtureState }
            function Remove-StateStorage { $proofState.StateRemoved = $true }
            function Write-ProtectedState { $proofState.StateRewritten = $true }
            function Find-LocalUserBySid { param([string]$Sid) return $fixtureAccount }
            function Find-LocalUserByName { param([string]$Name) return $fixtureAccount }
            function Disable-LocalUser { $fixtureAccount.Enabled = $false }
            function Initialize-PrincipalProcessNative { return $null }
            function Get-PrincipalProcesses { throw 'association-query-injected' }
            function Get-CimInstance {
                [CmdletBinding()]
                param([string]$ClassName, [string]$Filter)
                return @()
            }
            function Remove-LocalUser { $proofState.AccountRemoved = $true }
            Invoke-Expression $cleanupDefinition.Extent.Text

            $failure = $null
            try {
                Invoke-Cleanup `
                    -AfterProcessTermination {
                        $proofState.ProcessCheckpoint = $true
                    } `
                    -AfterAccountRemoval {
                        $proofState.AccountCheckpoint = $true
                    }
            }
            catch {
                $failure = $_.Exception.Message
            }
            return [pscustomobject]@{
                Failure = $failure
                State = $proofState
            }
        }
        Require `
            ($presentAccountQueryFailureProof.Failure -eq
                'Construction cleanup failed: query-principal-processes,retain-account,retain-firewall.' -and
                -not $presentAccountQueryFailureProof.State.ProcessCheckpoint -and
                -not $presentAccountQueryFailureProof.State.AccountCheckpoint -and
                -not $presentAccountQueryFailureProof.State.AccountRemoved -and
                -not $presentAccountQueryFailureProof.State.StateRewritten -and
                -not $presentAccountQueryFailureProof.State.StateRemoved) `
            "Present account reused stale process-absence authority after query failure."

        $currentIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
        $currentPrincipal = [System.Security.Principal.WindowsPrincipal]::new($currentIdentity)
        Require `
            $currentPrincipal.IsInRole($currentIdentity.User) `
            "Current user SID was not effective in its access token."
        Require `
            (-not $currentPrincipal.IsInRole(
                [System.Security.Principal.SecurityIdentifier]::new('S-1-0-0')
            )) `
            "Null SID unexpectedly participated in the access token."

        foreach ($cleanupFunctionName in @(
            "Remove-PrincipalAcl",
            "Assert-PrincipalAclAbsent"
        )) {
            $cleanupDefinitions = @($wrapperAst.FindAll(
                {
                    param($node)
                    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                        $node.Name -eq $cleanupFunctionName
                },
                $true
            ))
            Require ($cleanupDefinitions.Count -eq 1) "Expected one $cleanupFunctionName definition."
            Invoke-Expression $cleanupDefinitions[0].Extent.Text
        }
        Require `
            ($wrapperAst.Extent.Text.Contains('$acl.PurgeAccessRules($Sid)') -and
                $wrapperAst.Extent.Text -match 'GetAccessRules\(\s*\$true,\s*\$true,' -and
                -not $wrapperAst.Extent.Text.Contains('$acl.RemoveAccessRuleSpecific($rule)')) `
            "Construction ACL cleanup did not retain platform purge plus effective post-verification."

        $aclFixture = [System.IO.Path]::Combine($testRoot, "acl-fixture")
        [System.IO.Directory]::CreateDirectory($aclFixture) | Out-Null
        $aclChildFixture = [System.IO.Path]::Combine($aclFixture, "child.txt")
        [System.IO.File]::WriteAllText($aclChildFixture, "fixture")
        $fixtureSid = [System.Security.Principal.SecurityIdentifier]::new(
            "S-1-5-21-3141592653-2718281828-1618033988-424242"
        )
        $unrelatedFixtureSid = [System.Security.Principal.SecurityIdentifier]::new(
            "S-1-5-21-3141592653-2718281828-1618033988-424243"
        )
        $fixtureAcl = Get-Acl -LiteralPath $aclFixture
        $fixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $fixtureSid,
            [System.Security.AccessControl.FileSystemRights]::Write,
            [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
                [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [System.Security.AccessControl.PropagationFlags]::None,
            [System.Security.AccessControl.AccessControlType]::Deny
        ))
        $fixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $fixtureSid,
            [System.Security.AccessControl.FileSystemRights]::ReadAndExecute,
            [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
                [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [System.Security.AccessControl.PropagationFlags]::None,
            [System.Security.AccessControl.AccessControlType]::Allow
        ))
        Set-Acl -LiteralPath $aclFixture -AclObject $fixtureAcl
        $childFixtureAcl = Get-Acl -LiteralPath $aclChildFixture
        $childFixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $fixtureSid,
            [System.Security.AccessControl.FileSystemRights]::WriteData,
            [System.Security.AccessControl.AccessControlType]::Deny
        ))
        $childFixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $fixtureSid,
            [System.Security.AccessControl.FileSystemRights]::Read,
            [System.Security.AccessControl.AccessControlType]::Allow
        ))
        $childFixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $unrelatedFixtureSid,
            [System.Security.AccessControl.FileSystemRights]::Read,
            [System.Security.AccessControl.AccessControlType]::Allow
        ))
        Set-Acl -LiteralPath $aclChildFixture -AclObject $childFixtureAcl
        $unrelatedRulesBefore = @(
            (Get-Acl -LiteralPath $aclChildFixture).GetAccessRules(
                $true,
                $true,
                [System.Security.Principal.SecurityIdentifier]
            ) | Where-Object { $_.IdentityReference -eq $unrelatedFixtureSid }
        )
        Require `
            ($unrelatedRulesBefore.Count -eq 1) `
            "Construction ACL fixture did not retain one unrelated principal rule."
        $unrelatedRuleBefore = $unrelatedRulesBefore[0]
        Require `
            (@(
                (Get-Acl -LiteralPath $aclChildFixture).GetAccessRules(
                    $true,
                    $true,
                    [System.Security.Principal.SecurityIdentifier]
                ) | Where-Object { $_.IdentityReference -eq $fixtureSid }
            ).Count -ge 4) `
            "Construction ACL fixture did not contain explicit and inherited target rules."

        Remove-PrincipalAcl -Path $aclChildFixture -Sid $fixtureSid
        Remove-PrincipalAcl -Path $aclFixture -Sid $fixtureSid
        Assert-PrincipalAclAbsent -Path $aclFixture -Sid $fixtureSid
        Assert-PrincipalAclAbsent -Path $aclChildFixture -Sid $fixtureSid
        $unrelatedRules = @(
            (Get-Acl -LiteralPath $aclChildFixture).GetAccessRules(
                $true,
                $true,
                [System.Security.Principal.SecurityIdentifier]
            ) | Where-Object { $_.IdentityReference -eq $unrelatedFixtureSid }
        )
        Require `
            ($unrelatedRules.Count -eq 1 -and
                $unrelatedRules[0].IdentityReference -eq
                    $unrelatedRuleBefore.IdentityReference -and
                $unrelatedRules[0].FileSystemRights -eq
                    $unrelatedRuleBefore.FileSystemRights -and
                $unrelatedRules[0].AccessControlType -eq
                    $unrelatedRuleBefore.AccessControlType -and
                $unrelatedRules[0].IsInherited -eq
                    $unrelatedRuleBefore.IsInherited -and
                $unrelatedRules[0].InheritanceFlags -eq
                    $unrelatedRuleBefore.InheritanceFlags -and
                $unrelatedRules[0].PropagationFlags -eq
                    $unrelatedRuleBefore.PropagationFlags) `
            "Construction ACL cleanup removed an unrelated principal."
        Remove-PrincipalAcl -Path $aclChildFixture -Sid $fixtureSid
        Remove-PrincipalAcl -Path $aclFixture -Sid $fixtureSid
        Assert-PrincipalAclAbsent -Path $aclFixture -Sid $fixtureSid
        Assert-PrincipalAclAbsent -Path $aclChildFixture -Sid $fixtureSid

        $jobserverDefinitions = @($ast.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                    $node.Name -eq "Open-ContainedCargoJobserver"
            },
            $true
        ))
        Require `
            ($jobserverDefinitions.Count -eq 1) `
            "Contained construction lost protected seeded jobserver opening."
        $jobserverCanaryDefinitions = @($ast.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                    $node.Name -eq 'Invoke-ContainedCargoJobserverCanary'
            },
            $true
        ))
        Require `
            ($jobserverCanaryDefinitions.Count -eq 1 -and
                $jobserverCanaryDefinitions[0].Extent.Text.Contains('EntryPoint = "OpenSemaphoreW"') -and
                $jobserverCanaryDefinitions[0].Extent.Text.Contains('SynchronizeAndModify = 0x00100002')) `
            "Contained Cargo jobserver lost its exact-rights descendant canary."
        $jobserverDefinitionText = $jobserverDefinitions[0].Extent.Text
        Require `
            ($jobserverDefinitionText.Contains('$identity.Owner.Value') -and
                $jobserverDefinitionText.Contains('requires the construction SID as the token default owner') -and
                $jobserverDefinitionText.Contains('public static class ProjectAtlasCargoJobserverNative') -and
                $jobserverDefinitionText.Contains('SynchronizeAndModify = 0x00100002') -and
                $jobserverDefinitionText.Contains('OpenExisting(string name)') -and
                $jobserverDefinitionText.Contains('RequireExistingObject(string name)') -and
                $jobserverDefinitionText.Contains('EntryPoint = "OpenSemaphoreW"') -and
                $jobserverDefinitionText.Contains('if (createError != ErrorAlreadyExists)') -and
                -not $jobserverDefinitionText.Contains('InheritHandle = true')) `
            "Contained Cargo jobserver did not retain exact-rights named-open validation."
        Invoke-Expression $jobserverDefinitionText
        Invoke-Expression $jobserverCanaryDefinitions[0].Extent.Text
        $currentIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
        $currentSid = $currentIdentity.User
        $jobserverSecuritySid = $currentIdentity.Owner
        Require `
            ($null -ne $jobserverSecuritySid) `
            "Diagnostic token did not expose a default owner SID."
        if (-not [string]::Equals(
                $currentSid.Value,
                $jobserverSecuritySid.Value,
                [System.StringComparison]::Ordinal
            )) {
            $ownerMismatchRejected = $false
            try {
                $unexpectedOwnerJobserver = Open-ContainedCargoJobserver `
                    -Sid $currentSid `
                    -Name "Local\ProjectAtlasParserPack-$([guid]::NewGuid().ToString('N'))"
                $unexpectedOwnerJobserver.Dispose()
            }
            catch {
                $ownerMismatchRejected = $_.Exception.Message -eq
                    'Contained Cargo jobserver requires the construction SID as the token default owner.'
            }
            Require `
                $ownerMismatchRejected `
                "Contained Cargo jobserver accepted a non-owner construction SID."
        }
        $jobserverRights = [System.Security.AccessControl.SemaphoreRights]::Synchronize -bor
            [System.Security.AccessControl.SemaphoreRights]::Modify
        $jobserverName = "Local\ProjectAtlasParserPack-$([guid]::NewGuid().ToString('N'))"
        $jobserverSecurity = [System.Security.AccessControl.SemaphoreSecurity]::new()
        $jobserverSecurity.SetAccessRuleProtection($true, $false)
        $jobserverSecurity.AddAccessRule(
            [System.Security.AccessControl.SemaphoreAccessRule]::new(
                $jobserverSecuritySid,
                $jobserverRights,
                [System.Security.AccessControl.AccessControlType]::Allow
            )
        )
        $jobserverCreatedNew = $false
        $jobserverSeed = [System.Threading.SemaphoreAcl]::Create(
            1,
            1,
            $jobserverName,
            [ref]$jobserverCreatedNew,
            $jobserverSecurity
        )
        Require $jobserverCreatedNew "Diagnostic Cargo jobserver seed collided."
        $jobserver = Open-ContainedCargoJobserver `
            -Sid $jobserverSecuritySid `
            -Name $jobserverName
        $openedJobserver = $null
        try {
            $openedJobserver = [System.Threading.SemaphoreAcl]::OpenExisting(
                $jobserverName,
                $jobserverRights
            )
            $Target = 'x86_64-pc-windows-msvc'
            $commandDiagnosticTailBytes = 24 * 1024
            $constructionDiagnosticMaxBytes = 64 * 1024
            $script:constructionFailureRecorded = $false
            $script:constructionFailureExitCode = 1
            $script:constructionDiagnosticPath = [System.IO.Path]::Combine(
                $testRoot,
                'jobserver-canary-diagnostic.txt'
            )
            $jobserverCanaryPath = [System.IO.Path]::Combine(
                $testRoot,
                'jobserver-canary.ps1'
            )
            Invoke-ContainedCargoJobserverCanary `
                -Pwsh ([System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName) `
                -Name $jobserverName `
                -Sid $currentSid `
                -Path $jobserverCanaryPath
            Require `
                (-not [System.IO.File]::Exists($jobserverCanaryPath)) `
                "Contained Cargo jobserver descendant canary left its script."
            Require `
                ($openedJobserver.WaitOne(0) -and $openedJobserver.Release() -eq 0) `
                "Contained Cargo jobserver did not restore its token."
            $absentSeedRejected = $false
            try {
                $unexpectedJobserver = Open-ContainedCargoJobserver `
                    -Sid $jobserverSecuritySid `
                    -Name "Local\ProjectAtlasParserPack-$([guid]::NewGuid().ToString('N'))"
                $unexpectedJobserver.Dispose()
            }
            catch {
                $absentSeedRejected = $true
            }
            Require $absentSeedRejected "Contained Cargo jobserver fabricated an absent seed."
            foreach ($forbiddenName in @(
                "ProjectAtlasParserPack-$([guid]::NewGuid().ToString('N'))",
                "Global\ProjectAtlasParserPack-$([guid]::NewGuid().ToString('N'))",
                "local\ProjectAtlasParserPack-$([guid]::NewGuid().ToString('N'))"
            )) {
                $nonLocalNameRejected = $false
                try {
                    $unexpectedNonLocalJobserver = Open-ContainedCargoJobserver `
                        -Sid $jobserverSecuritySid `
                        -Name $forbiddenName
                    $unexpectedNonLocalJobserver.Dispose()
                }
                catch {
                    $nonLocalNameRejected = $true
                }
                Require `
                    $nonLocalNameRejected `
                    "Contained Cargo jobserver accepted a non-local namespace."
            }
        }
        finally {
            if ($null -ne $openedJobserver) {
                $openedJobserver.Dispose()
            }
            $jobserver.Dispose()
            $jobserverSeed.Dispose()
        }
        $postCleanupJobserver = $null
        $jobserverNameRemoved = -not [System.Threading.SemaphoreAcl]::TryOpenExisting(
            $jobserverName,
            $jobserverRights,
            [ref]$postCleanupJobserver
        )
        if ($null -ne $postCleanupJobserver) {
            $postCleanupJobserver.Dispose()
        }
        Require $jobserverNameRemoved "Contained Cargo jobserver survived its last owned handle."
        $validateConstructionToken = $adapterType.GetMethod(
            'ValidateConstructionToken',
            $nestedTypeFlags
        )
        Require `
            ($null -ne $validateConstructionToken) `
            "Native adapter construction token boundary was missing."
        $currentGroupCsv = @(whoami.exe /groups /fo csv /nh)
        Require ($LASTEXITCODE -eq 0) "Could not inspect the diagnostic token integrity."
        $currentIntegritySids = @(
            $currentGroupCsv |
                ConvertFrom-Csv -Header Name, Type, Sid, Attributes |
                Where-Object { $_.Sid -like 'S-1-16-*' } |
                ForEach-Object { $_.Sid }
        )
        Require `
            ($currentIntegritySids.Count -eq 1) `
            "The diagnostic token did not expose one integrity SID."
        $currentIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent(
            [System.Security.Principal.TokenAccessLevels]::Query
        )
        try {
            $currentToken = $currentIdentity.AccessToken
            $currentPrincipalSid = $currentIdentity.User.Value
            if ($currentIntegritySids[0] -eq 'S-1-16-8192') {
                $currentLogonSid = [string]$validateConstructionToken.Invoke(
                    $null,
                    [object[]]@($currentToken, $currentPrincipalSid)
                )
                Require `
                    (-not [string]::IsNullOrEmpty($currentLogonSid)) `
                    "Native adapter did not validate the current medium-integrity token."
            }
            else {
                $unsupportedIntegrityRejected = $false
                try {
                    $validateConstructionToken.Invoke(
                        $null,
                        [object[]]@($currentToken, $currentPrincipalSid)
                    ) | Out-Null
                }
                catch {
                    $failure = $_.Exception
                    while ($null -ne $failure) {
                        if ($failure.Message -eq 'validate-construction-token-integrity') {
                            $unsupportedIntegrityRejected = $true
                            break
                        }
                        $failure = $failure.InnerException
                    }
                }
                Require `
                    $unsupportedIntegrityRejected `
                    "Native adapter did not reject the ambient non-medium diagnostic token exactly."
            }
            $mismatchedTokenRejected = $false
            try {
                $validateConstructionToken.Invoke(
                    $null,
                    [object[]]@($currentToken, 'S-1-5-7')
                ) | Out-Null
            }
            catch {
                $mismatchedTokenRejected = $true
            }
            Require $mismatchedTokenRejected "Native adapter accepted a mismatched construction token."
        }
        finally {
            $currentIdentity.Dispose()
        }
    }
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
    $script:constructionFailureExitCode = 1
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
            $status.exit_code -eq 7 -and
            $script:constructionFailureExitCode -eq 7) `
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
