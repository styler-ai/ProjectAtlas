[CmdletBinding()]
param(
    [string]$ProductionScript = (Join-Path $PSScriptRoot "run-parser-pack-contained-construction.ps1"),
    [string]$WindowsWrapper = (Join-Path $PSScriptRoot "invoke-parser-pack-windows-construction.ps1")
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
        Require `
            ($wrapperText.Contains(
                '"Local\ProjectAtlasParserPack-$randomHex"',
                [System.StringComparison]::Ordinal
            )) `
            "Windows construction jobserver did not use the exact session-local prefix."
        Require `
            (-not $wrapperText.Contains(
                '"Global\ProjectAtlasParserPack-',
                [System.StringComparison]::Ordinal
            )) `
            "Windows construction jobserver retained the machine-global prefix."
        Require `
            (-not $wrapperText.Contains(
                '"ProjectAtlasParserPack-$randomHex"',
                [System.StringComparison]::Ordinal
            )) `
            "Windows construction jobserver used an implicit namespace."
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
            (-not $nativeSource.Contains('LogonWithProfile') -and
                $nativeSource -match 'passwordPointer,\s*0,\s*executable,') `
            "Windows construction adapter did not retain zero process logon flags."
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
                'Normal,FailBeforeJobAssignment,FailBeforeJobAssignmentAndCleanupFailure') `
            "Construction admission failure domain was not closed."

        $publicRunMethods = @($adapterType.GetMethods(
            [System.Reflection.BindingFlags]'Public,Static'
        ) | Where-Object Name -eq 'Run')
        Require `
            ($publicRunMethods.Count -eq 1 -and
                $publicRunMethods[0].GetParameters().Count -eq 8) `
            "Construction adapter exposed an admission fault through its public launch API."
        $processCreatedIndex = $nativeSource.IndexOf(
            'processCreated = true;',
            [System.StringComparison]::Ordinal
        )
        $admissionFailureIndex = $nativeSource.IndexOf(
            'if (admissionScenario != AdmissionScenario.Normal)',
            [System.StringComparison]::Ordinal
        )
        $jobAssignmentIndex = $nativeSource.IndexOf(
            'if (!AssignProcessToJobObject(job, process.Process))',
            [System.StringComparison]::Ordinal
        )
        Require `
            ($processCreatedIndex -ge 0 -and
                $admissionFailureIndex -gt $processCreatedIndex -and
                $jobAssignmentIndex -gt $admissionFailureIndex -and
                $nativeSource.Contains(
                    'uint flags = CreateSuspended | CreateNoWindow | CreateUnicodeEnvironment;'
                ) -and
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
            'ProjectAtlasCurrentProcessTokenRestrictionProbe',
            'ProjectAtlasObjectDirectoryProbe',
            '\BaseNamedObjects'
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
            ($probeSource.Contains('ReadToEndAsync()') -and
                $probeSource.Contains('$expectedCargoMakeflags') -and
                -not $probeSource.Contains('StandardOutput.ReadToEnd()')) `
            "Construction boundary probe did not drain both streams asynchronously or verify jobserver transport."
        Require `
            ($probeSource.Contains('$ExpectedSessionId') -and
                $probeSource.Contains('S-1-16-8192') -and
                $probeSource.Contains('$principal.IsInRole($expectedSecurityIdentifier)') -and
                $probeSource.Contains('$synchronizeJobserver = [System.Threading.SemaphoreAcl]::OpenExisting(') -and
                $probeSource.Contains('$modifyJobserver = [System.Threading.SemaphoreAcl]::OpenExisting(')) `
            "Construction boundary probe did not classify SID membership, session, integrity, and individual jobserver rights."
        Require `
            ($wrapperAst.Extent.Text.Contains('26 { "jobserver-synchronize-access" }') -and
                $wrapperAst.Extent.Text.Contains('28 { "jobserver-modify-access" }') -and
                $wrapperAst.Extent.Text.Contains('33 { "jobserver-combined-access" }') -and
                $wrapperAst.Extent.Text.Contains('37 { "target-sid-membership-query" }') -and
                $wrapperAst.Extent.Text.Contains('38 { "target-sid-not-effective" }')) `
            "Construction boundary probe did not retain distinct jobserver access diagnostics."
        $wrapperText = $wrapperAst.Extent.Text
        $sessionCheckIndex = $probeSource.IndexOf(
            '[System.Diagnostics.Process]::GetCurrentProcess().SessionId -ne $ExpectedSessionId',
            [System.StringComparison]::Ordinal
        )
        $firstJobserverOpenIndex = $probeSource.IndexOf(
            '[System.Threading.SemaphoreAcl]::OpenExisting(',
            [System.StringComparison]::Ordinal
        )
        Require `
            ($sessionCheckIndex -ge 0 -and
                $firstJobserverOpenIndex -gt $sessionCheckIndex -and
                $probeSource.Contains('catch [System.UnauthorizedAccessException] {') -and
                $probeSource.Contains('exit 26') -and
                -not $probeSource.Contains('$ObjectDirectoryProbePath') -and
                -not $probeSource.Contains('$TokenRestrictionProbePath')) `
            "Session-local jobserver proof did not remain exact and self-contained."

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
        $boundedCimCalls = @($principalProcessDefinition.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.CommandAst] -and
                    $node.GetCommandName() -in @('Get-CimInstance', 'Invoke-CimMethod')
            },
            $true
        ))
        Require `
            ($boundedCimCalls.Count -eq 3 -and
                @($boundedCimCalls | Where-Object {
                    @($_.CommandElements | Where-Object {
                        $_ -is [System.Management.Automation.Language.CommandParameterAst] -and
                            $_.ParameterName -eq 'OperationTimeoutSec'
                    }).Count -ne 1
                }).Count -eq 0) `
            "Principal-process CIM operations were not bounded by the cleanup deadline."

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
            ($cleanupParameters.Count -eq 1 -and
                $cleanupParameters[0].Name.VariablePath.UserPath -eq
                    'AfterProcessTermination') `
            "Construction cleanup checkpoint was not one internal optional function parameter."
        $topLevelParameters = @($wrapperAst.ParamBlock.Parameters |
            ForEach-Object { $_.Name.VariablePath.UserPath })
        Require `
            ('AfterProcessTermination' -notin $topLevelParameters) `
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
            'if ($zeroProcesses -and $null -ne $AfterProcessTermination)',
            [System.StringComparison]::Ordinal
        )
        $aclCleanupIndex = $cleanupText.IndexOf(
            'foreach ($path in @($state.acl_paths))',
            [System.StringComparison]::Ordinal
        )
        Require `
            ($zeroProcessIndex -ge 0 -and
                $checkpointIndex -gt $zeroProcessIndex -and
                $aclCleanupIndex -gt $checkpointIndex) `
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

        $raceProbeStart = [System.Diagnostics.ProcessStartInfo]::new()
        $raceProbeStart.FileName =
            [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
        $raceProbeStart.UseShellExecute = $false
        $raceProbeStart.CreateNoWindow = $true
        foreach ($argument in @(
            '-NoLogo', '-NoProfile', '-NonInteractive', '-Command',
            'Start-Sleep -Seconds 60'
        )) {
            $raceProbeStart.ArgumentList.Add($argument)
        }
        $raceProbe = $null
        try {
            $raceProbe = [System.Diagnostics.Process]::Start($raceProbeStart)
            Require ($null -ne $raceProbe) "Could not start the process-owner race probe."
            $snapshotDeadline = [DateTime]::UtcNow.AddSeconds(10)
            $staleSnapshot = @()
            do {
                $staleSnapshot = @(
                    CimCmdlets\Get-CimInstance `
                        -ClassName Win32_Process `
                        -Filter "ProcessId = $($raceProbe.Id)" `
                        -OperationTimeoutSec 5 `
                        -ErrorAction Stop
                )
                if ($staleSnapshot.Count -eq 0) {
                    Start-Sleep -Milliseconds 100
                }
            } while ($staleSnapshot.Count -eq 0 -and
                [DateTime]::UtcNow -lt $snapshotDeadline)
            Require `
                ($staleSnapshot.Count -eq 1) `
                "Could not snapshot the process-owner race probe."
            $raceProbe.Kill($true)
            Require `
                ($raceProbe.WaitForExit(5000) -and
                    $null -eq (Get-Process -Id $raceProbe.Id -ErrorAction SilentlyContinue)) `
                "Process-owner race probe could not be reaped."

            $staleOwnerFailure = $null
            try {
                CimCmdlets\Invoke-CimMethod `
                    -InputObject $staleSnapshot[0] `
                    -MethodName GetOwnerSid `
                    -OperationTimeoutSec 5 `
                    -ErrorAction Stop | Out-Null
            }
            catch {
                $staleOwnerFailure = $_.Exception
            }
            Require `
                ($staleOwnerFailure -is [Microsoft.Management.Infrastructure.CimException]) `
                "Exited process snapshot did not reproduce the GetOwnerSid race."

            $vanishedProcessProof = & {
                param(
                    [string]$Definition,
                    [Microsoft.Management.Infrastructure.CimInstance]$Snapshot
                )
                Invoke-Expression $Definition
                $capturedTimeouts = [System.Collections.Generic.List[uint32]]::new()
                function Get-CimInstance {
                    [CmdletBinding()]
                    param(
                        [string]$ClassName,
                        [string]$Filter,
                        [uint32]$OperationTimeoutSec
                    )
                    $capturedTimeouts.Add($OperationTimeoutSec)
                    if (-not [string]::IsNullOrEmpty($Filter)) {
                        return @()
                    }
                    return $Snapshot
                }
                $owned = @(
                    Get-PrincipalProcesses `
                        -Sid ([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value) `
                        -Deadline ([DateTime]::UtcNow.AddSeconds(10))
                )
                return [pscustomobject]@{
                    OwnedCount = $owned.Count
                    Timeouts = @($capturedTimeouts)
                }
            } $principalProcessDefinition.Extent.Text $staleSnapshot[0]
            Require `
                ($vanishedProcessProof.OwnedCount -eq 0 -and
                    $vanishedProcessProof.Timeouts.Count -eq 2 -and
                    @($vanishedProcessProof.Timeouts | Where-Object {
                        $_ -lt 1 -or $_ -gt 10
                    }).Count -eq 0) `
                "Principal-process scan did not tolerate only the reaped snapshot race."
        }
        finally {
            if ($null -ne $raceProbe) {
                if (-not $raceProbe.HasExited) {
                    $raceProbe.Kill($true)
                    if (-not $raceProbe.WaitForExit(5000)) {
                        throw "Fallback process-owner race probe termination could not be reaped."
                    }
                }
                $raceProbe.Dispose()
            }
        }

        $liveProcessFailureProof = & {
            param([string]$Definition)
            Invoke-Expression $Definition
            $probeState = [pscustomobject]@{ EnumerationCalls = 0 }
            function Get-CimInstance {
                [CmdletBinding()]
                param(
                    [string]$ClassName,
                    [string]$Filter,
                    [uint32]$OperationTimeoutSec
                )
                $probeState.EnumerationCalls += 1
                return [pscustomobject]@{
                    ProcessId = [System.Diagnostics.Process]::GetCurrentProcess().Id
                }
            }
            function Invoke-CimMethod {
                [CmdletBinding()]
                param(
                    [object]$InputObject,
                    [string]$MethodName,
                    [uint32]$OperationTimeoutSec
                )
                return [pscustomobject]@{ ReturnValue = 5; Sid = $null }
            }

            $liveFailure = $null
            try {
                Get-PrincipalProcesses `
                    -Sid ([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value) `
                    -Deadline ([DateTime]::UtcNow.AddSeconds(10)) | Out-Null
            }
            catch {
                $liveFailure = $_.Exception
            }
            $enumerationCallsBeforeExpiredScan = $probeState.EnumerationCalls
            $deadlineFailure = $null
            try {
                Get-PrincipalProcesses `
                    -Sid ([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value) `
                    -Deadline ([DateTime]::UtcNow.AddSeconds(-1)) | Out-Null
            }
            catch {
                $deadlineFailure = $_.Exception
            }
            return [pscustomobject]@{
                LiveFailure = $liveFailure
                DeadlineFailure = $deadlineFailure
                EnumerationCallsBeforeExpiredScan = $enumerationCallsBeforeExpiredScan
                EnumerationCallsAfterExpiredScan = $probeState.EnumerationCalls
            }
        } $principalProcessDefinition.Extent.Text
        Require `
            ($liveProcessFailureProof.LiveFailure -is [System.InvalidOperationException] -and
                $liveProcessFailureProof.LiveFailure.Message -eq
                    'Could not prove the owner of one running process.' -and
                $liveProcessFailureProof.LiveFailure.InnerException -is
                    [System.InvalidOperationException] -and
                $liveProcessFailureProof.LiveFailure.InnerException.Message -eq
                    'Process owner query returned a nonzero status.' -and
                $liveProcessFailureProof.DeadlineFailure.Message -eq
                    'Process ownership scan reached its cleanup deadline.' -and
                $liveProcessFailureProof.EnumerationCallsBeforeExpiredScan -eq 2 -and
                $liveProcessFailureProof.EnumerationCallsAfterExpiredScan -eq 2) `
            "Principal-process scan weakened live-PID failure or its fixed deadline."

        $requeryFailureProof = & {
            param([string]$Definition)
            Invoke-Expression $Definition
            $probeState = [pscustomobject]@{ EnumerationCalls = 0 }
            function Get-CimInstance {
                [CmdletBinding()]
                param(
                    [string]$ClassName,
                    [string]$Filter,
                    [uint32]$OperationTimeoutSec
                )
                $probeState.EnumerationCalls += 1
                if (-not [string]::IsNullOrEmpty($Filter)) {
                    throw 'exact PID requery failed'
                }
                return [pscustomobject]@{
                    ProcessId = [System.Diagnostics.Process]::GetCurrentProcess().Id
                }
            }
            function Invoke-CimMethod {
                [CmdletBinding()]
                param(
                    [object]$InputObject,
                    [string]$MethodName,
                    [uint32]$OperationTimeoutSec
                )
                return [pscustomobject]@{ ReturnValue = 5; Sid = $null }
            }
            try {
                Get-PrincipalProcesses `
                    -Sid ([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value) `
                    -Deadline ([DateTime]::UtcNow.AddSeconds(10)) | Out-Null
            }
            catch {
                return [pscustomobject]@{
                    Failure = $_.Exception
                    EnumerationCalls = $probeState.EnumerationCalls
                }
            }
            throw 'Expected the exact-PID requery probe to fail.'
        } $principalProcessDefinition.Extent.Text
        Require `
            ($requeryFailureProof.Failure -is [System.AggregateException] -and
                $requeryFailureProof.Failure.Message.StartsWith(
                    'Could not requery one process after its owner query failed.',
                    [System.StringComparison]::Ordinal
                ) -and
                $requeryFailureProof.Failure.InnerExceptions.Count -eq 2 -and
                $requeryFailureProof.Failure.InnerExceptions[0].Message -eq
                    'Process owner query returned a nonzero status.' -and
                $requeryFailureProof.Failure.InnerExceptions[1].Message -eq
                    'exact PID requery failed' -and
                $requeryFailureProof.EnumerationCalls -eq 2) `
            "Principal-process scan did not preserve owner and exact-PID requery failures."

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

        $checkpointProof = & {
            $placeholderSid = 'S-1-5-21-0-0-0-0'
            $checkpointState = [pscustomobject]@{
                InvokeCheckpoint = $false
                RemoveState = $false
                DurableCleanup = $false
                ReturnMissingState = $false
            }
            $fixtureState = [pscustomobject]@{
                sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
                username = 'projectatlas-cleanup-checkpoint-fixture'
                firewall_rule = 'ProjectAtlas-cleanup-checkpoint-fixture'
                acl_paths = @()
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
            function Find-LocalUserBySid { param([string]$Sid) return $null }
            function Find-LocalUserByName { param([string]$Name) return $null }
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

        foreach ($functionName in @(
            "New-ConstructionJobserverSecurity",
            "New-ConstructionJobserver"
        )) {
            $jobserverDefinitions = @($wrapperAst.FindAll(
                {
                    param($node)
                    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                        $node.Name -eq $functionName
                },
                $true
            ))
            Require ($jobserverDefinitions.Count -eq 1) "Expected one $functionName definition."
            Invoke-Expression $jobserverDefinitions[0].Extent.Text
        }

        $currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
        $jobserverRights = [System.Security.AccessControl.SemaphoreRights]::Synchronize -bor
            [System.Security.AccessControl.SemaphoreRights]::Modify
        $jobserverSecurity = New-ConstructionJobserverSecurity -Sid $currentSid
        $jobserverRules = @($jobserverSecurity.GetAccessRules(
            $true,
            $false,
            [System.Security.Principal.SecurityIdentifier]
        ))
        Require $jobserverSecurity.AreAccessRulesProtected "Construction jobserver DACL was not protected."
        Require ($jobserverRules.Count -eq 1) "Construction jobserver DACL was not target-only."
        Require `
            ($jobserverRules[0].IdentityReference -eq $currentSid -and
                $jobserverRules[0].SemaphoreRights -eq $jobserverRights -and
                $jobserverRules[0].AccessControlType -eq
                    [System.Security.AccessControl.AccessControlType]::Allow) `
            "Construction jobserver DACL did not grant the exact target rights."
        $jobserverName = "Local\ProjectAtlasJobserverTest-$([guid]::NewGuid().ToString('N'))"
        $jobserver = New-ConstructionJobserver -Sid $currentSid -Name $jobserverName
        $openedJobserver = $null
        $daclReader = $null
        try {
            Require `
                ([ProjectAtlasConstructionProcess]::HasMediumMandatoryLabel(
                    $jobserver.SafeWaitHandle
                )) `
                "Construction jobserver did not retain its medium mandatory label."
            $daclReader = [System.Threading.SemaphoreAcl]::OpenExisting(
                $jobserverName,
                [System.Security.AccessControl.SemaphoreRights]::ReadPermissions
            )
            Require `
                ([ProjectAtlasConstructionProcess]::HasExpectedJobserverDacl(
                    $daclReader.SafeWaitHandle,
                    $currentSid.Value
                )) `
                "Construction jobserver did not retain its target-only DACL."
            $openedJobserver = [System.Threading.SemaphoreAcl]::OpenExisting(
                $jobserverName,
                $jobserverRights
            )
            Require ($openedJobserver.WaitOne(0)) "Construction jobserver did not expose one token."
            Require `
                ($openedJobserver.Release() -eq 0) `
                "Construction jobserver did not restore its token."
            $collisionRejected = $false
            try {
                $unexpectedJobserver = New-ConstructionJobserver `
                    -Sid $currentSid `
                    -Name $jobserverName
                $unexpectedJobserver.Dispose()
            }
            catch {
                $collisionRejected = $true
            }
            Require $collisionRejected "Construction jobserver accepted a live-name collision."
        }
        finally {
            if ($null -ne $daclReader) {
                $daclReader.Dispose()
            }
            if ($null -ne $openedJobserver) {
                $openedJobserver.Dispose()
            }
            $jobserver.Dispose()
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
