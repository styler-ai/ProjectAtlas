[CmdletBinding()]
param(
    [string]$ProductionWrapper =
        (Join-Path $PSScriptRoot "invoke-parser-pack-windows-construction.ps1"),

    [string]$LauncherPath,

    [hashtable]$ConstructionParameters,

    [string]$BrokerJobName,

    [string]$RecoveryRoot,

    [string]$RunnerTemporaryRoot,

    [switch]$StaticOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$accountDescription = "ProjectAtlas optional parser pack construction"
$placeholderSid = "S-1-5-21-0-0-0-0"
$usernamePattern = '\Apa[0-9a-f]{12}\z'
$ruleNamePattern = '\AProjectAtlas-ParserPack-Construction-[0-9a-f]{12}\z'
$sidPattern = '\AS-1-5-21-[0-9]+-[0-9]+-[0-9]+-[0-9]+\z'
$processTimeoutSeconds = 60
$exactSidAuditSource = @'
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Sid,

    [Parameter(Mandatory = $true)]
    [ValidateSet("present", "absent")]
    [string]$Expectation,

    [ValidateRange(0, 60000)]
    [int]$DelayMilliseconds = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if ($DelayMilliseconds -gt 0) {
    Start-Sleep -Milliseconds $DelayMilliseconds
}
$auditNativeSource = @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;

public static class ProjectAtlasWtsProcessAudit
{
    [StructLayout(LayoutKind.Sequential)]
    private struct WtsProcessInfo
    {
        internal uint SessionId;
        internal uint ProcessId;
        internal IntPtr ProcessName;
        internal IntPtr UserSid;
    }

    [DllImport("Wtsapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool WTSEnumerateProcessesW(
        IntPtr server,
        uint reserved,
        uint version,
        out IntPtr processInformation,
        out uint count);

    [DllImport("Wtsapi32.dll")]
    private static extern void WTSFreeMemory(IntPtr memory);

    public static int CountExactSid(string expectedSid)
    {
        SecurityIdentifier expected = new SecurityIdentifier(expectedSid);
        IntPtr processInformation = IntPtr.Zero;
        uint count = 0;
        if (!WTSEnumerateProcessesW(
                IntPtr.Zero,
                0,
                1,
                out processInformation,
                out count))
        {
            throw new Win32Exception(
                Marshal.GetLastWin32Error(),
                "enumerate-wts-processes");
        }
        try
        {
            int matches = 0;
            int size = Marshal.SizeOf<WtsProcessInfo>();
            for (uint index = 0; index < count; index++)
            {
                IntPtr rowAddress = IntPtr.Add(
                    processInformation,
                    checked((int)index * size));
                WtsProcessInfo row = Marshal.PtrToStructure<WtsProcessInfo>(rowAddress);
                if (row.UserSid != IntPtr.Zero &&
                    new SecurityIdentifier(row.UserSid).Equals(expected))
                {
                    matches++;
                }
            }
            return matches;
        }
        finally
        {
            if (processInformation != IntPtr.Zero)
            {
                WTSFreeMemory(processInformation);
            }
        }
    }
}
"@
Add-Type -TypeDefinition $auditNativeSource -Language CSharp
$matches = [ProjectAtlasWtsProcessAudit]::CountExactSid($Sid)
if (($Expectation -eq "present" -and $matches -lt 1) -or
    ($Expectation -eq "absent" -and $matches -ne 0)) {
    throw "Exact-SID WTS process audit did not satisfy its expected state."
}
'@

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

function Get-ProductionWrapperAst {
    $tokens = $null
    $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        (Get-Item -LiteralPath $ProductionWrapper -Force).FullName,
        [ref]$tokens,
        [ref]$parseErrors
    )
    Require ($parseErrors.Count -eq 0) "Windows construction wrapper did not parse."
    return $ast
}

function Assert-ProductionRecoveryContracts {
    param(
        [Parameter(Mandatory = $true)]
        [System.Management.Automation.Language.ScriptBlockAst]$Ast
    )

    $nativeSources = @($Ast.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                $node.Left.Extent.Text -eq '$nativeSource'
        },
        $true
    ))
    Require ($nativeSources.Count -eq 1) "Expected one construction native adapter source."
    $nativeText = $nativeSources[0].Right.Extent.Text
    $principalLogonIndex = $nativeText.IndexOf(
        'if (!LogonUser(',
        [System.StringComparison]::Ordinal
    )
    $principalTokenValidationIndex = $nativeText.IndexOf(
        'admittedLogonSid = ValidateConstructionToken(',
        [System.StringComparison]::Ordinal
    )
    $processCreationIndex = $nativeText.IndexOf(
        'created = CreateProcessWithToken(',
        [System.StringComparison]::Ordinal
    )
    $logonNamespaceIndex = $nativeText.IndexOf(
        'CaptureTokenNamespaceSnapshot(logonToken)',
        [System.StringComparison]::Ordinal
    )
    $seedCreateIndex = $nativeText.IndexOf(
        'seededSemaphore = CreateSeededSemaphore(',
        [System.StringComparison]::Ordinal
    )
    $creationFlagsIndex = $nativeText.IndexOf(
        'uint flags = GetConstructionCreationFlags();',
        [System.StringComparison]::Ordinal
    )
    $processCreatedIndex = $nativeText.IndexOf(
        'processCreated = true;',
        [System.StringComparison]::Ordinal
    )
    $processTokenOpenIndex = $nativeText.IndexOf(
        'OpenProcessToken(process.Process, TokenQuery, out constructionToken)',
        [System.StringComparison]::Ordinal
    )
    $seedTransferIndex = $nativeText.IndexOf(
        'TransferSeededSemaphore(',
        $processCreatedIndex,
        [System.StringComparison]::Ordinal
    )
    $tokenValidationIndex = $nativeText.IndexOf(
        'string processLogonSid = ValidateConstructionToken(',
        [System.StringComparison]::Ordinal
    )
    $retainedJobInjectionIndex = $nativeText.IndexOf(
        'if (admissionScenario == AdmissionScenario.RetainedJobBeforeAdmission)',
        [System.StringComparison]::Ordinal
    )
    $inheritedJobCheckIndex = $nativeText.IndexOf(
        'IsProcessInJob(process.Process, IntPtr.Zero, out inheritedJob)',
        [System.StringComparison]::Ordinal
    )
    $ownJobAssignmentIndex = $nativeText.IndexOf(
        'AssignProcessToJobObject(job, process.Process)',
        $inheritedJobCheckIndex,
        [System.StringComparison]::Ordinal
    )
    $childBeforeNamespaceIndex = $nativeText.IndexOf(
        'CaptureTokenNamespaceSnapshot(constructionToken)',
        $tokenValidationIndex,
        [System.StringComparison]::Ordinal
    )
    $childAfterNamespaceIndex = $nativeText.IndexOf(
        'CaptureTokenNamespaceSnapshot(constructionToken)',
        $ownJobAssignmentIndex,
        [System.StringComparison]::Ordinal
    )
    $resumeIndex = $nativeText.IndexOf(
        'if (ResumeThread(process.Thread) == UInt32.MaxValue)',
        [System.StringComparison]::Ordinal
    )
    Require `
        ($creationFlagsIndex -ge 0 -and
            $principalLogonIndex -gt $creationFlagsIndex -and
            $principalTokenValidationIndex -gt $principalLogonIndex -and
            $logonNamespaceIndex -gt $principalTokenValidationIndex -and
            $seedCreateIndex -gt $logonNamespaceIndex -and
            $processCreationIndex -gt $seedCreateIndex -and
            $processCreatedIndex -gt $processCreationIndex -and
            $seedTransferIndex -gt $processCreatedIndex -and
            $processTokenOpenIndex -gt $processCreatedIndex -and
            $tokenValidationIndex -gt $processTokenOpenIndex -and
            $childBeforeNamespaceIndex -gt $tokenValidationIndex -and
            $retainedJobInjectionIndex -gt $tokenValidationIndex -and
            $inheritedJobCheckIndex -gt $retainedJobInjectionIndex -and
            $ownJobAssignmentIndex -gt $inheritedJobCheckIndex -and
            $childAfterNamespaceIndex -gt $ownJobAssignmentIndex -and
            $resumeIndex -gt $childAfterNamespaceIndex -and
            $nativeText.Contains('return CreateSuspended | CreateNoWindow | CreateUnicodeEnvironment;') -and
            $nativeText.Contains('EntryPoint = "LogonUserW"') -and
            $nativeText.Contains('EntryPoint = "CreateProcessWithTokenW"') -and
            -not $nativeText.Contains('EntryPoint = "CreateProcessWithLogonW"') -and
            -not $nativeText.Contains('CreateBreakawayFromJob = 0x01000000;') -and
            $nativeText.Contains('ValidateCurrentBrokerJob(brokerJobName);') -and
            $nativeText.Contains('ValidateJobPolicyValues(') -and
            $nativeText.Contains('JobObjectLimitKillOnJobClose | JobObjectLimitBreakawayOk') -and
            $nativeText.Contains('JobObjectBasicUiRestrictions') -and
            $nativeText.Contains('uiRestrictions != 0') -and
            $nativeText.Contains('construction-broker-job-required') -and
            $nativeText.Contains('construction-broker-job-membership') -and
            $nativeText.Contains('construction-broker-job-policy') -and
            $nativeText.Contains('Marshal.ZeroFreeGlobalAllocUnicode(passwordPointer);') -and
            $nativeText.Contains('LogonTokenHandleOwned') -and
            $nativeText.Contains('LogonTokenHandleClosed') -and
            $nativeText.Contains('SeededSemaphoreCreatedNew') -and
            $nativeText.Contains('SeededSemaphoreDuplicated') -and
            $nativeText.Contains('SeededSemaphoreParentHandleClosed') -and
            $nativeText.Contains('TokenNamespaceSnapshot') -and
            $nativeText.Contains('TokenBnoIsolationInformation') -and
            $nativeText.Contains('[MarshalAs(UnmanagedType.U1)]') -and
            $nativeText.Contains('MaximumTokenInformationBytes = 64 * 1024;') -and
            $nativeText -match
                'snapshot\.HasRestrictions\s*=\s*ReadExactTokenBoolean\(' -and
            $nativeText.Contains('if (information.Length != sizeof(byte))') -and
            $nativeText.Contains('byte value = Marshal.ReadByte(information.Pointer);') -and
            $nativeText.Contains('if (value > 1)') -and
            $nativeText -match
                'snapshot\.IsAppContainer\s*=\s*ReadExactTokenDword\(' -and
            $nativeText -match
                'snapshot\.IsSandboxed\s*=\s*ReadExactTokenDword\(' -and
            $nativeText -match
                'snapshot\.IsAppSilo\s*=\s*ReadExactTokenDword\(' -and
            $nativeText.Contains('ambient-construction-jobserver') -and
            $nativeText.Contains('EntryPoint = "CreateSemaphoreExW"') -and
            $nativeText.Contains('string sddl = "D:P(A;;0x00100002;;;"') -and
            $nativeText.Contains('")S:(ML;;NW;;;" + RequiredIntegritySid + ")";') -and
            $nativeText.Contains('DuplicateHandle(') -and
            $nativeText.Contains('DuplicateSameAccess') -and
            $nativeText.Contains('RequireEquivalentTokenNamespaces(') -and
            -not $nativeText.Contains('JobObjectCreateSilo') -and
            -not $nativeText.Contains('CreateRestrictedToken') -and
            -not $nativeText.Contains('SetTokenInformation') -and
            -not $nativeText.Contains('PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES') -and
            -not $nativeText.Contains('JOB_OBJECT_SECURITY_') -and
            $nativeText.Contains('MaximumLogonCommandLineCharacters = 1023;') -and
            $nativeText.Contains('construction-process-retained-inherited-job')) `
        "Construction admission no longer validates the suspended alternate-logon child before assigning its owned Job."

    $objectDirectorySources = @($Ast.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                $node.Left.Extent.Text -eq '$constructionObjectDirectoryAclSource'
        },
        $true
    ))
    Require ($objectDirectorySources.Count -eq 1) "Expected one object-directory ACL adapter source."
    $objectDirectoryText = $objectDirectorySources[0].Right.Extent.Text
    Require `
        ($objectDirectoryText.Contains('DirectoryTraverse = 0x00000002') -and
            $objectDirectoryText.Contains('DirectoryCreateObject = 0x00000004') -and
            $objectDirectoryText.Contains('NamedObjectCreationAccess =') -and
            $objectDirectoryText.Contains('DirectoryTraverse | DirectoryCreateObject') -and
            $objectDirectoryText.Contains('ReadControl = 0x00020000') -and
            $objectDirectoryText.Contains('WriteDac = 0x00040000') -and
            $objectDirectoryText.Contains('NtOpenDirectoryObject(') -and
            $objectDirectoryText.Contains('GetKernelObjectSecurity(') -and
            $objectDirectoryText.Contains('SetKernelObjectSecurity(') -and
            $objectDirectoryText.Contains('Process.GetCurrentProcess().SessionId') -and
            $objectDirectoryText.Contains('sessionId.ToString(CultureInfo.InvariantCulture)') -and
            $objectDirectoryText.Contains('return "\\BaseNamedObjects";') -and
            $objectDirectoryText.Contains('"\\Sessions\\" +') -and
            $objectDirectoryText.Contains('construction-object-directory-target-mismatch') -and
            -not $objectDirectoryText.Contains('construction-object-directory-session-mismatch') -and
            $objectDirectoryText.Contains('construction-object-directory-principal-already-present') -and
            $objectDirectoryText.Contains('common.AceFlags == AceFlags.None') -and
            $objectDirectoryText.Contains('common.AccessMask == checked((int)NamedObjectCreationAccess)') -and
            $objectDirectoryText.Contains('matching != 1 || !exact') -and
            $objectDirectoryText.Contains('StatusObjectNameNotFound') -and
            $objectDirectoryText.Contains('StatusObjectPathNotFound') -and
            $objectDirectoryText.Contains('NtClose(handle)') -and
            $objectDirectoryText.Contains('operation and handle cleanup failed')) `
        "Construction object-directory ACL adapter lost its exact grant, cleanup, or handle contract."
    $wrapperText = $Ast.Extent.Text
    $journalPathIndex = $wrapperText.IndexOf(
        '$state.object_directory = Get-ConstructionObjectDirectoryPath',
        [System.StringComparison]::Ordinal
    )
    $journalWriteIndex = $wrapperText.IndexOf(
        'Write-ProtectedState -State $state',
        $journalPathIndex,
        [System.StringComparison]::Ordinal
    )
    $grantIndex = $wrapperText.IndexOf(
        'Add-ConstructionObjectDirectoryPrincipalAccess',
        $journalWriteIndex,
        [System.StringComparison]::Ordinal
    )
    $grantVerificationIndex = $wrapperText.IndexOf(
        'Assert-ConstructionObjectDirectoryPrincipalAccess',
        $grantIndex,
        [System.StringComparison]::Ordinal
    )
    Require `
        ($journalPathIndex -ge 0 -and
            $journalWriteIndex -gt $journalPathIndex -and
            $grantIndex -gt $journalWriteIndex -and
            $grantVerificationIndex -gt $grantIndex -and
            $wrapperText.Contains('$stateSchemaVersion = 2') -and
            $wrapperText.Contains('$legacyKeys = @("acl_paths", "firewall_rule", "schema_version", "sid", "stage", "username")') -and
            $wrapperText.Contains('"object_directory", "schema_version"')) `
        "Construction object-directory access was not journaled and validated before mutation."

    $cleanupDefinitions = @($Ast.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq 'Invoke-Cleanup'
        },
        $true
    ))
    Require ($cleanupDefinitions.Count -eq 1) "Expected one production cleanup function."
    $cleanupText = $cleanupDefinitions[0].Extent.Text
    $zeroProcessIndex = $cleanupText.IndexOf(
        '$zeroProcesses = @(',
        [System.StringComparison]::Ordinal
    )
    $checkpointIndex = $cleanupText.IndexOf(
        '$null -ne $AfterProcessTermination)',
        [System.StringComparison]::Ordinal
    )
    $processAbsenceIndex = $cleanupText.IndexOf(
        '$state.stage = "processes_absent"',
        [System.StringComparison]::Ordinal
    )
    $accountCheckpointIndex = $cleanupText.IndexOf(
        'if ($accountAbsent -and $null -ne $AfterAccountRemoval)',
        [System.StringComparison]::Ordinal
    )
    $durableCleanupIndex = $cleanupText.IndexOf(
        'foreach ($path in @($state.acl_paths))',
        [System.StringComparison]::Ordinal
    )
    $objectDirectoryCleanupIndex = $cleanupText.IndexOf(
        'Remove-ConstructionObjectDirectoryPrincipalAccess',
        [System.StringComparison]::Ordinal
    )
    Require `
        ($zeroProcessIndex -ge 0 -and
            $processAbsenceIndex -gt $zeroProcessIndex -and
            $checkpointIndex -gt $processAbsenceIndex -and
            $objectDirectoryCleanupIndex -gt $checkpointIndex -and
            $durableCleanupIndex -gt $objectDirectoryCleanupIndex -and
            $accountCheckpointIndex -gt $durableCleanupIndex) `
        "Production cleanup checkpoint moved outside its recovery boundary."

    $dotSourceGuard = @($Ast.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.IfStatementAst] -and
                $node.Clauses.Count -eq 1 -and
                $node.Clauses[0].Item1.Extent.Text -eq
                    "`$MyInvocation.InvocationName -eq '.'"
        },
        $true
    ))
    Require ($dotSourceGuard.Count -eq 1) "Production cleanup is not safely dot-sourceable."

    $proxyCollisions = @($Ast.FindAll(
        {
            param($node)
            ($node -is [System.Management.Automation.Language.VariableExpressionAst] -and
                $node.VariablePath.UserPath -like 'proxy*') -or
                ($node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                    $node.Name -like 'proxy*')
        },
        $true
    ))
    Require `
        ($proxyCollisions.Count -eq 0) `
        "Production wrapper collided with recovery proxy names."

    $topLevelParameters = @($Ast.ParamBlock.Parameters |
        ForEach-Object { $_.Name.VariablePath.UserPath })
    foreach ($forbiddenParameter in @(
        'AfterProcessTermination',
        'AdmissionScenario',
        'Fault',
        'FailureScenario'
    )) {
        Require `
            ($forbiddenParameter -notin $topLevelParameters) `
            "Production wrapper exposed a recovery fault parameter."
    }
    return $nativeSources[0]
}

function Assert-AccountJournalConstructionContract {
    param(
        [Parameter(Mandatory = $true)]
        [System.Management.Automation.Language.ScriptBlockAst]$Ast
    )

    $commands = @($Ast.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.CommandAst] -and
                $node.GetCommandName() -ceq 'New-LocalUser' -and
                $node.CommandElements.Count -gt 0 -and
                $node.CommandElements[0].Extent.Text -ceq 'New-LocalUser'
        },
        $true
    ))
    Require `
        ($commands.Count -eq 1 -and
            $commands[0].Parent -is [System.Management.Automation.Language.PipelineAst] -and
            $commands[0].Parent.Parent -is
                [System.Management.Automation.Language.AssignmentStatementAst]) `
        "Expected one interceptable production account-creation command."

    $accountAssignment = $commands[0].Parent.Parent
    Require `
        ($accountAssignment.Left.Extent.Text -ceq '$account' -and
            $accountAssignment.Parent -is
                [System.Management.Automation.Language.StatementBlockAst]) `
        "Production account creation no longer assigns the retained account."
    $statements = @($accountAssignment.Parent.Statements)
    $accountIndex = [Array]::IndexOf([object[]]$statements, $accountAssignment)
    Require `
        ($accountIndex -ge 0 -and
            $statements.Count -gt ($accountIndex + 5) -and
            $statements[$accountIndex + 1].Extent.Text -ceq '$sid = $account.Sid' -and
            $statements[$accountIndex + 3].Extent.Text -ceq '$state.sid = $sid.Value' -and
            $statements[$accountIndex + 4].Extent.Text -ceq '$state.stage = "filesystem"' -and
            $statements[$accountIndex + 5].Extent.Text -ceq
                'Write-ProtectedState -State $state') `
        "Production account creation no longer precedes initial SID journal publication."
}

function Assert-NamedObjectProbeDiagnosticContract {
    $selfTokens = $null
    $selfParseErrors = $null
    $selfAst = [System.Management.Automation.Language.Parser]::ParseFile(
        $PSCommandPath,
        [ref]$selfTokens,
        [ref]$selfParseErrors
    )
    Require ($selfParseErrors.Count -eq 0) "Windows recovery script did not parse itself."
    $probeAssignments = @($selfAst.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                $node.Left.Extent.Text -eq '$namespaceProbeSource'
        },
        $true
    ))
    Require ($probeAssignments.Count -eq 1) "Expected one named-object probe source."
    $probeText = [string]$probeAssignments[0].Right.Expression.Value
    $probeTokens = $null
    $probeParseErrors = $null
    [void][System.Management.Automation.Language.Parser]::ParseInput(
        $probeText,
        [ref]$probeTokens,
        [ref]$probeParseErrors
    )
    Require ($probeParseErrors.Count -eq 0) "Named-object probe source did not parse."
    foreach ($contract in @(
        "'identity' = 121",
        "'ambient-environment' = 122",
        "'semaphore-acl' = 123",
        "'semaphore-create' = 124",
        "'cargo-makeflags' = 125",
        "'descendant-launch' = 126",
        "'descendant-open' = 127",
        "'result-write' = 128",
        "'cleanup' = 129",
        "'native-semaphore-create' = 130",
        "'native-semaphore-close' = 131",
        'function ConvertTo-BoundedProbeError',
        'function Write-AtomicProbeRecord',
        'public static class ProjectAtlasNamedObjectAccessProbe',
        'SemaphoreSynchronizeAndModify = 0x00100002',
        'NtOpenDirectoryObject(',
        'NtOpenSemaphore(',
        'CreateAndCloseSemaphore(',
        'OpenAndCloseSemaphoreByPath(',
        'OpenAndCloseSemaphore(',
        'OpenOwnedSemaphore(',
        'directory_traverse_ntstatus = $directoryTraverseNtStatus',
        'directory_create_object_ntstatus = $directoryCreateObjectNtStatus',
        'directory_traverse_create_ntstatus = $directoryTraverseCreateNtStatus',
        'session_directory_traverse_ntstatus = $sessionDirectoryTraverseNtStatus',
        'seeded_direct_open_ntstatus = $seededDirectOpenNtStatus',
        'seeded_direct_open_close_ntstatus = $seededDirectOpenCloseNtStatus',
        'construction-token-owner-sid-mismatch',
        'post_job_native_create_win32 = $postJobNativeCreateWin32',
        'post_job_native_created_new = $postJobNativeCreatedNew',
        'post_job_native_close_win32 = $postJobNativeCloseWin32',
        'seeded_semaphore_name = $SeededSemaphoreName',
        'seeded_open_win32 = $seededOpenWin32',
        'seeded_create_win32 = $seededCreateWin32',
        'schema_version = 5',
        '$probeStage = ''cleanup''',
        '$probeStage = ''result-write''',
        '[Console]::Error.WriteLine($fallbackJson)',
        '$child.WaitForExit(15000)',
        '$start.RedirectStandardOutput = $true',
        '[System.Threading.Tasks.Task]::WaitAll($pipeTasks, 5000)',
        "'descendant-native-diagnostic-invalid'",
        '[System.ComponentModel.Win32Exception]::new(',
        "[ValidateSet('none', 'operation-and-cleanup', 'descendant-open-not-found')]",
        "'diagnostic-operation-fault ordinary/token C:/private/forward.txt //server/share/unquoted.txt'",
        "'diagnostic-cleanup-fault ordinary\token D:\private/mixed\cleanup.txt ""//server/share/quoted forward.txt"" \\server\share\backward.txt \/server/share\mixed.txt'",
        'exit [int]$record.exit_code'
    )) {
        Require `
            $probeText.Contains($contract) `
            "Named-object probe lost one stable diagnostic or cleanup contract."
    }
    Require `
        (-not $probeText.Contains("'post-job-native-semaphore-create'") -and
            -not $probeText.Contains("'post-job-native-semaphore-close'") -and
            -not $probeText.Contains('$security.SetOwner(')) `
        "Default-security semaphore diagnostics must not gate the explicit-security access proof."
    Require `
        ($probeText -notmatch '(?m)^\s*exit\s+1\s*$') `
        "Named-object probe retained an unclassified exit path."
    $canaryAssignments = @($selfAst.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                $node.Left.Extent.Text -eq '$namespaceCanarySource'
        },
        $true
    ))
    Require ($canaryAssignments.Count -eq 1) "Expected one named-object canary source."
    $canaryText = [string]$canaryAssignments[0].Right.Expression.Value
    $canaryTokens = $null
    $canaryParseErrors = $null
    [void][System.Management.Automation.Language.Parser]::ParseInput(
        $canaryText,
        [ref]$canaryTokens,
        [ref]$canaryParseErrors
    )
    Require `
        ($canaryParseErrors.Count -eq 0 -and
            $canaryText -notmatch '(?m)^\s*exit\s+1\s*$' -and
            $canaryText.Contains('exit 141') -and
            $canaryText.Contains('exit 142') -and
            $canaryText.Contains('exit 143') -and
            $canaryText.Contains('exit 144') -and
            $canaryText.Contains('[Console]::Out.WriteLine("native_code=$canaryNativeCode")')) `
        "Named-object descendant canary retained an unclassified exit path."
    $scenarioDefinitions = @($selfAst.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq 'Invoke-ConstructionAdmissionRecoveryScenario'
        },
        $true
    ))
    Require ($scenarioDefinitions.Count -eq 1) "Expected one construction recovery scenario."
    $scenarioText = $scenarioDefinitions[0].Extent.Text
    Require `
        ($scenarioText.Contains('Read-NamedObjectProbeRecord') -and
            $scenarioText.Contains('Format-NamedObjectProbeFailure') -and
            $scenarioText.Contains('Format-NamedObjectAccessComparison') -and
            $scenarioText.Contains('[named-object-access]') -and
            $scenarioText.Contains('-ComparisonSemaphoreName') -and
            $scenarioText.Contains('stage=diagnostic-unavailable') -and
            $scenarioText.Contains('stage=diagnostic-invalid') -and
            $scenarioText -notmatch
                '\[(?:bool|int|string)\]\$probeResult(?!\.)') `
        "Construction recovery no longer surfaces one bounded child diagnostic."
    $temporaryCleanupDefinitions = @($selfAst.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq 'Remove-NamedObjectProbeTemporaryRecords'
        },
        $true
    ))
    Require `
        ($temporaryCleanupDefinitions.Count -eq 1) `
        "Expected one named-object temporary cleanup function."
    $temporaryCleanupCommands = @($temporaryCleanupDefinitions[0].FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.CommandAst] -and
                $node.GetCommandName() -eq 'Get-ChildItem'
        },
        $true
    ))
    Require `
        ($temporaryCleanupCommands.Count -eq 2 -and
            @($temporaryCleanupCommands | Where-Object {
                $_.Extent.Text.Contains('-Filter $temporaryNameFilter')
            }).Count -eq 2 -and
            -not $temporaryCleanupDefinitions[0].Extent.Text.Contains(
                'Get-ChildItem -LiteralPath $resolvedExpectedParent -Force -File'
            )) `
        "Named-object temporary cleanup lost its provider-bounded exact-prefix audit."
    $selfText = $selfAst.Extent.Text
    Require `
        ($selfText.Contains('function Test-ExactJsonInteger') -and
            $selfText.Contains('$Value.GetType() -eq [long]') -and
            $selfText.Contains('$Value.GetType() -eq [string]') -and
            $selfText.Contains('$Value.GetType() -eq [bool]') -and
            $selfText.Contains('function Test-BoundedProbeErrorsEqual') -and
            $selfText.Contains('function Test-DefaultSecuritySemaphoreProbe') -and
            $selfText.Contains('$isCleanupFailure =') -and
            $selfText.Contains('function Format-NamedObjectProbeFailure') -and
            $selfText.Contains('function Format-NamedObjectAccessComparison') -and
            $selfText.Contains('function Assert-NamedObjectProbeRecordFixtures') -and
            $selfText.Contains('post_job_native_create_win32') -and
            $selfText.Contains('seeded_name_matches=') -and
            $selfText.Contains('operation_error_type=$operationType') -and
            $selfText.Contains('cleanup_error_type=$cleanupType') -and
            $selfText.Contains('function Remove-NamedObjectProbeTemporaryRecords') -and
            $selfText.Contains('$namespaceProbeResultPaths')) `
        "Named-object probe reader, combined failure, or temporary cleanup contract changed."
}

$productionAst = Get-ProductionWrapperAst
$nativeSourceAssignment = Assert-ProductionRecoveryContracts -Ast $productionAst
Assert-AccountJournalConstructionContract -Ast $productionAst
Assert-NamedObjectProbeDiagnosticContract
$auditTokens = $null
$auditParseErrors = $null
[void][System.Management.Automation.Language.Parser]::ParseInput(
    $exactSidAuditSource,
    [ref]$auditTokens,
    [ref]$auditParseErrors
)
Require `
    ($auditParseErrors.Count -eq 0) `
    "Exact-SID WTS audit helper did not parse."
if ($StaticOnly) {
    if (-not [string]::IsNullOrWhiteSpace($BrokerJobName)) {
        if ($env:OS -ne "Windows_NT" -or -not [Environment]::Is64BitProcess) {
            throw "Windows broker admission validation requires 64-bit Windows."
        }
        Invoke-Expression $nativeSourceAssignment.Extent.Text
        if (-not ('ProjectAtlasConstructionProcess' -as [type])) {
            Add-Type -TypeDefinition $nativeSource -Language CSharp
        }
        [ProjectAtlasConstructionProcess]::ConfigureBrokerJob($BrokerJobName)
        Write-Output "Windows parser-pack construction broker admission passed."
    }
    Write-Output "Windows parser-pack recovery static validation passed."
    return
}

if ($env:OS -ne "Windows_NT" -or -not [Environment]::Is64BitProcess) {
    throw "Windows parser-pack recovery requires 64-bit Windows."
}
$principal = [System.Security.Principal.WindowsPrincipal]::new(
    [System.Security.Principal.WindowsIdentity]::GetCurrent()
)
if (-not $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Windows parser-pack recovery requires an elevated runner."
}
if ($null -eq $ConstructionParameters) {
    throw "Windows parser-pack recovery requires the production construction parameters."
}
if ([string]::IsNullOrWhiteSpace($BrokerJobName)) {
    throw "Windows parser-pack recovery requires the shared broker Job name."
}
if ([string]::IsNullOrWhiteSpace($RunnerTemporaryRoot)) {
    throw "Windows parser-pack recovery requires the trusted runner temporary root."
}
foreach ($requiredParameter in @(
    'Mode', 'StatePath', 'SourceRoot', 'InputDirectory', 'VendorDirectory',
    'OutputDirectory', 'CargoHome', 'TemporaryDirectory', 'HomeDirectory',
    'ToolchainRoot', 'PwshPath', 'VcToolsRoot', 'WindowsSdkRoot',
    'ProjectAtlasRevision', 'CargoPackageVersion', 'IntendedReleaseVersion',
    'CargoLockSha256', 'RustcRelease', 'RustcCommitHash', 'ResolverAddress'
)) {
    if (-not $ConstructionParameters.ContainsKey($requiredParameter) -or
        [string]::IsNullOrWhiteSpace([string]$ConstructionParameters[$requiredParameter])) {
        throw "Windows parser-pack recovery is missing one production construction parameter."
    }
}
Require `
    ([string]$ConstructionParameters.Mode -eq 'construct') `
    "Windows parser-pack recovery received a non-construction parameter set."
Require `
    ($ConstructionParameters.ContainsKey('BrokerJobName') -and
        [string]$ConstructionParameters.BrokerJobName -ceq $BrokerJobName) `
    "Windows parser-pack recovery did not receive one exact shared broker Job."

$ProductionWrapper = (Get-Item -LiteralPath $ProductionWrapper -Force).FullName
$LauncherPath = (Get-Item -LiteralPath $LauncherPath -Force).FullName
$RecoveryRoot = [System.IO.Path]::GetFullPath($RecoveryRoot)
$runnerTempItem = Get-Item -LiteralPath (
    [System.IO.Path]::GetFullPath($RunnerTemporaryRoot)
) -Force
Require `
    ($runnerTempItem.PSIsContainer -and
        (($runnerTempItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0)) `
    "Windows parser-pack recovery requires one regular runner temporary root."
$runnerTemp = $runnerTempItem.FullName.TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
)
Require `
    ($RecoveryRoot.StartsWith(
        "$runnerTemp$([System.IO.Path]::DirectorySeparatorChar)",
        [System.StringComparison]::OrdinalIgnoreCase
    )) `
    "Windows parser-pack recovery root escaped RUNNER_TEMP."
[System.IO.Directory]::CreateDirectory($RecoveryRoot) | Out-Null
$exactSidAuditOwnerSid =
    [System.Security.Principal.WindowsIdentity]::GetCurrent().User
$exactSidAuditDirectory = [System.IO.Path]::Combine(
    $RecoveryRoot,
    'exact-sid-audit-helper'
)
[System.IO.Directory]::CreateDirectory($exactSidAuditDirectory) | Out-Null
$auditDirectorySecurity = [System.Security.AccessControl.DirectorySecurity]::new()
$auditDirectorySecurity.SetOwner($exactSidAuditOwnerSid)
$auditDirectorySecurity.SetAccessRuleProtection($true, $false)
foreach ($allowedSid in @(
    $exactSidAuditOwnerSid,
    [System.Security.Principal.SecurityIdentifier]::new('S-1-5-18'),
    [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
)) {
    $auditDirectorySecurity.AddAccessRule(
        [System.Security.AccessControl.FileSystemAccessRule]::new(
            $allowedSid,
            [System.Security.AccessControl.FileSystemRights]::FullControl,
            [System.Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit',
            [System.Security.AccessControl.PropagationFlags]::None,
            [System.Security.AccessControl.AccessControlType]::Allow
        )
    )
}
Set-Acl -LiteralPath $exactSidAuditDirectory -AclObject $auditDirectorySecurity
$exactSidAuditPath = [System.IO.Path]::Combine(
    $exactSidAuditDirectory,
    'audit-exact-sid-processes.ps1'
)
[System.IO.File]::WriteAllText(
    $exactSidAuditPath,
    $exactSidAuditSource,
    [System.Text.UTF8Encoding]::new($false)
)
$auditFileSecurity = [System.Security.AccessControl.FileSecurity]::new()
$auditFileSecurity.SetOwner($exactSidAuditOwnerSid)
$auditFileSecurity.SetAccessRuleProtection($true, $false)
foreach ($allowedSid in @(
    $exactSidAuditOwnerSid,
    [System.Security.Principal.SecurityIdentifier]::new('S-1-5-18'),
    [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
)) {
    $auditFileSecurity.AddAccessRule(
        [System.Security.AccessControl.FileSystemAccessRule]::new(
            $allowedSid,
            [System.Security.AccessControl.FileSystemRights]::FullControl,
            [System.Security.AccessControl.AccessControlType]::Allow
        )
    )
}
Set-Acl -LiteralPath $exactSidAuditPath -AclObject $auditFileSecurity
$exactSidAuditSha256 = (Get-FileHash `
    -LiteralPath $exactSidAuditPath `
    -Algorithm SHA256).Hash

$scenarioStatePaths = [ordered]@{
    LegacyJournal = [System.IO.Path]::Combine(
        $RecoveryRoot,
        'legacy-journal',
        'parser-pack-windows-construction-state',
        'state.json'
    )
    AccountJournal = [System.IO.Path]::Combine(
        $RecoveryRoot,
        'account-journal',
        'parser-pack-windows-construction-state',
        'state.json'
    )
    LauncherAdmission = [System.IO.Path]::Combine(
        $RecoveryRoot,
        'launcher-admission',
        'parser-pack-windows-construction-state',
        'state.json'
    )
    CleanupRetry = [System.IO.Path]::Combine(
        $RecoveryRoot,
        'cleanup-retry',
        'parser-pack-windows-construction-state',
        'state.json'
    )
}
$recoveryPasswords = [System.Collections.Generic.List[System.Security.SecureString]]::new()
$namespaceProbePaths = [System.Collections.Generic.List[string]]::new()
$namespaceProbeResultPaths = [System.Collections.Generic.List[string]]::new()

function Invoke-BoundedProcess {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,

        [Parameter(Mandatory = $true)]
        [ValidateRange(1, 600)]
        [int]$TimeoutSeconds
    )

    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $FilePath
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $start.ArgumentList.Add($argument)
    }
    $process = $null
    $processId = 0
    $exitCode = $null
    $standardOutput = ''
    $standardError = ''
    $operationFailure = $null
    $cleanupFailures = [System.Collections.Generic.List[System.Exception]]::new()
    try {
        $process = [System.Diagnostics.Process]::Start($start)
        if ($null -eq $process) {
            throw "Could not start one bounded recovery process."
        }
        $processId = $process.Id
        $standardOutputTask = $process.StandardOutput.ReadToEndAsync()
        $standardErrorTask = $process.StandardError.ReadToEndAsync()
        $timedOut = -not $process.WaitForExit($TimeoutSeconds * 1000)
        if ($timedOut) {
            $process.Kill($true)
            if (-not $process.WaitForExit(5000)) {
                throw "Timed-out recovery process could not be reaped."
            }
        }
        $standardOutput = $standardOutputTask.GetAwaiter().GetResult()
        $standardError = $standardErrorTask.GetAwaiter().GetResult()
        $maximumDiagnosticCharacters = 4096
        if ($standardOutput.Length -gt $maximumDiagnosticCharacters -or
            $standardError.Length -gt $maximumDiagnosticCharacters) {
            throw "Bounded recovery process exceeded its fixed diagnostic output limit."
        }
        $standardOutput = ($standardOutput -replace '[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]+', ' ').Trim()
        $standardError = ($standardError -replace '[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]+', ' ').Trim()
        if ($timedOut) {
            throw "Recovery process exceeded its fixed deadline. stdout=$standardOutput stderr=$standardError"
        }
        if ($null -ne (Get-Process -Id $processId -ErrorAction SilentlyContinue)) {
            throw "Bounded recovery process PID survived completion."
        }
        $exitCode = $process.ExitCode
        if ($exitCode -ne 0) {
            throw "Bounded recovery process failed. exit=$exitCode stdout=$standardOutput stderr=$standardError"
        }
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
                        throw "Fallback bounded recovery process termination could not be reaped."
                    }
                }
                if ($processId -gt 0 -and
                    $null -ne (Get-Process -Id $processId -ErrorAction SilentlyContinue)) {
                    throw "Fallback bounded recovery process PID survived."
                }
            }
            catch {
                $cleanupFailures.Add($_.Exception)
            }
            try {
                $process.Dispose()
            }
            catch {
                $cleanupFailures.Add($_.Exception)
            }
        }
    }
    if ($null -ne $operationFailure) {
        if ($cleanupFailures.Count -ne 0) {
            throw [System.AggregateException]::new(
                "Bounded recovery operation and cleanup failed.",
                @($operationFailure) + @($cleanupFailures)
            )
        }
        throw $operationFailure
    }
    if ($cleanupFailures.Count -ne 0) {
        throw [System.AggregateException]::new(
            "Bounded recovery cleanup failed.",
            @($cleanupFailures)
        )
    }
    return $exitCode
}

function Invoke-ExactSidProcessAudit {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Sid,

        [Parameter(Mandatory = $true)]
        [ValidateSet('present', 'absent')]
        [string]$Expectation,

        [ValidateRange(0, 60000)]
        [int]$DelayMilliseconds = 0,

        [ValidateRange(1, 60)]
        [int]$TimeoutSeconds = 10
    )

    $auditDirectoryItem = Get-Item -LiteralPath $exactSidAuditDirectory -Force
    $auditItem = Get-Item -LiteralPath $exactSidAuditPath -Force
    $auditDirectoryAcl = Get-Acl -LiteralPath $auditDirectoryItem.FullName
    $auditAcl = Get-Acl -LiteralPath $auditItem.FullName
    Require `
        ($auditDirectoryItem.PSIsContainer -and
            (($auditDirectoryItem.Attributes -band
                [System.IO.FileAttributes]::ReparsePoint) -eq 0) -and
            -not $auditItem.PSIsContainer -and
            (($auditItem.Attributes -band
                [System.IO.FileAttributes]::ReparsePoint) -eq 0) -and
            $auditItem.FullName -eq $exactSidAuditPath -and
            $auditItem.Length -gt 0 -and
            $auditItem.Length -le 32768 -and
            $auditDirectoryAcl.AreAccessRulesProtected -and
            $auditAcl.AreAccessRulesProtected -and
            $auditDirectoryAcl.GetOwner(
                [System.Security.Principal.SecurityIdentifier]
            ).Value -eq $exactSidAuditOwnerSid.Value -and
            $auditAcl.GetOwner(
                [System.Security.Principal.SecurityIdentifier]
            ).Value -eq $exactSidAuditOwnerSid.Value -and
            (Get-FileHash `
                -LiteralPath $auditItem.FullName `
                -Algorithm SHA256).Hash -eq $exactSidAuditSha256) `
        "Exact-SID WTS audit helper changed before launch."

    $exitCode = Invoke-BoundedProcess `
        -FilePath ([string]$ConstructionParameters.PwshPath) `
        -Arguments @(
            '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
            '-File', $exactSidAuditPath,
            '-Sid', $Sid,
            '-Expectation', $Expectation,
            '-DelayMilliseconds', $DelayMilliseconds
        ) `
        -TimeoutSeconds $TimeoutSeconds
    Require `
        ($exitCode -eq 0) `
        "Exact-SID WTS process audit failed."
}

function New-RecoveryIdentity {
    $identityBytes = [byte[]]::new(16)
    [System.Security.Cryptography.RandomNumberGenerator]::Fill($identityBytes)
    $randomHex = [Convert]::ToHexString($identityBytes).ToLowerInvariant()
    [Array]::Clear($identityBytes, 0, $identityBytes.Length)

    $passwordBytes = [byte[]]::new(32)
    [System.Security.Cryptography.RandomNumberGenerator]::Fill($passwordBytes)
    $alphabet = [char[]]'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*_-+='
    $passwordCharacters = [char[]]::new(36)
    $passwordCharacters[0] = 'A'
    $passwordCharacters[1] = '!'
    $passwordCharacters[2] = '9'
    $passwordCharacters[3] = 'z'
    for ($index = 0; $index -lt $passwordBytes.Length; $index++) {
        $passwordCharacters[$index + 4] = $alphabet[$passwordBytes[$index] % $alphabet.Length]
    }
    $password = [System.Security.SecureString]::new()
    foreach ($character in $passwordCharacters) {
        $password.AppendChar($character)
    }
    $password.MakeReadOnly()
    [Array]::Clear($passwordCharacters, 0, $passwordCharacters.Length)
    [Array]::Clear($passwordBytes, 0, $passwordBytes.Length)
    return [pscustomobject]@{
        Username = "pa$($randomHex.Substring(0, 12))"
        FirewallRule = "ProjectAtlas-ParserPack-Construction-$($randomHex.Substring(0, 12))"
        Password = $password
        Sid = $null
    }
}

function Invoke-WithCleanupDefinitions {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath,

        [Parameter(Mandatory = $true)]
        [scriptblock]$Operation
    )

    & {
        param(
            [string]$Wrapper,
            [string]$ScenarioStatePath,
            [scriptblock]$ScenarioOperation
        )
        . $Wrapper -Mode cleanup -StatePath $ScenarioStatePath
        & $ScenarioOperation
    } $ProductionWrapper $StatePath $Operation
}

function Write-ScenarioState {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath,

        [Parameter(Mandatory = $true)]
        [hashtable]$State
    )

    Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
        Write-ProtectedState -State $State
    }
}

function Read-ScenarioState {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath
    )

    return Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
        return Read-CleanupState
    }
}

function Test-ExactJsonInteger {
    param($Value)
    return $null -ne $Value -and $Value.GetType() -eq [long]
}

function Test-ExactJsonString {
    param($Value)
    return $null -ne $Value -and $Value.GetType() -eq [string]
}

function Test-ExactJsonBoolean {
    param($Value)
    return $null -ne $Value -and $Value.GetType() -eq [bool]
}

function Test-BoundedProbeError {
    param($Value)

    if ($null -eq $Value -or
        $Value.GetType() -ne [System.Management.Automation.PSCustomObject]) {
        return $false
    }
    $keys = @($Value.PSObject.Properties.Name | Sort-Object)
    if (Compare-Object `
        -ReferenceObject @('message', 'native_code', 'type') `
        -DifferenceObject $keys) {
        return $false
    }
    if (-not (Test-ExactJsonString $Value.type) -or
        $Value.type -notmatch '\A[A-Za-z0-9_.]{1,96}\z' -or
        -not (Test-ExactJsonString $Value.message) -or
        $Value.message -notmatch '\A[^\x00-\x1F\x7F]{1,384}\z') {
        return $false
    }
    return $null -eq $Value.native_code -or
        ((Test-ExactJsonInteger $Value.native_code) -and
            $Value.native_code -ge [int32]::MinValue -and
            $Value.native_code -le [int32]::MaxValue)
}

function Test-BoundedProbeErrorsEqual {
    param(
        $Left,
        $Right
    )

    return (Test-BoundedProbeError $Left) -and
        (Test-BoundedProbeError $Right) -and
        $Left.type -ceq $Right.type -and
        $Left.message -ceq $Right.message -and
        (($null -eq $Left.native_code -and $null -eq $Right.native_code) -or
            ($null -ne $Left.native_code -and
                $null -ne $Right.native_code -and
                $Left.native_code -eq $Right.native_code))
}

function Test-DefaultSecuritySemaphoreProbe {
    param(
        [Parameter(Mandatory = $true)]
        [long]$CreateWin32,

        [Parameter(Mandatory = $true)]
        [bool]$CreatedNew,

        [Parameter(Mandatory = $true)]
        [long]$CloseWin32
    )

    return $CreateWin32 -eq 0L -and $CreatedNew -and $CloseWin32 -eq 0L
}

function Read-NamedObjectProbeRecord {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    Require `
        ([System.IO.File]::Exists($Path) -and
            -not [System.IO.Directory]::Exists($Path)) `
        "Named-object probe diagnostic record is missing."
    $item = Get-Item -LiteralPath $Path -Force
    Require `
        (-not $item.PSIsContainer -and
            (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0) -and
            $item.Length -ge 1 -and
            $item.Length -le 4096) `
        "Named-object probe diagnostic record is unsafe or unbounded."
    $record = [System.IO.File]::ReadAllText($item.FullName) |
        ConvertFrom-Json -Depth 8
    $expectedKeys = @(
        'cleanup_error', 'created_new', 'descendant_exit_code', 'directory_path',
        'directory_create_object_ntstatus', 'directory_traverse_create_ntstatus',
        'directory_traverse_ntstatus', 'error', 'exit_code', 'native_semaphore_name',
        'operation_error', 'operation_stage', 'post_job_native_close_win32',
        'post_job_native_create_win32', 'post_job_native_created_new', 'schema_version',
        'seeded_create_close_win32', 'seeded_create_created_new',
        'seeded_direct_open_close_ntstatus', 'seeded_direct_open_ntstatus',
        'seeded_create_win32', 'seeded_open_close_win32', 'seeded_open_win32',
        'seeded_semaphore_name', 'semaphore_name', 'session_directory_traverse_ntstatus',
        'session_id', 'stage', 'status'
    ) | Sort-Object
    $actualKeys = @($record.PSObject.Properties.Name | Sort-Object)
    Require `
        (-not (Compare-Object -ReferenceObject $expectedKeys -DifferenceObject $actualKeys)) `
        "Named-object probe diagnostic schema changed."
    $operationStageExitCodes = @{
        'identity' = 121
        'ambient-environment' = 122
        'semaphore-acl' = 123
        'semaphore-create' = 124
        'cargo-makeflags' = 125
        'descendant-launch' = 126
        'descendant-open' = 127
        'native-semaphore-create' = 130
        'native-semaphore-close' = 131
    }
    Require `
        ((Test-ExactJsonInteger $record.schema_version) -and
            $record.schema_version -eq 5L -and
            (Test-ExactJsonString $record.status) -and
            (Test-ExactJsonString $record.stage) -and
            (Test-ExactJsonInteger $record.exit_code) -and
            (Test-ExactJsonInteger $record.session_id) -and
            (Test-ExactJsonString $record.directory_path) -and
            (Test-ExactJsonInteger $record.directory_traverse_ntstatus) -and
            (Test-ExactJsonInteger $record.directory_create_object_ntstatus) -and
            (Test-ExactJsonInteger $record.directory_traverse_create_ntstatus) -and
            (Test-ExactJsonInteger $record.session_directory_traverse_ntstatus) -and
            (Test-ExactJsonString $record.native_semaphore_name) -and
            (Test-ExactJsonInteger $record.post_job_native_create_win32) -and
            (Test-ExactJsonBoolean $record.post_job_native_created_new) -and
            (Test-ExactJsonInteger $record.post_job_native_close_win32) -and
            (Test-ExactJsonString $record.seeded_semaphore_name) -and
            (Test-ExactJsonInteger $record.seeded_direct_open_ntstatus) -and
            (Test-ExactJsonInteger $record.seeded_direct_open_close_ntstatus) -and
            (Test-ExactJsonInteger $record.seeded_open_win32) -and
            (Test-ExactJsonInteger $record.seeded_open_close_win32) -and
            (Test-ExactJsonInteger $record.seeded_create_win32) -and
            (Test-ExactJsonBoolean $record.seeded_create_created_new) -and
            (Test-ExactJsonInteger $record.seeded_create_close_win32) -and
            (Test-ExactJsonString $record.semaphore_name) -and
            (Test-ExactJsonBoolean $record.created_new) -and
            (Test-ExactJsonInteger $record.descendant_exit_code) -and
            ($null -eq $record.operation_stage -or
                (Test-ExactJsonString $record.operation_stage)) -and
            $record.session_id -ge -1L -and
            $record.session_id -le [int32]::MaxValue -and
            $record.descendant_exit_code -ge -1L -and
            $record.descendant_exit_code -le [int32]::MaxValue -and
            $record.directory_traverse_ntstatus -ge [int32]::MinValue -and
            $record.directory_traverse_ntstatus -le [int32]::MaxValue -and
            $record.directory_create_object_ntstatus -ge [int32]::MinValue -and
            $record.directory_create_object_ntstatus -le [int32]::MaxValue -and
            $record.directory_traverse_create_ntstatus -ge [int32]::MinValue -and
            $record.directory_traverse_create_ntstatus -le [int32]::MaxValue -and
            $record.session_directory_traverse_ntstatus -ge [int32]::MinValue -and
            $record.session_directory_traverse_ntstatus -le [int32]::MaxValue -and
            $record.post_job_native_create_win32 -ge -1L -and
            $record.post_job_native_create_win32 -le [int32]::MaxValue -and
            $record.post_job_native_close_win32 -ge -1L -and
            $record.post_job_native_close_win32 -le [int32]::MaxValue -and
            $record.seeded_open_win32 -ge -1L -and
            $record.seeded_open_win32 -le [int32]::MaxValue -and
            $record.seeded_open_close_win32 -ge -1L -and
            $record.seeded_open_close_win32 -le [int32]::MaxValue -and
            $record.seeded_direct_open_ntstatus -ge [int32]::MinValue -and
            $record.seeded_direct_open_ntstatus -le [int32]::MaxValue -and
            $record.seeded_direct_open_close_ntstatus -ge [int32]::MinValue -and
            $record.seeded_direct_open_close_ntstatus -le [int32]::MaxValue -and
            $record.seeded_create_win32 -ge -1L -and
            $record.seeded_create_win32 -le [int32]::MaxValue -and
            $record.seeded_create_close_win32 -ge -1L -and
            $record.seeded_create_close_win32 -le [int32]::MaxValue -and
            ([string]::IsNullOrEmpty($record.directory_path) -or
                ($record.session_id -eq 0L -and
                    $record.directory_path -ceq '\BaseNamedObjects') -or
                ($record.session_id -gt 0L -and
                    $record.directory_path -ceq
                        "\Sessions\$($record.session_id)\BaseNamedObjects")) -and
            ([string]::IsNullOrEmpty($record.semaphore_name) -or
                $record.semaphore_name -match
                    '\AProjectAtlasParserPack-[0-9a-f]{32}\z') -and
            ([string]::IsNullOrEmpty($record.native_semaphore_name) -or
                $record.native_semaphore_name -match
                    '\AProjectAtlasParserPack-[0-9a-f]{32}\z') -and
            ([string]::IsNullOrEmpty($record.seeded_semaphore_name) -or
                $record.seeded_semaphore_name -match
                    '\AProjectAtlasParserPack-[0-9a-f]{32}\z')) `
        "Named-object probe diagnostic field types or values were invalid."

    $isSuccess = $record.status -ceq 'success' -and
        $record.stage -ceq 'complete' -and
        $record.exit_code -eq 0L -and
        $null -eq $record.error -and
        $null -eq $record.operation_stage -and
        $null -eq $record.operation_error -and
        $null -eq $record.cleanup_error -and
        $record.session_id -ge 0L -and
        -not [string]::IsNullOrEmpty($record.directory_path) -and
        -not [string]::IsNullOrEmpty($record.native_semaphore_name) -and
        $record.directory_traverse_ntstatus -eq 0L -and
        $record.directory_create_object_ntstatus -eq 0L -and
        $record.directory_traverse_create_ntstatus -eq 0L -and
        $record.session_directory_traverse_ntstatus -eq 0L -and
        $record.seeded_direct_open_ntstatus -eq 0L -and
        $record.seeded_direct_open_close_ntstatus -eq 0L -and
        (Test-DefaultSecuritySemaphoreProbe `
            -CreateWin32 $record.post_job_native_create_win32 `
            -CreatedNew $record.post_job_native_created_new `
            -CloseWin32 $record.post_job_native_close_win32) -and
        $record.seeded_open_win32 -eq 0L -and
        $record.seeded_open_close_win32 -eq 0L -and
        $record.seeded_create_win32 -eq 183L -and
        $record.seeded_create_created_new -eq $false -and
        $record.seeded_create_close_win32 -eq 0L -and
        -not [string]::IsNullOrEmpty($record.semaphore_name) -and
        $record.created_new -eq $false -and
        $record.descendant_exit_code -eq 0L

    $isOperationFailure = $record.status -ceq 'failure' -and
        $operationStageExitCodes.ContainsKey($record.stage) -and
        $record.exit_code -eq [long]$operationStageExitCodes[$record.stage] -and
        $record.operation_stage -ceq $record.stage -and
        (Test-BoundedProbeErrorsEqual $record.error $record.operation_error) -and
        $null -eq $record.cleanup_error

    $operationPairAbsent = $null -eq $record.operation_stage -and
        $null -eq $record.operation_error
    $operationPairPresent = (Test-ExactJsonString $record.operation_stage) -and
        $operationStageExitCodes.ContainsKey($record.operation_stage) -and
        (Test-BoundedProbeError $record.operation_error)
    $isCleanupFailure = $record.status -ceq 'failure' -and
        $record.stage -ceq 'cleanup' -and
        $record.exit_code -eq 129L -and
        (Test-BoundedProbeErrorsEqual $record.error $record.cleanup_error) -and
        ($operationPairAbsent -or $operationPairPresent)

    Require `
        (($isSuccess -or $isOperationFailure -or $isCleanupFailure) -and
            @($isSuccess, $isOperationFailure, $isCleanupFailure |
                Where-Object { $_ }).Count -eq 1) `
        "Named-object probe diagnostic stage, exit, or error relationship was invalid."
    return $record
}

function Format-NamedObjectProbeFailure {
    param(
        [Parameter(Mandatory = $true)]
        [pscustomobject]$Record,

        [Parameter(Mandatory = $true)]
        [int]$ProcessExitCode
    )

    $nativeCode = if ($null -eq $Record.error.native_code) {
        ''
    }
    else {
        [string]$Record.error.native_code
    }
    $operationStage = if ($null -eq $Record.operation_stage) {
        ''
    }
    else {
        $Record.operation_stage
    }
    $operationType = if ($null -eq $Record.operation_error) { '' } else { $Record.operation_error.type }
    $operationNative = if ($null -eq $Record.operation_error -or
        $null -eq $Record.operation_error.native_code) { '' } else { [string]$Record.operation_error.native_code }
    $operationMessage = if ($null -eq $Record.operation_error) { '' } else { $Record.operation_error.message }
    $cleanupType = if ($null -eq $Record.cleanup_error) { '' } else { $Record.cleanup_error.type }
    $cleanupNative = if ($null -eq $Record.cleanup_error -or
        $null -eq $Record.cleanup_error.native_code) { '' } else { [string]$Record.cleanup_error.native_code }
    $cleanupMessage = if ($null -eq $Record.cleanup_error) { '' } else { $Record.cleanup_error.message }
    return "Construction named-object probe child failed. exit_code=$ProcessExitCode stage=$($Record.stage) error_type=$($Record.error.type) native_error_code=$nativeCode message=$($Record.error.message) operation_stage=$operationStage operation_error_type=$operationType operation_native_error_code=$operationNative operation_message=$operationMessage cleanup_error_type=$cleanupType cleanup_native_error_code=$cleanupNative cleanup_message=$cleanupMessage"
}

function Assert-NamedObjectProbeRecordFixtures {
    $fixtureRoot = [System.IO.Path]::Combine(
        [System.IO.Path]::GetTempPath(),
        "projectatlas-named-object-record-$([Guid]::NewGuid().ToString('N'))"
    )
    [System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
    try {
        $nativeName = 'ProjectAtlasParserPack-00000000000000000000000000000001'
        $managedName = 'ProjectAtlasParserPack-00000000000000000000000000000002'
        $success = [ordered]@{
            schema_version = 5
            status = 'success'
            stage = 'complete'
            exit_code = 0
            error = $null
            operation_stage = $null
            operation_error = $null
            cleanup_error = $null
            session_id = 1
            directory_path = '\Sessions\1\BaseNamedObjects'
            directory_traverse_ntstatus = 0
            directory_create_object_ntstatus = 0
            directory_traverse_create_ntstatus = 0
            session_directory_traverse_ntstatus = 0
            native_semaphore_name = $nativeName
            post_job_native_create_win32 = 0
            post_job_native_created_new = $true
            post_job_native_close_win32 = 0
            seeded_semaphore_name = $managedName
            seeded_direct_open_ntstatus = 0
            seeded_direct_open_close_ntstatus = 0
            seeded_open_win32 = 0
            seeded_open_close_win32 = 0
            seeded_create_win32 = 183
            seeded_create_created_new = $false
            seeded_create_close_win32 = 0
            semaphore_name = $managedName
            created_new = $false
            descendant_exit_code = 0
        }
        $records = [System.Collections.Generic.List[System.Collections.IDictionary]]::new()
        $records.Add($success)
        $defaultSecurityDenied = [ordered]@{}
        foreach ($entry in $success.GetEnumerator()) {
            $defaultSecurityDenied[$entry.Key] = $entry.Value
        }
        $defaultSecurityDenied.post_job_native_create_win32 = 5
        $defaultSecurityDenied.post_job_native_created_new = $false
        $defaultSecurityDenied.post_job_native_close_win32 = -1
        foreach ($failureRow in @(
            [pscustomobject]@{
                Stage = 'native-semaphore-create'
                ExitCode = 130
                NativeCode = 5
                PostCreate = 5
                PostCreated = $false
                PostClose = -1
                ManagedName = ''
            },
            [pscustomobject]@{
                Stage = 'native-semaphore-close'
                ExitCode = 131
                NativeCode = 6
                PostCreate = 0
                PostCreated = $true
                PostClose = 6
                ManagedName = ''
            },
            [pscustomobject]@{
                Stage = 'semaphore-create'
                ExitCode = 124
                NativeCode = 5
                PostCreate = 0
                PostCreated = $true
                PostClose = 0
                ManagedName = $managedName
            }
        )) {
            $record = [ordered]@{}
            foreach ($entry in $success.GetEnumerator()) {
                $record[$entry.Key] = $entry.Value
            }
            $error = [ordered]@{
                type = 'Win32Exception'
                native_code = [int]$failureRow.NativeCode
                message = [string]$failureRow.Stage
            }
            $record.status = 'failure'
            $record.stage = [string]$failureRow.Stage
            $record.exit_code = [int]$failureRow.ExitCode
            $record.error = $error
            $record.operation_stage = [string]$failureRow.Stage
            $record.operation_error = $error
            $record.post_job_native_create_win32 = [int]$failureRow.PostCreate
            $record.post_job_native_created_new = [bool]$failureRow.PostCreated
            $record.post_job_native_close_win32 = [int]$failureRow.PostClose
            $record.semaphore_name = [string]$failureRow.ManagedName
            $record.created_new = $false
            $record.descendant_exit_code = -1
            $records.Add($record)
        }
        for ($index = 0; $index -lt $records.Count; $index++) {
            $path = [System.IO.Path]::Combine($fixtureRoot, "valid-$index.json")
            [System.IO.File]::WriteAllText(
                $path,
                ($records[$index] | ConvertTo-Json -Compress -Depth 8),
                [System.Text.UTF8Encoding]::new($false)
            )
            Read-NamedObjectProbeRecord -Path $path | Out-Null
        }

        $defaultSecurityDeniedPath = [System.IO.Path]::Combine(
            $fixtureRoot,
            'default-security-denied.json'
        )
        [System.IO.File]::WriteAllText(
            $defaultSecurityDeniedPath,
            ($defaultSecurityDenied | ConvertTo-Json -Compress -Depth 8),
            [System.Text.UTF8Encoding]::new($false)
        )
        $defaultSecurityDeniedRejected = $false
        try {
            Read-NamedObjectProbeRecord -Path $defaultSecurityDeniedPath | Out-Null
        }
        catch {
            $defaultSecurityDeniedRejected = $_.Exception.Message -match
                'stage, exit, or error relationship was invalid'
        }
        Require `
            $defaultSecurityDeniedRejected `
            "Named-object probe reader accepted denied session object creation as success."

        $legacy = [ordered]@{}
        foreach ($entry in $success.GetEnumerator()) {
            $legacy[$entry.Key] = $entry.Value
        }
        $legacy.schema_version = 2
        $legacyPath = [System.IO.Path]::Combine($fixtureRoot, 'legacy.json')
        [System.IO.File]::WriteAllText(
            $legacyPath,
            ($legacy | ConvertTo-Json -Compress -Depth 8),
            [System.Text.UTF8Encoding]::new($false)
        )
        $legacyRejected = $false
        try {
            Read-NamedObjectProbeRecord -Path $legacyPath | Out-Null
        }
        catch {
            $legacyRejected = $_.Exception.Message -match
                'field types or values were invalid'
        }
        Require $legacyRejected "Named-object probe reader accepted an obsolete schema."
    }
    finally {
        if ([System.IO.Directory]::Exists($fixtureRoot)) {
            [System.IO.Directory]::Delete($fixtureRoot, $true)
        }
    }
}

function Remove-NamedObjectProbeTemporaryRecords {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ResultPath,

        [Parameter(Mandatory = $true)]
        [string]$ExpectedParent
    )

    $resolvedResult = [System.IO.Path]::GetFullPath($ResultPath)
    $resolvedParent = [System.IO.Path]::GetDirectoryName($resolvedResult)
    $resolvedExpectedParent = [System.IO.Path]::GetFullPath($ExpectedParent).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $resultName = [System.IO.Path]::GetFileName($resolvedResult)
    Require `
        ([string]::Equals(
            $resolvedParent,
            $resolvedExpectedParent,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
            $resultName -match
                '\Aprojectatlas-object-namespace-probe-[0-9a-f]{32}\.json\z') `
        "Refused to inspect an unsafe named-object probe temporary prefix."
    $temporaryNamePattern = '\A' +
        [System.Text.RegularExpressions.Regex]::Escape($resultName) +
        '\.tmp-[0-9a-f]{32}\z'
    $temporaryNameFilter = "$resultName.tmp-*"
    foreach ($item in @(Get-ChildItem `
        -LiteralPath $resolvedExpectedParent `
        -Force `
        -Filter $temporaryNameFilter |
        Where-Object Name -Match $temporaryNamePattern)) {
        Require `
            (-not $item.PSIsContainer -and
                (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0) -and
                [string]::Equals(
                    [System.IO.Path]::GetDirectoryName($item.FullName),
                    $resolvedExpectedParent,
                    [System.StringComparison]::OrdinalIgnoreCase
                )) `
            "Refused to remove an unsafe named-object probe temporary file."
        Remove-Item -LiteralPath $item.FullName -Force
    }
    Require `
        (@(Get-ChildItem `
            -LiteralPath $resolvedExpectedParent `
            -Force `
            -Filter $temporaryNameFilter |
            Where-Object Name -Match $temporaryNamePattern).Count -eq 0) `
        "Named-object probe temporary result survived suite cleanup."
}

function Add-ScenarioObjectDirectoryAccess {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath,

        [Parameter(Mandatory = $true)]
        [hashtable]$State,

        [Parameter(Mandatory = $true)]
        [System.Security.Principal.SecurityIdentifier]$Sid
    )

    $objectDirectory = Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
        Get-ConstructionObjectDirectoryPath
    }
    $State.object_directory = [string]$objectDirectory
    Write-ScenarioState -StatePath $StatePath -State $State
    Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
        Add-ConstructionObjectDirectoryPrincipalAccess `
            -Path ([string]$State.object_directory) `
            -Sid $Sid
        Assert-ConstructionObjectDirectoryPrincipalAccess `
            -Path ([string]$State.object_directory) `
            -Sid $Sid
    }
    return [string]$objectDirectory
}

function Invoke-ScenarioCleanup {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath,

        [scriptblock]$AfterProcessTermination,

        [scriptblock]$AfterAccountRemoval
    )

    if ($null -eq $AfterProcessTermination -and $null -eq $AfterAccountRemoval) {
        Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
            Invoke-Cleanup
        }
        return
    }
    Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
        Invoke-Cleanup `
            -AfterProcessTermination $AfterProcessTermination `
            -AfterAccountRemoval $AfterAccountRemoval
    }
}

function Get-ExactLocalAccount {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Username,

        [string]$Sid
    )

    $matches = @(Get-LocalUser -ErrorAction Stop | Where-Object {
        $_.Name -ceq $Username -and
            ([string]::IsNullOrEmpty($Sid) -or
                ($null -ne $_.Sid -and $_.Sid.Value -eq $Sid))
    })
    Require ($matches.Count -le 1) "Recovery identity resolved to multiple local accounts."
    if ($matches.Count -eq 0) {
        return $null
    }
    return $matches[0]
}

function Get-ExactProfile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Sid
    )

    return @(Get-CimInstance `
        -ClassName Win32_UserProfile `
        -Filter "SID='$Sid'" `
        -ErrorAction Stop)
}

function Get-ExactFirewallRules {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [Parameter(Mandatory = $true)]
        [ValidateSet('PersistentStore', 'ActiveStore')]
        [string]$PolicyStore
    )

    return @(Get-NetFirewallRule `
        -PolicyStore $PolicyStore `
        -Name $Name `
        -ErrorAction SilentlyContinue)
}

function Get-ExactAclRules {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [System.Security.Principal.SecurityIdentifier]$Sid
    )

    if (-not (Test-Path -LiteralPath $Path)) {
        return @()
    }
    return @((Get-Acl -LiteralPath $Path).GetAccessRules(
        $true,
        $true,
        [System.Security.Principal.SecurityIdentifier]
    ) | Where-Object { $_.IdentityReference -eq $Sid })
}

function Assert-ScenarioAbsent {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Username,

        [Parameter(Mandatory = $true)]
        [string]$Sid,

        [Parameter(Mandatory = $true)]
        [string]$FirewallRule,

        [Parameter(Mandatory = $true)]
        [string]$StatePath,

        [string[]]$AclPaths = @(),

        [string]$ObjectDirectory = ''
    )

    $securityIdentifier = [System.Security.Principal.SecurityIdentifier]::new($Sid)
    Require `
        (@(Get-LocalUser -ErrorAction Stop | Where-Object {
            $_.Name -ceq $Username -or
                ($null -ne $_.Sid -and $_.Sid.Value -eq $Sid)
        }).Count -eq 0) `
        "Recovery cleanup left its exact account."
    Invoke-ExactSidProcessAudit -Sid $Sid -Expectation absent
    Require `
        (@(Get-ExactProfile -Sid $Sid).Count -eq 0) `
        "Recovery cleanup left its exact profile."
    Require `
        (@(Get-ExactFirewallRules -Name $FirewallRule -PolicyStore PersistentStore).Count -eq 0 -and
            @(Get-ExactFirewallRules -Name $FirewallRule -PolicyStore ActiveStore).Count -eq 0) `
        "Recovery cleanup left its exact firewall rule."
    foreach ($aclPath in $AclPaths) {
        Require `
            (@(Get-ExactAclRules -Path $aclPath -Sid $securityIdentifier).Count -eq 0) `
            "Recovery cleanup left an exact-SID ACL entry."
    }
    if (-not [string]::IsNullOrEmpty($ObjectDirectory)) {
        Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
            Assert-ConstructionObjectDirectoryPrincipalAbsent `
                -Path $ObjectDirectory `
                -Sid $securityIdentifier
        }
    }
    Require `
        (-not (Test-Path -LiteralPath $StatePath) -and
            -not (Test-Path -LiteralPath (Split-Path -Parent $StatePath))) `
        "Recovery cleanup left protected state."
}

function New-ScenarioAccount {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath
    )

    $identity = New-RecoveryIdentity
    $recoveryPasswords.Add($identity.Password)
    $state = @{
        schema_version = 2
        username = $identity.Username
        sid = $placeholderSid
        firewall_rule = $identity.FirewallRule
        object_directory = ''
        acl_paths = @()
        stage = 'identity'
    }
    Write-ScenarioState -StatePath $StatePath -State $state
    $account = Microsoft.PowerShell.LocalAccounts\New-LocalUser `
        -Name $identity.Username `
        -Password $identity.Password `
        -AccountExpires ([DateTime]::UtcNow.AddHours(2)) `
        -PasswordNeverExpires `
        -UserMayNotChangePassword `
        -Description $accountDescription `
        -ErrorAction Stop
    Require `
        ($null -ne $account.Sid -and $account.Sid.Value -match $sidPattern) `
        "Recovery account did not receive one local-user SID."
    $identity.Sid = $account.Sid.Value
    $state.sid = $identity.Sid
    Write-ScenarioState -StatePath $StatePath -State $state
    return [pscustomobject]@{
        Identity = $identity
        State = $state
        Account = $account
    }
}

function Invoke-AccountJournalRecoveryScenario {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath
    )

    $scenarioDirectory = Split-Path -Parent (Split-Path -Parent $StatePath)
    [System.IO.Directory]::CreateDirectory($scenarioDirectory) | Out-Null
    $parameterPath = Join-Path $scenarioDirectory 'construction-parameters.json'
    $accountCreatorPath = Join-Path $scenarioDirectory 'create-durable-account.ps1'
    $proxyPath = Join-Path $scenarioDirectory 'hold-after-durable-account.ps1'
    $readyMarkerPath = Join-Path $scenarioDirectory 'account-created.ready'
    $accountCreatorSource = @'
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Name,
    [Parameter(Mandatory = $true)][long]$AccountExpiresUtcTicks,
    [switch]$PasswordNeverExpires,
    [switch]$UserMayNotChangePassword,
    [Parameter(Mandatory = $true)][string]$Description
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$passwordEnvelope = [Console]::In.ReadToEnd()
if ([string]::IsNullOrWhiteSpace($passwordEnvelope) -or $passwordEnvelope.Length -gt 32768 -or
    $Name -notmatch '\Apa[0-9a-f]{12}\z' -or
    $Description -ne "ProjectAtlas optional parser pack construction") {
    throw "Account creator received invalid bounded input."
}
$password = $null
try {
    $password = ConvertTo-SecureString -String $passwordEnvelope -ErrorAction Stop
    $account = Microsoft.PowerShell.LocalAccounts\New-LocalUser `
        -Name $Name `
        -Password $password `
        -AccountExpires ([DateTime]::new($AccountExpiresUtcTicks, [DateTimeKind]::Utc)) `
        -PasswordNeverExpires:$PasswordNeverExpires `
        -UserMayNotChangePassword:$UserMayNotChangePassword `
        -Description $Description `
        -ErrorAction Stop
    if ($null -eq $account -or
        [string]$account.Name -cne $Name -or
        $null -eq $account.Sid -or
        $account.Sid.Value -notmatch '\AS-1-5-21-[0-9]+-[0-9]+-[0-9]+-[0-9]+\z' -or
        [string]$account.Description -ne $Description) {
        throw "Account creator did not receive one exact local identity."
    }
    [Console]::Out.Write($account.Sid.Value)
}
finally {
    if ($null -ne $password) {
        $password.Dispose()
    }
}
'@
    [System.IO.File]::WriteAllText(
        $accountCreatorPath,
        $accountCreatorSource,
        [System.Text.UTF8Encoding]::new($false)
    )
    $scenarioParameters = @{}
    foreach ($entry in $ConstructionParameters.GetEnumerator()) {
        $scenarioParameters[[string]$entry.Key] = $entry.Value
    }
    $scenarioParameters.StatePath = $StatePath
    [System.IO.File]::WriteAllText(
        $parameterPath,
        (@{
            wrapper = $ProductionWrapper
            parameters = $scenarioParameters
            account_creator = $accountCreatorPath
            ready_marker = $readyMarkerPath
            expected_description = $accountDescription
            placeholder_sid = $placeholderSid
        } | ConvertTo-Json -Depth 8 -Compress),
        [System.Text.UTF8Encoding]::new($false)
    )
    $proxySource = @'
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ParameterPath
)
$ErrorActionPreference = "Stop"
$payload = [System.IO.File]::ReadAllText($ParameterPath) |
    ConvertFrom-Json -AsHashtable -Depth 8
$parameters = [hashtable]$payload.parameters
$wrapperPath = (Get-Item -LiteralPath ([string]$payload.wrapper) -Force).FullName
$proxyAccountCreatorPath = (Get-Item -LiteralPath ([string]$payload.account_creator) -Force).FullName
$proxyReadyMarkerPath = [System.IO.Path]::GetFullPath([string]$payload.ready_marker)
$proxyExpectedDescription = [string]$payload.expected_description
$proxyPlaceholderSid = [string]$payload.placeholder_sid
$parameterDirectory = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::GetDirectoryName($ParameterPath)
)
if ([System.IO.Path]::GetDirectoryName($proxyAccountCreatorPath) -ne $parameterDirectory -or
    ((Get-Item -LiteralPath $proxyAccountCreatorPath -Force).Attributes -band
        [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
    [System.IO.Path]::GetDirectoryName($proxyReadyMarkerPath) -ne $parameterDirectory -or
    [System.IO.File]::Exists($proxyReadyMarkerPath) -or
    $proxyExpectedDescription -ne "ProjectAtlas optional parser pack construction" -or
    $proxyPlaceholderSid -ne "S-1-5-21-0-0-0-0") {
    throw "Account-ready marker path is unsafe."
}
$proxyStatePath = [System.IO.Path]::GetFullPath([string]$parameters.StatePath)
function global:New-LocalUser {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][System.Security.SecureString]$Password,
        [Parameter(Mandatory = $true)][DateTime]$AccountExpires,
        [switch]$PasswordNeverExpires,
        [switch]$UserMayNotChangePassword,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $creationStart = [System.Diagnostics.ProcessStartInfo]::new()
    $creationStart.FileName = [string]$parameters.PwshPath
    $creationStart.UseShellExecute = $false
    $creationStart.CreateNoWindow = $true
    $creationStart.RedirectStandardInput = $true
    $creationStart.RedirectStandardOutput = $true
    $creationStart.RedirectStandardError = $true
    foreach ($argument in @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
        '-File', $proxyAccountCreatorPath,
        '-Name', $Name,
        '-AccountExpiresUtcTicks', $AccountExpires.ToUniversalTime().Ticks,
        '-Description', $Description
    )) {
        $creationStart.ArgumentList.Add([string]$argument)
    }
    if ($PasswordNeverExpires) {
        $creationStart.ArgumentList.Add('-PasswordNeverExpires')
    }
    if ($UserMayNotChangePassword) {
        $creationStart.ArgumentList.Add('-UserMayNotChangePassword')
    }

    $creationProcess = $null
    $creationProcessId = 0
    $creationReaped = $false
    $creationOutputTask = $null
    $creationErrorTask = $null
    $creationExitCode = $null
    $creationOutput = $null
    $creationError = $null
    $creationOperationFailure = $null
    $creationCleanupFailures = [System.Collections.Generic.List[System.Exception]]::new()
    try {
        $creationProcess = [System.Diagnostics.Process]::Start($creationStart)
        if ($null -eq $creationProcess) {
            throw "Could not start the durable account creator."
        }
        $creationProcessId = $creationProcess.Id
        $creationOutputTask = $creationProcess.StandardOutput.ReadToEndAsync()
        $creationErrorTask = $creationProcess.StandardError.ReadToEndAsync()
        $passwordEnvelope = ConvertFrom-SecureString -SecureString $Password -ErrorAction Stop
        $creationProcess.StandardInput.Write($passwordEnvelope)
        $creationProcess.StandardInput.Close()
        if (-not $creationProcess.WaitForExit(30000)) {
            $creationProcess.Kill($true)
            if (-not $creationProcess.WaitForExit(5000)) {
                throw "Durable account creator could not be reaped after timeout."
            }
            $creationReaped = $true
            throw "Durable account creator exceeded its fixed deadline."
        }
        $creationReaped = $true
        if ($null -ne (Get-Process -Id $creationProcessId -ErrorAction SilentlyContinue)) {
            throw "Durable account creator PID survived successful completion."
        }
        $creationOutput = $creationOutputTask.GetAwaiter().GetResult()
        $creationError = $creationErrorTask.GetAwaiter().GetResult()
        if ($creationOutput.Length -gt 128 -or $creationError.Length -gt 4096) {
            throw "Durable account creator exceeded its bounded receipt sizes."
        }
        $creationExitCode = $creationProcess.ExitCode
    }
    catch {
        $creationOperationFailure = $_.Exception
    }
    finally {
        if ($null -ne $creationProcess) {
            try {
                if (-not $creationProcess.HasExited) {
                    $creationProcess.Kill($true)
                    if (-not $creationProcess.WaitForExit(5000)) {
                        throw "Fallback durable account creator termination could not be reaped."
                    }
                    $creationReaped = $true
                }
                else {
                    $creationReaped = $true
                }
            }
            catch {
                $creationCleanupFailures.Add($_.Exception)
            }
            if ($creationReaped) {
                try {
                    if ($null -ne $creationOutputTask -and $null -eq $creationOutput) {
                        $creationOutput = $creationOutputTask.GetAwaiter().GetResult()
                    }
                    if ($null -ne $creationErrorTask -and $null -eq $creationError) {
                        $creationError = $creationErrorTask.GetAwaiter().GetResult()
                    }
                }
                catch {
                    $creationCleanupFailures.Add($_.Exception)
                }
            }
            try {
                $creationProcess.Dispose()
            }
            catch {
                $creationCleanupFailures.Add($_.Exception)
            }
        }
    }
    if ($null -ne $creationOperationFailure) {
        if ($creationCleanupFailures.Count -ne 0) {
            throw [System.AggregateException]::new(
                "Durable account creation operation and cleanup failed.",
                @($creationOperationFailure) + @($creationCleanupFailures)
            )
        }
        throw $creationOperationFailure
    }
    if ($creationCleanupFailures.Count -ne 0) {
        throw [System.AggregateException]::new(
            "Durable account creation cleanup failed.",
            @($creationCleanupFailures)
        )
    }
    if ($creationExitCode -ne 0 -or
        -not [string]::IsNullOrEmpty($creationError) -or
        $creationOutput -notmatch '\AS-1-5-21-[0-9]+-[0-9]+-[0-9]+-[0-9]+\z') {
        throw "Durable account creator did not publish one bounded successful receipt."
    }

    $publishedAccounts = @(
        Microsoft.PowerShell.LocalAccounts\Get-LocalUser -ErrorAction Stop |
            Where-Object { $_.Name -ceq $Name }
    )
    if ($publishedAccounts.Count -ne 1 -or
        $null -eq $publishedAccounts[0].Sid -or
        $creationOutput -ne $publishedAccounts[0].Sid.Value -or
        $publishedAccounts[0].Sid.Value -notmatch
            '\AS-1-5-21-[0-9]+-[0-9]+-[0-9]+-[0-9]+\z' -or
        [string]$publishedAccounts[0].Description -ne $Description) {
        throw "Completed account creation did not publish one exact durable identity."
    }

    $stateItem = Get-Item -LiteralPath $proxyStatePath -Force
    if (($stateItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
        $stateItem.Length -le 0) {
        throw "Protected construction journal was not ready for the account marker."
    }
    $proxyJournal = [System.IO.File]::ReadAllText($stateItem.FullName) |
        ConvertFrom-Json -Depth 6
    if ([int]$proxyJournal.schema_version -ne 2 -or
        [string]$proxyJournal.username -cne $Name -or
        [string]$proxyJournal.sid -ne $proxyPlaceholderSid -or
        [string]$proxyJournal.firewall_rule -notmatch
            '\AProjectAtlas-ParserPack-Construction-[0-9a-f]{12}\z' -or
        [string]$proxyJournal.stage -ne 'identity' -or
        [string]$proxyJournal.object_directory -ne '' -or
        @($proxyJournal.acl_paths).Count -ne 0 -or
        $Description -ne $proxyExpectedDescription) {
        throw "Durable construction account did not retain its placeholder journal."
    }

    $markerTemporaryPath = "$proxyReadyMarkerPath.$([Guid]::NewGuid().ToString('N')).tmp"
    try {
        [System.IO.File]::WriteAllText(
            $markerTemporaryPath,
            "projectatlas-account-created-ready-v1`n",
            [System.Text.UTF8Encoding]::new($false)
        )
        Set-Acl `
            -LiteralPath $markerTemporaryPath `
            -AclObject (Get-Acl -LiteralPath $proxyStatePath)
        if (-not (Get-Acl -LiteralPath $markerTemporaryPath).AreAccessRulesProtected) {
            throw "Account-ready marker ACL was not protected."
        }
        [System.IO.File]::Move($markerTemporaryPath, $proxyReadyMarkerPath)
    }
    finally {
        if ([System.IO.File]::Exists($markerTemporaryPath)) {
            Remove-Item -LiteralPath $markerTemporaryPath -Force
        }
    }

    $accountPublicationGate = [System.Threading.ManualResetEventSlim]::new($false)
    try {
        $accountPublicationGate.Wait()
    }
    finally {
        $accountPublicationGate.Dispose()
    }
    throw "Durable account proxy returned unexpectedly."
}
& $wrapperPath @parameters
throw "Construction wrapper returned after the durable account proxy."
'@
    [System.IO.File]::WriteAllText(
        $proxyPath,
        $proxySource,
        [System.Text.UTF8Encoding]::new($false)
    )

    $wrapperStart = [System.Diagnostics.ProcessStartInfo]::new()
    $wrapperStart.FileName = [string]$ConstructionParameters.PwshPath
    $wrapperStart.UseShellExecute = $false
    $wrapperStart.CreateNoWindow = $true
    foreach ($argument in @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
        '-File', $proxyPath,
        '-ParameterPath', $parameterPath
    )) {
        $wrapperStart.ArgumentList.Add($argument)
    }

    $wrapperProcess = $null
    $wrapperProcessId = 0
    $wrapperOperationFailure = $null
    $wrapperCleanupFailures = [System.Collections.Generic.List[System.Exception]]::new()
    $observedAccount = $null
    $observedPlaceholderState = $null
    $markerObserved = $false
    $markerValidated = $false
    $accountObserved = $false
    $accountSidValidated = $false
    $accountDescriptionValidated = $false
    try {
        $wrapperProcess = [System.Diagnostics.Process]::Start($wrapperStart)
        if ($null -eq $wrapperProcess) {
            throw "Could not start the account-ready construction wrapper."
        }
        $wrapperProcessId = $wrapperProcess.Id
        $readyDeadline = [DateTime]::UtcNow.AddSeconds($processTimeoutSeconds)
        do {
            if ($wrapperProcess.HasExited) {
                throw "Account-ready construction wrapper exited before the recovery handshake."
            }
            if (-not $markerValidated -and [System.IO.File]::Exists($readyMarkerPath)) {
                $markerObserved = $true
                $readyItem = Get-Item -LiteralPath $readyMarkerPath -Force
                if (($readyItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
                    $readyItem.Length -le 0 -or
                    $readyItem.Length -gt 128 -or
                    [System.IO.File]::ReadAllText($readyItem.FullName) -ne
                        "projectatlas-account-created-ready-v1`n") {
                    throw "Account-ready marker was not one bounded regular marker."
                }
                $observedPlaceholderState = Invoke-WithCleanupDefinitions `
                    -StatePath $StatePath `
                    -Operation {
                        Assert-StateAcl -Path $readyMarkerPath
                        return Read-CleanupState
                    }
                Require `
                    ($null -ne $observedPlaceholderState) `
                    "Account-ready marker appeared without its protected journal."
                $markerValidated = $true
            }
            if ($markerValidated) {
                $candidateAccount = Get-ExactLocalAccount `
                    -Username ([string]$observedPlaceholderState.username)
                $accountObserved = $null -ne $candidateAccount
                $accountSidValidated =
                    $accountObserved -and
                    $null -ne $candidateAccount.Sid -and
                    $candidateAccount.Sid.Value -match $sidPattern
                $accountDescriptionValidated =
                    $accountObserved -and
                    [string]$candidateAccount.Description -eq $accountDescription
                if ($accountSidValidated -and $accountDescriptionValidated) {
                    $observedAccount = $candidateAccount
                    break
                }
            }
            Start-Sleep -Milliseconds 250
        } while ([DateTime]::UtcNow -lt $readyDeadline)
        $preKillObservation = @(
            "marker=$markerObserved"
            "marker_acl=$markerValidated"
            "journal=$($null -ne $observedPlaceholderState)"
            "account=$accountObserved"
            "account_sid=$accountSidValidated"
            "account_description=$accountDescriptionValidated"
            "process_alive=$(-not $wrapperProcess.HasExited)"
        ) -join ','
        Require `
            ($markerValidated -and
                $null -ne $observedPlaceholderState -and
                $null -ne $observedAccount -and
                -not $wrapperProcess.HasExited -and
                $null -ne (Get-Process -Id $wrapperProcessId -ErrorAction SilentlyContinue)) `
            "Pre-kill account-ready handshake did not complete under the fixed deadline ($preKillObservation)."

        $wrapperProcess.Kill($true)
        if (-not $wrapperProcess.WaitForExit(5000)) {
            throw "Account-ready construction wrapper could not be reaped."
        }
        $wrapperProcessAbsent =
            $null -eq (Get-Process -Id $wrapperProcessId -ErrorAction SilentlyContinue)
        Require `
            $wrapperProcessAbsent `
            "Account-ready construction wrapper PID survived abrupt termination."

        $postKillAccount = $null
        $accountVisibilityDeadline = [DateTime]::UtcNow.AddSeconds(10)
        do {
            $candidateAccount = Get-ExactLocalAccount `
                -Username ([string]$observedPlaceholderState.username)
            $postKillAccountObserved = $null -ne $candidateAccount
            $postKillAccountSidValidated =
                $postKillAccountObserved -and
                $null -ne $candidateAccount.Sid -and
                $candidateAccount.Sid.Value -eq $observedAccount.Sid.Value -and
                $candidateAccount.Sid.Value -match $sidPattern
            $postKillAccountDescriptionValidated =
                $postKillAccountObserved -and
                [string]$candidateAccount.Description -eq $accountDescription
            if ($postKillAccountSidValidated -and $postKillAccountDescriptionValidated) {
                $postKillAccount = $candidateAccount
                break
            }
            Start-Sleep -Milliseconds 250
        } while ([DateTime]::UtcNow -lt $accountVisibilityDeadline)
        $postKillObservation = @(
            "account=$postKillAccountObserved"
            "account_sid=$postKillAccountSidValidated"
            "account_description=$postKillAccountDescriptionValidated"
            "process_absent=$wrapperProcessAbsent"
        ) -join ','
        Require `
            ($null -ne $postKillAccount -and
                $postKillAccountSidValidated -and
                $postKillAccountDescriptionValidated -and
                $wrapperProcessAbsent) `
            "Post-kill account publication did not complete under the fixed deadline ($postKillObservation)."
    }
    catch {
        $wrapperOperationFailure = $_.Exception
    }
    finally {
        if ($null -ne $wrapperProcess) {
            try {
                if (-not $wrapperProcess.HasExited) {
                    $wrapperProcess.Kill($true)
                    if (-not $wrapperProcess.WaitForExit(5000)) {
                        throw "Fallback account-ready wrapper termination could not be reaped."
                    }
                }
            }
            catch {
                $wrapperCleanupFailures.Add($_.Exception)
            }
            try {
                $wrapperProcess.Dispose()
            }
            catch {
                $wrapperCleanupFailures.Add($_.Exception)
            }
        }
        try {
            if ([System.IO.File]::Exists($readyMarkerPath)) {
                Remove-Item -LiteralPath $readyMarkerPath -Force
            }
        }
        catch {
            $wrapperCleanupFailures.Add($_.Exception)
        }
    }
    if ($null -ne $wrapperOperationFailure) {
        if ($wrapperCleanupFailures.Count -ne 0) {
            throw [System.AggregateException]::new(
                "Account-ready recovery operation and cleanup failed.",
                @($wrapperOperationFailure) + @($wrapperCleanupFailures)
            )
        }
        throw $wrapperOperationFailure
    }
    if ($wrapperCleanupFailures.Count -ne 0) {
        throw [System.AggregateException]::new(
            "Account-ready recovery cleanup failed.",
            @($wrapperCleanupFailures)
        )
    }

    $placeholderState = Read-ScenarioState -StatePath $StatePath
    Require `
        ($null -ne $placeholderState -and
            [string]$placeholderState.username -eq [string]$observedPlaceholderState.username -and
            [string]$placeholderState.username -match $usernamePattern -and
            [string]$placeholderState.sid -eq $placeholderSid -and
            [string]$placeholderState.firewall_rule -match $ruleNamePattern -and
            [string]$placeholderState.stage -eq 'identity' -and
            [string]$placeholderState.object_directory -eq '' -and
            @($placeholderState.acl_paths).Count -eq 0) `
        "Abrupt account-ready exit did not retain the exact placeholder journal."
    $account = Get-ExactLocalAccount -Username ([string]$placeholderState.username)
    Require `
        ($null -ne $account -and
            $null -ne $account.Sid -and
            $account.Sid.Value -eq $observedAccount.Sid.Value -and
            $account.Sid.Value -match $sidPattern -and
            [string]$account.Description -eq $accountDescription) `
        "Abrupt account-ready exit did not retain the validated generated identity."

    $checkpointFailure = $null
    try {
        Invoke-ScenarioCleanup `
            -StatePath $StatePath `
            -AfterProcessTermination { throw 'account-journal-rebound' }
    }
    catch {
        $checkpointFailure = $_.Exception.Message
    }
    Require `
        ($checkpointFailure -eq 'account-journal-rebound') `
        "Account-journal cleanup did not reach its post-process checkpoint."
    $boundState = Read-ScenarioState -StatePath $StatePath
    Require `
        ([string]$boundState.username -eq [string]$placeholderState.username -and
            [string]$boundState.sid -eq $account.Sid.Value -and
            [string]$boundState.firewall_rule -eq [string]$placeholderState.firewall_rule -and
            [string]$boundState.stage -eq 'processes_absent' -and
            [string]$boundState.object_directory -eq '' -and
            @($boundState.acl_paths).Count -eq 0) `
        "Production cleanup did not bind only the validated account SID."

    Invoke-ScenarioCleanup -StatePath $StatePath
    Assert-ScenarioAbsent `
        -Username ([string]$boundState.username) `
        -Sid ([string]$boundState.sid) `
        -FirewallRule ([string]$boundState.firewall_rule) `
        -StatePath $StatePath
}

function Initialize-ConstructionAdapter {
    Invoke-Expression $nativeSourceAssignment.Extent.Text
    if (-not ('ProjectAtlasConstructionProcess' -as [type])) {
        Add-Type -TypeDefinition $nativeSource -Language CSharp
    }
    [ProjectAtlasConstructionProcess]::ConfigureBrokerJob($BrokerJobName)
    return [ProjectAtlasConstructionProcess]
}

function Invoke-LegacyJournalCompatibilityScenario {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath
    )

    $identity = New-RecoveryIdentity
    $legacyState = @{
        schema_version = 1
        username = $identity.Username
        sid = $placeholderSid
        firewall_rule = $identity.FirewallRule
        acl_paths = @()
        stage = 'identity'
    }
    $operationFailure = $null
    $cleanupFailure = $null
    try {
        Write-ScenarioState -StatePath $StatePath -State $legacyState
        $normalized = Read-ScenarioState -StatePath $StatePath
        Require `
            ([int]$normalized.schema_version -eq 2 -and
                [string]$normalized.object_directory -eq '' -and
                [string]$normalized.username -eq $identity.Username -and
                [string]$normalized.sid -eq $placeholderSid) `
            "Legacy cleanup journal did not normalize to the current empty object-directory state."

        $invalidState = @{
            schema_version = 2
            username = $identity.Username
            sid = $placeholderSid
            firewall_rule = $identity.FirewallRule
            object_directory = '\Sessions\01\BaseNamedObjects'
            acl_paths = @()
            stage = 'identity'
        }
        Write-ScenarioState -StatePath $StatePath -State $invalidState
        $invalidFailure = $null
        try {
            Read-ScenarioState -StatePath $StatePath | Out-Null
        }
        catch {
            $invalidFailure = $_.Exception.Message
        }
        Require `
            ($invalidFailure -eq 'Construction cleanup state contains invalid values.') `
            "Cleanup journal accepted a non-canonical object-directory path."
    }
    catch {
        $operationFailure = $_.Exception
    }
    finally {
        try {
            Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
                Remove-StateStorage
            }
        }
        catch {
            $cleanupFailure = $_.Exception
        }
    }
    $identity.Password.Dispose()
    if ($null -ne $operationFailure -and $null -ne $cleanupFailure) {
        throw [System.AggregateException]::new(
            "Legacy journal validation and cleanup both failed.",
            @($operationFailure, $cleanupFailure)
        )
    }
    if ($null -ne $operationFailure) { throw $operationFailure }
    if ($null -ne $cleanupFailure) { throw $cleanupFailure }
    Require `
        (-not [System.IO.Directory]::Exists((Split-Path -Parent $StatePath))) `
        "Legacy journal compatibility scenario left protected state."
}

function Get-ReflectedReceiptValue {
    param(
        [Parameter(Mandatory = $true)]
        [Type]$ReceiptType,

        [Parameter(Mandatory = $true)]
        [object]$Receipt,

        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $property = $ReceiptType.GetProperty(
        $Name,
        [System.Reflection.BindingFlags]'NonPublic,Instance'
    )
    Require ($null -ne $property) "Construction admission receipt lost one required field."
    return $property.GetValue($Receipt)
}

function Assert-TokenNamespaceSnapshot {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Snapshot
    )

    $flags = [System.Reflection.BindingFlags]'NonPublic,Instance'
    $values = @{}
    foreach ($name in @(
        'HasRestrictions', 'RestrictedSidCount', 'IsAppContainer', 'IsSandboxed',
        'IsAppSilo', 'BnoIsolationEnabled', 'BnoIsolationPrefix',
        'PrivateNamespaceQueryWin32', 'PrivateNamespaceInformationLength'
    )) {
        $property = $Snapshot.GetType().GetProperty($name, $flags)
        Require ($null -ne $property) "Token namespace snapshot lost $name."
        $values[$name] = $property.GetValue($Snapshot)
    }
    Require `
        (-not [bool]$values.HasRestrictions -and
            [uint32]$values.RestrictedSidCount -eq 0 -and
            -not [bool]$values.IsAppContainer -and
            -not [bool]$values.IsSandboxed -and
            -not [bool]$values.IsAppSilo -and
            -not [bool]$values.BnoIsolationEnabled -and
            [string]$values.BnoIsolationPrefix -ceq '' -and
            [int]$values.PrivateNamespaceInformationLength -ge 0) `
        "Construction token entered an isolated or restricted namespace."
    return $values
}

function Format-NamedObjectAccessComparison {
    param(
        [Parameter(Mandatory = $true)]
        [Type]$ReceiptType,

        [Parameter(Mandatory = $true)]
        [object]$Receipt,

        [AllowNull()]
        [pscustomobject]$Record
    )

    $seedName = [string](Get-ReflectedReceiptValue `
        -ReceiptType $ReceiptType `
        -Receipt $Receipt `
        -Name SeededSemaphoreName)
    $seedCreated = [bool](Get-ReflectedReceiptValue `
        -ReceiptType $ReceiptType `
        -Receipt $Receipt `
        -Name SeededSemaphoreCreatedNew)
    $seedDuplicated = [bool](Get-ReflectedReceiptValue `
        -ReceiptType $ReceiptType `
        -Receipt $Receipt `
        -Name SeededSemaphoreDuplicated)
    $parentClosed = [bool](Get-ReflectedReceiptValue `
        -ReceiptType $ReceiptType `
        -Receipt $Receipt `
        -Name SeededSemaphoreParentHandleClosed)
    $postCreate = if ($null -eq $Record) { '' } else { [string]$Record.post_job_native_create_win32 }
    $postCreated = if ($null -eq $Record) { '' } else { [string]$Record.post_job_native_created_new }
    $postClose = if ($null -eq $Record) { '' } else { [string]$Record.post_job_native_close_win32 }
    $sessionId = if ($null -eq $Record) { '' } else { [string]$Record.session_id }
    $directoryTraverse = if ($null -eq $Record) { '' } else { [string]$Record.directory_traverse_ntstatus }
    $directoryCreate = if ($null -eq $Record) { '' } else { [string]$Record.directory_create_object_ntstatus }
    $directoryCombined = if ($null -eq $Record) { '' } else { [string]$Record.directory_traverse_create_ntstatus }
    $sessionDirectoryTraverse = if ($null -eq $Record) { '' } else { [string]$Record.session_directory_traverse_ntstatus }
    $seededDirectOpen = if ($null -eq $Record) { '' } else { [string]$Record.seeded_direct_open_ntstatus }
    $seededDirectClose = if ($null -eq $Record) { '' } else { [string]$Record.seeded_direct_open_close_ntstatus }
    $sameName = if ($null -eq $Record) {
        ''
    }
    else {
        [string][string]::Equals(
            $seedName,
            [string]$Record.seeded_semaphore_name,
            [System.StringComparison]::Ordinal
        )
    }
    return "seed_created=$seedCreated seed_duplicated=$seedDuplicated parent_seed_closed=$parentClosed seeded_name_matches=$sameName session_id=$sessionId post_job_create_win32=$postCreate post_job_created_new=$postCreated post_job_close_win32=$postClose directory_traverse_ntstatus=$directoryTraverse directory_create_object_ntstatus=$directoryCreate directory_traverse_create_ntstatus=$directoryCombined session_directory_traverse_ntstatus=$sessionDirectoryTraverse seeded_direct_open_ntstatus=$seededDirectOpen seeded_direct_open_close_ntstatus=$seededDirectClose"
}

function Get-ReflectedOperationFailure {
    param(
        [Parameter(Mandatory = $true)]
        [System.Reflection.MethodInfo]$Method,

        [Parameter(Mandatory = $true)]
        [object[]]$Arguments
    )

    try {
        $Method.Invoke($null, $Arguments) | Out-Null
    }
    catch {
        $failure = $_.Exception
        while (($failure -is [System.Reflection.TargetInvocationException] -or
                $failure -is [System.Management.Automation.MethodInvocationException]) -and
            $null -ne $failure.InnerException) {
            $failure = $failure.InnerException
        }
        return $failure
    }
    throw "Construction admission fault unexpectedly succeeded."
}

function Assert-AdmissionReceipt {
    param(
        [Parameter(Mandatory = $true)]
        [Type]$ReceiptType,

        [Parameter(Mandatory = $true)]
        [object]$Receipt,

        [bool]$ExpectTermination = $true
    )

    $processId = [int](Get-ReflectedReceiptValue `
        -ReceiptType $ReceiptType `
        -Receipt $Receipt `
        -Name ProcessId)
    Require `
        ($processId -gt 0 -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name TerminationAttempted) -eq $ExpectTermination -and
            [uint32](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name WaitResult) -eq 0 -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name Reaped) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name JobHandleOwned) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name JobHandleClosed) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ProcessHandleOwned) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ProcessHandleClosed) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ThreadHandleOwned) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ThreadHandleClosed) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name LogonTokenHandleOwned) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name LogonTokenHandleClosed) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ConstructionTokenHandleOwned) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ConstructionTokenHandleClosed)) `
        "Construction admission recovery receipt was incomplete."
    Require `
        ($null -eq (Get-Process -Id $processId -ErrorAction SilentlyContinue)) `
        "Construction admission recovery left its exact PID alive."
}

function New-MinimalUserEnvironmentBlock {
    $values = [ordered]@{
        CARGO_BUILD_JOBS = '1'
        ComSpec = $env:ComSpec
        LOCALAPPDATA = (Join-Path $env:SystemRoot 'Temp')
        OS = 'Windows_NT'
        PATH = "$(Split-Path -Parent ([string]$ConstructionParameters.PwshPath));$env:SystemRoot\System32"
        PATHEXT = $env:PATHEXT
        SystemRoot = $env:SystemRoot
        TEMP = (Join-Path $env:SystemRoot 'Temp')
        TMP = (Join-Path $env:SystemRoot 'Temp')
        WINDIR = $env:WINDIR
    }
    $environmentBlock = (($values.GetEnumerator() | ForEach-Object {
        "$($_.Key)=$($_.Value)"
    }) -join "`0") + "`0`0"
    Require `
        ($environmentBlock.Length -le 32766) `
        "Construction recovery environment exceeded the Unicode process limit."
    return $environmentBlock
}

function Invoke-ConstructionAdmissionRecoveryScenario {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath
    )

    $scenario = New-ScenarioAccount -StatePath $StatePath
    $identity = $scenario.Identity
    $objectDirectory = Add-ScenarioObjectDirectoryAccess `
        -StatePath $StatePath `
        -State $scenario.State `
        -Sid $scenario.Account.Sid
    $duplicateGrantRejected = $false
    try {
        Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
            Add-ConstructionObjectDirectoryPrincipalAccess `
                -Path $objectDirectory `
                -Sid $scenario.Account.Sid
        }
    }
    catch {
        $duplicateGrantRejected = $_.Exception.Message -match
            'construction-object-directory-principal-already-present'
    }
    Require $duplicateGrantRejected "Object-directory grant accepted a preexisting exact-SID ACE."
    $adapterType = Initialize-ConstructionAdapter
    $scenarioType = $adapterType.GetNestedType(
        'AdmissionScenario',
        [System.Reflection.BindingFlags]::NonPublic
    )
    $receiptType = $adapterType.GetNestedType(
        'AdmissionReceipt',
        [System.Reflection.BindingFlags]::NonPublic
    )
    $runCore = $adapterType.GetMethod(
        'RunCore',
        [System.Reflection.BindingFlags]'NonPublic,Static'
    )
    Require `
        ($null -ne $scenarioType -and $null -ne $receiptType -and $null -ne $runCore) `
        "Construction adapter lost its private recovery boundary."
    Require `
        (([enum]::GetNames($scenarioType) -join ',') -eq
            'Normal,RetainedJobBeforeAdmission,FailBeforeJobAssignment,FailBeforeJobAssignmentAndCleanupFailure') `
        "Construction adapter recovery scenario domain changed."

    $failureArguments = [string[]]@(
        '-NoLogo', '-NoProfile', '-NonInteractive',
        '-Command', 'Start-Sleep -Seconds 30'
    )
    $probeId = [Guid]::NewGuid().ToString('N')
    $probeScriptPath = [System.IO.Path]::Combine(
        $env:SystemRoot,
        'Temp',
        "projectatlas-object-namespace-probe-$probeId.ps1"
    )
    $probeResultPath = [System.IO.Path]::Combine(
        $env:SystemRoot,
        'Temp',
        "projectatlas-object-namespace-probe-$probeId.json"
    )
    $probeCanaryPath = [System.IO.Path]::Combine(
        $env:SystemRoot,
        'Temp',
        "projectatlas-object-namespace-canary-$probeId.ps1"
    )
    $namespaceProbePaths.Add($probeScriptPath)
    $namespaceProbePaths.Add($probeResultPath)
    $namespaceProbePaths.Add($probeCanaryPath)
    $namespaceProbeResultPaths.Add($probeResultPath)
    Require `
        (-not [System.IO.File]::Exists($probeScriptPath) -and
            -not [System.IO.Directory]::Exists($probeScriptPath) -and
            -not [System.IO.File]::Exists($probeResultPath) -and
            -not [System.IO.Directory]::Exists($probeResultPath) -and
            -not [System.IO.File]::Exists($probeCanaryPath) -and
            -not [System.IO.Directory]::Exists($probeCanaryPath)) `
        "Construction named-object probe paths were not disposable."
    $namespaceProbeSource = @'
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ExpectedPrincipalSid,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedOwnerSid,

    [Parameter(Mandatory = $true)]
    [string]$ResultPath,

    [Parameter(Mandatory = $true)]
    [string]$CanaryPath,

    [ValidatePattern('\A(?:|ProjectAtlasParserPack-[0-9a-f]{32})\z')]
    [string]$ComparisonSemaphoreName = '',

    [Parameter(Mandatory = $true)]
    [ValidatePattern('\AProjectAtlasParserPack-[0-9a-f]{32}\z')]
    [string]$SeededSemaphoreName,

    [ValidateSet('none', 'operation-and-cleanup', 'descendant-open-not-found')]
    [string]$DiagnosticFault = 'none'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$probeExitCodes = [ordered]@{
    'identity' = 121
    'ambient-environment' = 122
    'semaphore-acl' = 123
    'semaphore-create' = 124
    'cargo-makeflags' = 125
    'descendant-launch' = 126
    'descendant-open' = 127
    'result-write' = 128
    'cleanup' = 129
    'native-semaphore-create' = 130
    'native-semaphore-close' = 131
}

function ConvertTo-BoundedProbeError {
    param(
        [Parameter(Mandatory = $true)]
        [System.Exception]$Exception
    )

    $failure = $Exception
    while (($failure -is [System.Management.Automation.MethodInvocationException] -or
            $failure -is [System.Management.Automation.RuntimeException]) -and
        $null -ne $failure.InnerException) {
        $failure = $failure.InnerException
    }
    $type = ($failure.GetType().Name -replace '[^A-Za-z0-9_.]', '_')
    if ($type.Length -lt 1) { $type = 'Exception' }
    if ($type.Length -gt 96) { $type = $type.Substring(0, 96) }
    $message = [string]$failure.Message
    foreach ($redaction in @(
        $ExpectedPrincipalSid,
        $ExpectedOwnerSid,
        $ResultPath,
        $CanaryPath
    )) {
        if (-not [string]::IsNullOrEmpty($redaction)) {
            $message = $message.Replace($redaction, '<redacted>')
        }
    }
    $message = [System.Text.RegularExpressions.Regex]::Replace(
        $message,
        '(?i)(?:''(?:[A-Z]:[\\/]|[\\/]{2})[^'']*''|"(?:[A-Z]:[\\/]|[\\/]{2})[^"]*")',
        '<path>'
    )
    $message = [System.Text.RegularExpressions.Regex]::Replace(
        $message,
        '(?i)(?<![A-Za-z0-9_])(?:[A-Z]:[\\/]|[\\/]{2})[^\s,;''"]+',
        '<path>'
    )
    $message = ($message -replace '[\x00-\x1F\x7F]+', ' ').Trim()
    if ($message.Length -lt 1) { $message = 'probe-stage-failed' }
    if ($message.Length -gt 384) { $message = $message.Substring(0, 384) }
    $nativeCode = if ($failure -is [System.ComponentModel.Win32Exception]) {
        [int]$failure.NativeErrorCode
    }
    else {
        $null
    }
    return [ordered]@{
        type = $type
        native_code = $nativeCode
        message = $message
    }
}

function Write-AtomicProbeRecord {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [System.Collections.IDictionary]$Record
    )

    $parent = [System.IO.Path]::GetDirectoryName([System.IO.Path]::GetFullPath($Path))
    if (-not [System.IO.Directory]::Exists($parent) -or
        [System.IO.File]::Exists($Path) -or
        [System.IO.Directory]::Exists($Path)) {
        throw [System.InvalidOperationException]::new('probe-result-path-unavailable')
    }
    $json = $Record | ConvertTo-Json -Compress -Depth 6
    if ($json.Length -lt 1 -or $json.Length -gt 4096) {
        throw [System.InvalidOperationException]::new('probe-result-size')
    }
    $temporaryPath = "$Path.tmp-$([Guid]::NewGuid().ToString('N'))"
    try {
        [System.IO.File]::WriteAllText(
            $temporaryPath,
            $json,
            [System.Text.UTF8Encoding]::new($false)
        )
        $temporaryItem = Get-Item -LiteralPath $temporaryPath -Force
        if ($temporaryItem.PSIsContainer -or
            (($temporaryItem.Attributes -band
                [System.IO.FileAttributes]::ReparsePoint) -ne 0) -or
            $temporaryItem.Length -lt 1 -or
            $temporaryItem.Length -gt 4096) {
            throw [System.InvalidOperationException]::new('probe-result-temporary-file')
        }
        [System.IO.File]::Move($temporaryPath, $Path)
    }
    finally {
        if ([System.IO.File]::Exists($temporaryPath)) {
            [System.IO.File]::Delete($temporaryPath)
        }
    }
}

$nativeProbeSource = @"
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class ProjectAtlasNamedObjectAccessProbe
{
    private const uint ObjCaseInsensitive = 0x00000040;
    private const uint SemaphoreSynchronizeAndModify = 0x00100002;
    private const int ErrorAlreadyExists = 183;

    [StructLayout(LayoutKind.Sequential)]
    private struct UnicodeString
    {
        internal ushort Length;
        internal ushort MaximumLength;
        internal IntPtr Buffer;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ObjectAttributes
    {
        internal int Length;
        internal IntPtr RootDirectory;
        internal IntPtr ObjectName;
        internal uint Attributes;
        internal IntPtr SecurityDescriptor;
        internal IntPtr SecurityQualityOfService;
    }

    [DllImport("ntdll.dll")]
    private static extern int NtOpenDirectoryObject(
        out IntPtr directoryHandle,
        uint desiredAccess,
        ref ObjectAttributes objectAttributes);

    [DllImport("ntdll.dll")]
    private static extern int NtOpenSemaphore(
        out IntPtr semaphoreHandle,
        uint desiredAccess,
        ref ObjectAttributes objectAttributes);

    [DllImport("ntdll.dll")]
    private static extern int NtClose(IntPtr handle);

    [DllImport(
        "kernel32.dll",
        CharSet = CharSet.Unicode,
        SetLastError = true,
        EntryPoint = "CreateSemaphoreExW")]
    private static extern IntPtr CreateSemaphoreEx(
        IntPtr securityAttributes,
        int initialCount,
        int maximumCount,
        string name,
        uint flags,
        uint desiredAccess);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    [DllImport(
        "kernel32.dll",
        CharSet = CharSet.Unicode,
        SetLastError = true,
        EntryPoint = "OpenSemaphoreW")]
    private static extern IntPtr OpenSemaphore(
        uint desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool inheritHandle,
        string name);

    public static int OpenDirectory(string path, uint desiredAccess)
    {
        IntPtr pathBuffer = IntPtr.Zero;
        IntPtr unicodePointer = IntPtr.Zero;
        IntPtr directoryHandle = IntPtr.Zero;
        try
        {
            pathBuffer = Marshal.StringToHGlobalUni(path);
            UnicodeString unicode = new UnicodeString();
            unicode.Length = checked((ushort)(path.Length * sizeof(char)));
            unicode.MaximumLength = checked((ushort)((path.Length + 1) * sizeof(char)));
            unicode.Buffer = pathBuffer;
            unicodePointer = Marshal.AllocHGlobal(Marshal.SizeOf<UnicodeString>());
            Marshal.StructureToPtr(unicode, unicodePointer, false);
            ObjectAttributes attributes = new ObjectAttributes();
            attributes.Length = Marshal.SizeOf<ObjectAttributes>();
            attributes.RootDirectory = IntPtr.Zero;
            attributes.ObjectName = unicodePointer;
            attributes.Attributes = ObjCaseInsensitive;
            attributes.SecurityDescriptor = IntPtr.Zero;
            attributes.SecurityQualityOfService = IntPtr.Zero;
            int status = NtOpenDirectoryObject(
                out directoryHandle,
                desiredAccess,
                ref attributes);
            if (status < 0)
            {
                return status;
            }
            int closeStatus = NtClose(directoryHandle);
            directoryHandle = IntPtr.Zero;
            if (closeStatus < 0)
            {
                throw new InvalidOperationException("native-directory-probe-close");
            }
            return status;
        }
        finally
        {
            if (directoryHandle != IntPtr.Zero)
            {
                NtClose(directoryHandle);
            }
            if (unicodePointer != IntPtr.Zero)
            {
                Marshal.FreeHGlobal(unicodePointer);
            }
            if (pathBuffer != IntPtr.Zero)
            {
                Marshal.FreeHGlobal(pathBuffer);
            }
        }
    }

    public static int OpenAndCloseSemaphoreByPath(
        string path,
        out int closeStatus)
    {
        closeStatus = -1;
        IntPtr pathBuffer = IntPtr.Zero;
        IntPtr unicodePointer = IntPtr.Zero;
        IntPtr semaphoreHandle = IntPtr.Zero;
        try
        {
            pathBuffer = Marshal.StringToHGlobalUni(path);
            UnicodeString unicode = new UnicodeString();
            unicode.Length = checked((ushort)(path.Length * sizeof(char)));
            unicode.MaximumLength = checked((ushort)((path.Length + 1) * sizeof(char)));
            unicode.Buffer = pathBuffer;
            unicodePointer = Marshal.AllocHGlobal(Marshal.SizeOf<UnicodeString>());
            Marshal.StructureToPtr(unicode, unicodePointer, false);
            ObjectAttributes attributes = new ObjectAttributes();
            attributes.Length = Marshal.SizeOf<ObjectAttributes>();
            attributes.RootDirectory = IntPtr.Zero;
            attributes.ObjectName = unicodePointer;
            attributes.Attributes = ObjCaseInsensitive;
            attributes.SecurityDescriptor = IntPtr.Zero;
            attributes.SecurityQualityOfService = IntPtr.Zero;
            int status = NtOpenSemaphore(
                out semaphoreHandle,
                SemaphoreSynchronizeAndModify,
                ref attributes);
            if (status < 0)
            {
                return status;
            }
            closeStatus = NtClose(semaphoreHandle);
            semaphoreHandle = IntPtr.Zero;
            return status;
        }
        finally
        {
            if (semaphoreHandle != IntPtr.Zero)
            {
                NtClose(semaphoreHandle);
            }
            if (unicodePointer != IntPtr.Zero)
            {
                Marshal.FreeHGlobal(unicodePointer);
            }
            if (pathBuffer != IntPtr.Zero)
            {
                Marshal.FreeHGlobal(pathBuffer);
            }
        }
    }

    public static int CreateAndCloseSemaphore(
        string name,
        out bool createdNew,
        out int closeError)
    {
        createdNew = false;
        closeError = -1;
        IntPtr handle = CreateSemaphoreEx(
            IntPtr.Zero,
            1,
            1,
            name,
            0,
            SemaphoreSynchronizeAndModify);
        int createError = Marshal.GetLastWin32Error();
        if (handle == IntPtr.Zero)
        {
            return createError;
        }
        createdNew = createError != ErrorAlreadyExists;
        closeError = CloseHandle(handle) ? 0 : Marshal.GetLastWin32Error();
        return createdNew ? 0 : createError;
    }

    public static int OpenAndCloseSemaphore(
        string name,
        out int closeError)
    {
        closeError = -1;
        IntPtr handle = OpenSemaphore(
            SemaphoreSynchronizeAndModify,
            false,
            name);
        int openError = Marshal.GetLastWin32Error();
        if (handle == IntPtr.Zero)
        {
            return openError;
        }
        closeError = CloseHandle(handle) ? 0 : Marshal.GetLastWin32Error();
        return 0;
    }

    public static SafeWaitHandle OpenOwnedSemaphore(string name)
    {
        IntPtr handle = OpenSemaphore(
            SemaphoreSynchronizeAndModify,
            false,
            name);
        if (handle == IntPtr.Zero)
        {
            throw new System.ComponentModel.Win32Exception(
                Marshal.GetLastWin32Error(),
                "open-seeded-cargo-jobserver");
        }
        return new SafeWaitHandle(handle, true);
    }

}
"@

$probeStage = 'identity'
$sessionId = -1
$directoryPath = ''
$directoryTraverseNtStatus = -1
$directoryCreateObjectNtStatus = -1
$directoryTraverseCreateNtStatus = -1
$sessionDirectoryTraverseNtStatus = -1
$nativeSemaphoreName = ''
$postJobNativeCreateWin32 = -1
$postJobNativeCreatedNew = $false
$postJobNativeCloseWin32 = -1
$seededOpenWin32 = -1
$seededOpenCloseWin32 = -1
$seededDirectOpenNtStatus = -1
$seededDirectOpenCloseNtStatus = -1
$seededCreateWin32 = -1
$seededCreateCreatedNew = $false
$seededCreateCloseWin32 = -1
$name = ''
$createdNew = $false
$childExitCode = -1
$semaphore = $null
$child = $null
$childOutputTask = $null
$childErrorTask = $null
$operationStage = $null
$operationError = $null
$cleanupError = $null
try {
    $actualIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    $actualSid = $actualIdentity.User.Value
    $sessionId = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
    $directoryPath = if ($sessionId -eq 0) {
        '\BaseNamedObjects'
    }
    else {
        "\Sessions\$sessionId\BaseNamedObjects"
    }
    if (-not [string]::Equals(
            $actualSid,
            $ExpectedPrincipalSid,
            [System.StringComparison]::Ordinal
        )) {
        throw [System.InvalidOperationException]::new('construction-principal-sid-mismatch')
    }
    $probeStage = 'ambient-environment'
    if (Test-Path -LiteralPath Env:CARGO_MAKEFLAGS) {
        throw [System.InvalidOperationException]::new('ambient-cargo-makeflags')
    }

    $probeStage = 'identity'
    if ($null -eq $actualIdentity.Owner -or
        -not [string]::Equals(
            $actualIdentity.Owner.Value,
            $ExpectedOwnerSid,
            [System.StringComparison]::Ordinal
        )) {
        throw [System.InvalidOperationException]::new('construction-token-owner-sid-mismatch')
    }

    $probeStage = 'native-semaphore-create'
    Add-Type -TypeDefinition $nativeProbeSource -Language CSharp -ErrorAction Stop
    $probeStage = 'native-semaphore-close'
    $directoryTraverseNtStatus =
        [ProjectAtlasNamedObjectAccessProbe]::OpenDirectory($directoryPath, 0x00000002)
    $directoryCreateObjectNtStatus =
        [ProjectAtlasNamedObjectAccessProbe]::OpenDirectory($directoryPath, 0x00000004)
    $directoryTraverseCreateNtStatus =
        [ProjectAtlasNamedObjectAccessProbe]::OpenDirectory($directoryPath, 0x00000006)
    $sessionDirectoryPath = $directoryPath
    $sessionDirectoryTraverseNtStatus =
        [ProjectAtlasNamedObjectAccessProbe]::OpenDirectory(
            $sessionDirectoryPath,
            0x00000002
        )
    $seededNativePath = "$sessionDirectoryPath\$SeededSemaphoreName"
    $seededDirectOpenNtStatus =
        [ProjectAtlasNamedObjectAccessProbe]::OpenAndCloseSemaphoreByPath(
            $seededNativePath,
            [ref]$seededDirectOpenCloseNtStatus
        )

    $probeStage = 'native-semaphore-create'
    $nativeSemaphoreName = if ([string]::IsNullOrEmpty($ComparisonSemaphoreName)) {
        "ProjectAtlasParserPack-$([Guid]::NewGuid().ToString('N'))"
    }
    else {
        $ComparisonSemaphoreName
    }
    $postJobNativeCreateWin32 =
        [ProjectAtlasNamedObjectAccessProbe]::CreateAndCloseSemaphore(
            $nativeSemaphoreName,
            [ref]$postJobNativeCreatedNew,
            [ref]$postJobNativeCloseWin32
        )

    $probeStage = 'native-semaphore-create'
    $seededOpenWin32 =
        [ProjectAtlasNamedObjectAccessProbe]::OpenAndCloseSemaphore(
            $SeededSemaphoreName,
            [ref]$seededOpenCloseWin32
        )
    if ($seededOpenWin32 -ne 0 -or $seededOpenCloseWin32 -ne 0) {
        throw [System.ComponentModel.Win32Exception]::new(
            $seededOpenWin32,
            'open-seeded-cargo-jobserver'
        )
    }
    $seededCreateWin32 =
        [ProjectAtlasNamedObjectAccessProbe]::CreateAndCloseSemaphore(
            $SeededSemaphoreName,
            [ref]$seededCreateCreatedNew,
            [ref]$seededCreateCloseWin32
        )
    if ($seededCreateWin32 -ne 183 -or
        $seededCreateCreatedNew -or
        $seededCreateCloseWin32 -ne 0) {
        throw [System.InvalidOperationException]::new(
            'seeded-cargo-jobserver-existing-object-probe'
        )
    }

    $probeStage = 'semaphore-acl'
    if ($DiagnosticFault -ceq 'operation-and-cleanup') {
        throw [System.InvalidOperationException]::new(
            'diagnostic-operation-fault ordinary/token C:/private/forward.txt //server/share/unquoted.txt'
        )
    }
    $probeStage = 'semaphore-create'
    $name = $SeededSemaphoreName
    $semaphore = [ProjectAtlasNamedObjectAccessProbe]::OpenOwnedSemaphore($name)
    $createdNew = $false

    $probeStage = 'cargo-makeflags'
    $env:CARGO_MAKEFLAGS = "-j --jobserver-fds=$name --jobserver-auth=$name"
    if ([string]$env:CARGO_MAKEFLAGS -cne
        "-j --jobserver-fds=$name --jobserver-auth=$name") {
        throw [System.InvalidOperationException]::new('cargo-makeflags-write')
    }

    $probeStage = 'descendant-launch'
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $descendantName = if ($DiagnosticFault -ceq 'descendant-open-not-found') {
        "ProjectAtlasParserPack-$([Guid]::NewGuid().ToString('N'))"
    }
    else {
        $name
    }
    foreach ($argument in @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
        '-File', $CanaryPath,
        '-Name', $descendantName,
        '-ExpectedSid', $ExpectedPrincipalSid
    )) {
        $start.ArgumentList.Add($argument)
    }
    $child = [System.Diagnostics.Process]::Start($start)
    if ($null -eq $child) {
        throw [System.InvalidOperationException]::new('descendant-start-returned-null')
    }
    $childOutputTask = $child.StandardOutput.ReadToEndAsync()
    $childErrorTask = $child.StandardError.ReadToEndAsync()

    $probeStage = 'descendant-open'
    if (-not $child.WaitForExit(15000)) {
        throw [System.TimeoutException]::new('descendant-open-timeout')
    }
    $pipeTasks = [System.Threading.Tasks.Task[]]@($childOutputTask, $childErrorTask)
    if (-not [System.Threading.Tasks.Task]::WaitAll($pipeTasks, 5000)) {
        throw [System.TimeoutException]::new('descendant-diagnostic-pipe-timeout')
    }
    $childOutput = $childOutputTask.Result
    $childError = $childErrorTask.Result
    if ($childOutput.Length -gt 1024 -or $childError.Length -gt 1024) {
        throw [System.InvalidOperationException]::new('descendant-diagnostic-output-limit')
    }
    $childExitCode = [int]$child.ExitCode
    if ($childExitCode -ne 0) {
        if ($childExitCode -in @(143, 144)) {
            $nativeMatch = [System.Text.RegularExpressions.Regex]::Match(
                $childOutput,
                '\Anative_code=(-?[0-9]{1,10})\r?\n?\z'
            )
            $descendantNativeCode = 0
            if (-not $nativeMatch.Success -or
                -not [int]::TryParse(
                    $nativeMatch.Groups[1].Value,
                    [System.Globalization.NumberStyles]::Integer,
                    [System.Globalization.CultureInfo]::InvariantCulture,
                    [ref]$descendantNativeCode
                )) {
                throw [System.InvalidOperationException]::new(
                    'descendant-native-diagnostic-invalid'
                )
            }
            throw [System.ComponentModel.Win32Exception]::new(
                $descendantNativeCode,
                "descendant-open-exit-$childExitCode"
            )
        }
        throw [System.InvalidOperationException]::new(
            "descendant-open-exit-$childExitCode"
        )
    }
}
catch {
    $operationStage = $probeStage
    $operationError = ConvertTo-BoundedProbeError -Exception $_.Exception
}
finally {
    $probeStage = 'cleanup'
    $cleanupFailures = [System.Collections.Generic.List[System.Exception]]::new()
    Remove-Item -LiteralPath Env:CARGO_MAKEFLAGS -ErrorAction SilentlyContinue
    if ($null -ne $child) {
        try {
            if (-not $child.HasExited) {
                $child.Kill($true)
                if (-not $child.WaitForExit(5000)) {
                    throw [System.TimeoutException]::new('descendant-cleanup-timeout')
                }
            }
        }
        catch {
            $cleanupFailures.Add($_.Exception)
        }
        try {
            $child.Dispose()
        }
        catch {
            $cleanupFailures.Add($_.Exception)
        }
    }
    if ($null -ne $semaphore) {
        try {
            $semaphore.Dispose()
        }
        catch {
            $cleanupFailures.Add($_.Exception)
        }
    }
    if ($DiagnosticFault -ceq 'operation-and-cleanup') {
        $cleanupFailures.Add(
            [System.InvalidOperationException]::new(
                'diagnostic-cleanup-fault ordinary\token D:\private/mixed\cleanup.txt "//server/share/quoted forward.txt" \\server\share\backward.txt \/server/share\mixed.txt'
            )
        )
    }
    if ($cleanupFailures.Count -ne 0) {
        $cleanupFailure = if ($cleanupFailures.Count -eq 1) {
            $cleanupFailures[0]
        }
        else {
            [System.AggregateException]::new('probe-cleanup-failed', $cleanupFailures)
        }
        $cleanupError = ConvertTo-BoundedProbeError -Exception $cleanupFailure
    }
}

if ($null -ne $cleanupError) {
    $status = 'failure'
    $finalStage = 'cleanup'
    $exitCode = [int]$probeExitCodes[$finalStage]
    $finalError = $cleanupError
}
elseif ($null -ne $operationError) {
    $status = 'failure'
    $finalStage = [string]$operationStage
    $exitCode = [int]$probeExitCodes[$finalStage]
    $finalError = $operationError
}
else {
    $status = 'success'
    $finalStage = 'complete'
    $exitCode = 0
    $finalError = $null
}
$record = [ordered]@{
    schema_version = 5
    status = $status
    stage = $finalStage
    exit_code = $exitCode
    error = $finalError
    operation_stage = $operationStage
    operation_error = $operationError
    cleanup_error = $cleanupError
    session_id = $sessionId
    directory_path = $directoryPath
    directory_traverse_ntstatus = $directoryTraverseNtStatus
    directory_create_object_ntstatus = $directoryCreateObjectNtStatus
    directory_traverse_create_ntstatus = $directoryTraverseCreateNtStatus
    session_directory_traverse_ntstatus = $sessionDirectoryTraverseNtStatus
    native_semaphore_name = $nativeSemaphoreName
    post_job_native_create_win32 = $postJobNativeCreateWin32
    post_job_native_created_new = $postJobNativeCreatedNew
    post_job_native_close_win32 = $postJobNativeCloseWin32
    seeded_semaphore_name = $SeededSemaphoreName
    seeded_direct_open_ntstatus = $seededDirectOpenNtStatus
    seeded_direct_open_close_ntstatus = $seededDirectOpenCloseNtStatus
    seeded_open_win32 = $seededOpenWin32
    seeded_open_close_win32 = $seededOpenCloseWin32
    seeded_create_win32 = $seededCreateWin32
    seeded_create_created_new = $seededCreateCreatedNew
    seeded_create_close_win32 = $seededCreateCloseWin32
    semaphore_name = $name
    created_new = $createdNew
    descendant_exit_code = $childExitCode
}
$probeStage = 'result-write'
try {
    Write-AtomicProbeRecord -Path $ResultPath -Record $record
}
catch {
    $writeError = ConvertTo-BoundedProbeError -Exception $_.Exception
    $fallback = [ordered]@{
        schema_version = 5
        status = 'failure'
        stage = 'result-write'
        exit_code = [int]$probeExitCodes['result-write']
        error = $writeError
    }
    $fallbackJson = $fallback | ConvertTo-Json -Compress -Depth 4
    if ($fallbackJson.Length -gt 1024) {
        $fallbackJson = '{"schema_version":5,"status":"failure","stage":"result-write","exit_code":128,"error":{"type":"Exception","native_code":null,"message":"probe-result-write-failed"}}'
    }
    [Console]::Error.WriteLine($fallbackJson)
    exit [int]$probeExitCodes['result-write']
}
exit [int]$record.exit_code
'@
    $namespaceCanarySource = @'
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('\AProjectAtlasParserPack-[0-9a-f]{32}\z')]
    [string]$Name,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedSid
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value -ne $ExpectedSid) {
    exit 141
}
$nativeSource = @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class ProjectAtlasRecoveryJobserverCanary
{
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true,
        EntryPoint = "OpenSemaphoreW")]
    private static extern IntPtr OpenSemaphore(
        uint desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool inheritHandle,
        string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);
    public static void OpenAndClose(string name)
    {
        IntPtr handle = OpenSemaphore(0x00100002, false, name);
        if (handle == IntPtr.Zero)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "open-recovery-jobserver");
        }
        if (!CloseHandle(handle))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "close-recovery-jobserver");
        }
    }
}
"@
try {
    Add-Type -TypeDefinition $nativeSource -Language CSharp
}
catch {
    exit 142
}
try {
    [ProjectAtlasRecoveryJobserverCanary]::OpenAndClose($Name)
}
catch {
    $canaryFailure = $_.Exception
    while ($null -ne $canaryFailure.InnerException) {
        $canaryFailure = $canaryFailure.InnerException
    }
    $canaryNativeCode = if ($canaryFailure -is [System.ComponentModel.Win32Exception]) {
        [int]$canaryFailure.NativeErrorCode
    }
    else {
        0
    }
    [Console]::Out.WriteLine("native_code=$canaryNativeCode")
    if ([string]$canaryFailure.Message -match '^open-recovery-jobserver') {
        exit 143
    }
    exit 144
}
exit 0
'@
    [System.IO.File]::WriteAllText(
        $probeScriptPath,
        $namespaceProbeSource,
        [System.Text.UTF8Encoding]::new($false)
    )
    [System.IO.File]::WriteAllText(
        $probeCanaryPath,
        $namespaceCanarySource,
        [System.Text.UTF8Encoding]::new($false)
    )
    $comparisonSemaphoreName =
        "ProjectAtlasParserPack-$([Guid]::NewGuid().ToString('N'))"
    $normalArguments = [string[]]@(
        '-NoLogo', '-NoProfile', '-NonInteractive',
        '-File', $probeScriptPath,
        '-ExpectedPrincipalSid', $identity.Sid,
        '-ExpectedOwnerSid', $identity.Sid,
        '-ResultPath', $probeResultPath,
        '-CanaryPath', $probeCanaryPath,
        '-ComparisonSemaphoreName', $comparisonSemaphoreName,
        '-SeededSemaphoreName', '__PROJECTATLAS_SEEDED_SEMAPHORE__'
    )
    # RunCore validates its command line before authentication. Keep this probe
    # short so invalid credentials reach the intended LogonUser boundary.
    $authenticationProbeArguments = [string[]]@(
        '-NoLogo', '-NoProfile', '-NonInteractive',
        '-Command', 'exit 0'
    )
    $environmentBlock = New-MinimalUserEnvironmentBlock
    $invalidPassword = [System.Security.SecureString]::new()
    $invalidPassword.AppendChar('x')
    $invalidPassword.MakeReadOnly()
    try {
        $invalidReceipt = [Activator]::CreateInstance($receiptType, $true)
        $invalidArguments = [object[]]::new(10)
        $invalidArguments[0] = $identity.Username
        $invalidArguments[1] = $identity.Sid
        $invalidArguments[2] = $invalidPassword
        $invalidArguments[3] = [string]$ConstructionParameters.PwshPath
        $invalidArguments[4] = $authenticationProbeArguments
        $invalidArguments[5] = $env:SystemRoot
        $invalidArguments[6] = $environmentBlock
        $invalidArguments[7] = 30
        $invalidArguments[8] = [enum]::Parse($scenarioType, 'Normal')
        $invalidArguments[9] = $invalidReceipt
        $invalidCredentialFailure = Get-ReflectedOperationFailure `
            -Method $runCore `
            -Arguments $invalidArguments
        Require `
            ($invalidCredentialFailure -is [System.ComponentModel.Win32Exception] -and
                $invalidCredentialFailure.NativeErrorCode -eq 1326 -and
                $invalidCredentialFailure.Message -match '^logon-construction-principal' -and
                [int](Get-ReflectedReceiptValue `
                    -ReceiptType $receiptType `
                    -Receipt $invalidReceipt `
                    -Name ProcessId) -eq 0) `
            "Construction principal authentication did not fail closed for invalid credentials."
        Invoke-ExactSidProcessAudit -Sid $identity.Sid -Expectation absent
    }
    finally {
        $invalidPassword.Dispose()
    }

    foreach ($scenarioRow in @(
        [pscustomobject]@{
            Name = 'Normal'
            Arguments = $normalArguments
        },
        [pscustomobject]@{
            Name = 'RetainedJobBeforeAdmission'
            Arguments = $failureArguments
        },
        [pscustomobject]@{
            Name = 'FailBeforeJobAssignment'
            Arguments = $failureArguments
        },
        [pscustomobject]@{
            Name = 'FailBeforeJobAssignmentAndCleanupFailure'
            Arguments = $failureArguments
        }
    )) {
        $scenarioName = $scenarioRow.Name
        $admissionScenario = [enum]::Parse($scenarioType, $scenarioName)
        $receipt = [Activator]::CreateInstance($receiptType, $true)
        if ($scenarioName -eq 'Normal') {
            $seededNameProperty = $receiptType.GetProperty(
                'SeededSemaphoreName',
                [System.Reflection.BindingFlags]'NonPublic,Instance'
            )
            Require `
                ($null -ne $seededNameProperty) `
                "Construction receipt lost its seeded semaphore name."
            $seededNameProperty.SetValue(
                $receipt,
                "ProjectAtlasParserPack-$([Guid]::NewGuid().ToString('N'))"
            )
        }
        $invokeArguments = [object[]]::new(10)
        $invokeArguments[0] = $identity.Username
        $invokeArguments[1] = $identity.Sid
        $invokeArguments[2] = $identity.Password
        $invokeArguments[3] = [string]$ConstructionParameters.PwshPath
        $invokeArguments[4] = [string[]]$scenarioRow.Arguments
        $invokeArguments[5] = $env:SystemRoot
        $invokeArguments[6] = $environmentBlock
        $invokeArguments[7] = 30
        $invokeArguments[8] = $admissionScenario
        $invokeArguments[9] = $receipt
        if ($scenarioName -eq 'Normal') {
            try {
                $normalExitCode = [int]$runCore.Invoke($null, $invokeArguments)
            }
            catch {
                $normalFailure = $_.Exception
                while (($normalFailure -is [System.Reflection.TargetInvocationException] -or
                        $normalFailure -is [System.Management.Automation.MethodInvocationException]) -and
                    $null -ne $normalFailure.InnerException) {
                    $normalFailure = $normalFailure.InnerException
                }
                $nativeError = if ($normalFailure -is [System.ComponentModel.Win32Exception]) {
                    $normalFailure.NativeErrorCode
                }
                else {
                    $null
                }
                $normalMessage = ([string]$normalFailure.Message -replace '[\x00-\x1F\x7F]+', ' ').Trim()
                if ($normalMessage.Length -gt 512) {
                    $normalMessage = $normalMessage.Substring(0, 512)
                }
                $comparison = Format-NamedObjectAccessComparison `
                    -ReceiptType $receiptType `
                    -Receipt $receipt `
                    -Record $null
                throw "Construction normal admission invocation failed. type=$($normalFailure.GetType().Name) native_error_code=$nativeError message=$normalMessage comparison=$comparison"
            }
            $probeResult = $null
            if ([System.IO.File]::Exists($probeResultPath) -and
                -not [System.IO.Directory]::Exists($probeResultPath)) {
                try {
                    $probeResult = Read-NamedObjectProbeRecord -Path $probeResultPath
                }
                catch {
                    $diagnosticFailure = ($_.Exception.Message -replace
                        '[\x00-\x1F\x7F]+', ' ').Trim()
                    if ($diagnosticFailure.Length -gt 384) {
                        $diagnosticFailure = $diagnosticFailure.Substring(0, 384)
                    }
                    $comparison = Format-NamedObjectAccessComparison `
                        -ReceiptType $receiptType `
                        -Receipt $receipt `
                        -Record $null
                    throw "Construction named-object probe child failed. exit_code=$normalExitCode stage=diagnostic-invalid error_type=InvalidProbeRecord native_error_code= message=$diagnosticFailure comparison=$comparison"
                }
            }
            elseif ($normalExitCode -ne 0) {
                $comparison = Format-NamedObjectAccessComparison `
                    -ReceiptType $receiptType `
                    -Receipt $receipt `
                    -Record $null
                throw "Construction named-object probe child failed. exit_code=$normalExitCode stage=diagnostic-unavailable error_type=MissingProbeRecord native_error_code= message=probe-result-missing comparison=$comparison"
            }
            else {
                $comparison = Format-NamedObjectAccessComparison `
                    -ReceiptType $receiptType `
                    -Receipt $receipt `
                    -Record $null
                throw "Construction named-object probe child failed. exit_code=0 stage=diagnostic-unavailable error_type=MissingProbeRecord native_error_code= message=probe-result-missing comparison=$comparison"
            }
            $comparison = Format-NamedObjectAccessComparison `
                -ReceiptType $receiptType `
                -Receipt $receipt `
                -Record $probeResult
            Write-Host "[named-object-access] $comparison"
            if ($normalExitCode -ne 0) {
                if ($probeResult.status -cne 'failure' -or
                    $probeResult.exit_code -ne [long]$normalExitCode -or
                    $null -eq $probeResult.error) {
                    throw "Construction named-object probe child failed. exit_code=$normalExitCode stage=diagnostic-invalid error_type=InconsistentProbeRecord native_error_code= message=probe-result-exit-mismatch comparison=$comparison"
                }
                $probeFailure = Format-NamedObjectProbeFailure `
                    -Record $probeResult `
                    -ProcessExitCode $normalExitCode
                throw "$probeFailure comparison=$comparison"
            }
            Require `
                ([ProjectAtlasConstructionProcess]::LastTotalProcesses -ge 2) `
                "Construction normal admission did not contain its child and descendant canary."
            Assert-AdmissionReceipt `
                -ReceiptType $receiptType `
                -Receipt $receipt `
                -ExpectTermination $false
            $seededSemaphoreName = [string](Get-ReflectedReceiptValue `
                -ReceiptType $receiptType `
                -Receipt $receipt `
                -Name SeededSemaphoreName)
            Require `
                ((Get-ReflectedReceiptValue `
                        -ReceiptType $receiptType `
                        -Receipt $receipt `
                        -Name SeededSemaphoreCreatedNew) -eq $true -and
                    (Get-ReflectedReceiptValue `
                        -ReceiptType $receiptType `
                        -Receipt $receipt `
                        -Name SeededSemaphoreDuplicated) -eq $true -and
                    (Get-ReflectedReceiptValue `
                        -ReceiptType $receiptType `
                        -Receipt $receipt `
                        -Name SeededSemaphoreParentHandleClosed) -eq $true -and
                    $seededSemaphoreName -match
                        '\AProjectAtlasParserPack-[0-9a-f]{32}\z' -and
                    [string]::Equals(
                        $seededSemaphoreName,
                        [string]$probeResult.seeded_semaphore_name,
                        [System.StringComparison]::Ordinal
                    )) `
                "Construction seeded semaphore transfer failed. comparison=$comparison"
            $logonNamespace = Assert-TokenNamespaceSnapshot -Snapshot (
                Get-ReflectedReceiptValue `
                    -ReceiptType $receiptType `
                    -Receipt $receipt `
                    -Name LogonTokenNamespace
            )
            $childNamespaceBefore = Assert-TokenNamespaceSnapshot -Snapshot (
                Get-ReflectedReceiptValue `
                    -ReceiptType $receiptType `
                    -Receipt $receipt `
                    -Name ChildTokenNamespaceBeforeJob
            )
            $childNamespaceAfter = Assert-TokenNamespaceSnapshot -Snapshot (
                Get-ReflectedReceiptValue `
                    -ReceiptType $receiptType `
                    -Receipt $receipt `
                    -Name ChildTokenNamespaceAfterJob
            )
            foreach ($namespaceField in $childNamespaceBefore.Keys) {
                Require `
                    ($childNamespaceBefore[$namespaceField] -ceq
                        $childNamespaceAfter[$namespaceField]) `
                    "Construction Job changed token namespace field $namespaceField."
            }
            Invoke-ExactSidProcessAudit -Sid $identity.Sid -Expectation absent
            $expectedDirectoryPath = if ($probeResult.session_id -eq 0L) {
                '\BaseNamedObjects'
            }
            else {
                "\Sessions\$($probeResult.session_id)\BaseNamedObjects"
            }
            Require `
                ($probeResult.schema_version -eq 5L -and
                    $probeResult.status -ceq 'success' -and
                    $probeResult.stage -ceq 'complete' -and
                    $probeResult.exit_code -eq 0L -and
                    $probeResult.session_id -ge 0L -and
                     [string]::Equals(
                        $probeResult.directory_path,
                        $expectedDirectoryPath,
                        [System.StringComparison]::Ordinal
                    ) -and
                    [string]::Equals(
                        $probeResult.directory_path,
                        $objectDirectory,
                        [System.StringComparison]::Ordinal
                    ) -and
                    $probeResult.native_semaphore_name -match
                        '\AProjectAtlasParserPack-[0-9a-f]{32}\z' -and
                    $probeResult.session_directory_traverse_ntstatus -eq 0L -and
                    $probeResult.seeded_direct_open_ntstatus -eq 0L -and
                    $probeResult.seeded_direct_open_close_ntstatus -eq 0L -and
                    (Test-DefaultSecuritySemaphoreProbe `
                        -CreateWin32 $probeResult.post_job_native_create_win32 `
                        -CreatedNew $probeResult.post_job_native_created_new `
                        -CloseWin32 $probeResult.post_job_native_close_win32) -and
                    $probeResult.seeded_open_win32 -eq 0L -and
                    $probeResult.seeded_open_close_win32 -eq 0L -and
                    $probeResult.seeded_create_win32 -eq 183L -and
                    $probeResult.seeded_create_created_new -eq $false -and
                    $probeResult.seeded_create_close_win32 -eq 0L -and
                    $probeResult.created_new -eq $false -and
                    $probeResult.descendant_exit_code -eq 0L -and
                    $probeResult.semaphore_name -match
                        '\AProjectAtlasParserPack-[0-9a-f]{32}\z') `
                "Construction named-object probe result identity was invalid. comparison=$comparison"
            $survivingSemaphore = $null
            $semaphoreAbsent = -not [System.Threading.SemaphoreAcl]::TryOpenExisting(
                $probeResult.semaphore_name,
                [System.Security.AccessControl.SemaphoreRights]::Synchronize -bor
                    [System.Security.AccessControl.SemaphoreRights]::Modify,
                [ref]$survivingSemaphore
            )
            if ($null -ne $survivingSemaphore) {
                $survivingSemaphore.Dispose()
            }
            Require $semaphoreAbsent "Construction named-object probe left a semaphore handle survivor."
        }

        $failure = Get-ReflectedOperationFailure `
            -Method $runCore `
            -Arguments $invokeArguments
        if ($scenarioName -eq 'RetainedJobBeforeAdmission') {
            Require `
                ($failure -is [System.InvalidOperationException] -and
                    $failure.Message -eq 'construction-process-retained-inherited-job') `
                "Construction retained-Job admission failure returned the wrong operation error."
        }
        elseif ($scenarioName -eq 'FailBeforeJobAssignment') {
            Require `
                ($failure -is [System.InvalidOperationException] -and
                    $failure.Message -eq 'construction-self-test-before-job-assignment') `
                "Construction pre-Job failure returned the wrong operation error."
        }
        else {
            Require `
                ($failure -is [System.AggregateException] -and
                    $failure.InnerExceptions.Count -eq 2 -and
                    $failure.InnerExceptions[0].Message -eq
                        'construction-self-test-before-job-assignment' -and
                    $failure.InnerExceptions[1].Message -eq
                        'construction-self-test-cleanup') `
                "Construction adapter did not preserve operation and cleanup failures."
        }
        Assert-AdmissionReceipt -ReceiptType $receiptType -Receipt $receipt
        $failedSeedName = [string](Get-ReflectedReceiptValue `
            -ReceiptType $receiptType `
            -Receipt $receipt `
            -Name SeededSemaphoreName)
        $failedSeed = $null
        $failedSeedAbsent = -not [System.Threading.SemaphoreAcl]::TryOpenExisting(
            $failedSeedName,
            [System.Security.AccessControl.SemaphoreRights]::Synchronize -bor
                [System.Security.AccessControl.SemaphoreRights]::Modify,
            [ref]$failedSeed
        )
        if ($null -ne $failedSeed) { $failedSeed.Dispose() }
        Require $failedSeedAbsent "Construction admission failure left its seeded semaphore."
        Invoke-ExactSidProcessAudit -Sid $identity.Sid -Expectation absent
    }
    $launcherExit = Invoke-BoundedProcess `
        -FilePath $LauncherPath `
        -Arguments @('self-test') `
        -TimeoutSeconds 180
    Require `
        ($launcherExit -eq 0) `
        "Shipped AppContainer launcher recovery self-test failed."

    Invoke-ScenarioCleanup -StatePath $StatePath
    Assert-ScenarioAbsent `
        -Username $identity.Username `
        -Sid $identity.Sid `
        -FirewallRule $identity.FirewallRule `
        -StatePath $StatePath `
        -ObjectDirectory $objectDirectory
}

$profileLaunchSource = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security;
using System.Text;

public sealed class ProjectAtlasProfileLaunchReceipt
{
    public int ProcessId { get; set; }
    public bool ProcessHandleClosed { get; set; }
    public bool ThreadHandleClosed { get; set; }
}

public static class ProjectAtlasProfileLaunch
{
    private const uint LogonWithProfile = 0x00000001;
    private const uint CreateNoWindow = 0x08000000;
    private const uint CreateUnicodeEnvironment = 0x00000400;

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct StartupInfo
    {
        public int Size;
        public string Reserved;
        public string Desktop;
        public string Title;
        public uint X;
        public uint Y;
        public uint XSize;
        public uint YSize;
        public uint XCountChars;
        public uint FillAttribute;
        public uint Flags;
        public ushort ShowWindow;
        public ushort Reserved2;
        public IntPtr Reserved2Pointer;
        public IntPtr StandardInput;
        public IntPtr StandardOutput;
        public IntPtr StandardError;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessInformation
    {
        public IntPtr Process;
        public IntPtr Thread;
        public uint ProcessId;
        public uint ThreadId;
    }

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CreateProcessWithLogonW(
        string username,
        string domain,
        IntPtr password,
        uint logonFlags,
        string applicationName,
        StringBuilder commandLine,
        uint creationFlags,
        IntPtr environment,
        string currentDirectory,
        ref StartupInfo startupInfo,
        out ProcessInformation processInformation);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    public static ProjectAtlasProfileLaunchReceipt Start(
        string username,
        SecureString password,
        string executable,
        string workingDirectory)
    {
        StartupInfo startup = new StartupInfo();
        startup.Size = Marshal.SizeOf<StartupInfo>();
        ProcessInformation process = new ProcessInformation();
        IntPtr passwordPointer = IntPtr.Zero;
        bool created = false;
        int createError = 0;
        try
        {
            passwordPointer = Marshal.SecureStringToGlobalAllocUnicode(password);
            created = CreateProcessWithLogonW(
                username,
                ".",
                passwordPointer,
                LogonWithProfile,
                executable,
                new StringBuilder(
                    "\"" + executable + "\" -NoLogo -NoProfile -NonInteractive " +
                    "-Command \"Start-Sleep -Seconds 120\""),
                CreateNoWindow | CreateUnicodeEnvironment,
                IntPtr.Zero,
                workingDirectory,
                ref startup,
                out process);
            if (!created)
            {
                createError = Marshal.GetLastWin32Error();
            }
        }
        finally
        {
            if (passwordPointer != IntPtr.Zero)
            {
                Marshal.ZeroFreeGlobalAllocUnicode(passwordPointer);
            }
        }
        if (!created)
        {
            throw new Win32Exception(createError, "create-profile-backed-process");
        }

        bool threadClosed = CloseHandle(process.Thread);
        int threadCloseError = threadClosed ? 0 : Marshal.GetLastWin32Error();
        bool processClosed = CloseHandle(process.Process);
        int processCloseError = processClosed ? 0 : Marshal.GetLastWin32Error();
        if (!threadClosed)
        {
            throw new Win32Exception(threadCloseError, "close-profile-backed-thread");
        }
        if (!processClosed)
        {
            throw new Win32Exception(processCloseError, "close-profile-backed-process");
        }
        return new ProjectAtlasProfileLaunchReceipt
        {
            ProcessId = checked((int)process.ProcessId),
            ProcessHandleClosed = processClosed,
            ThreadHandleClosed = threadClosed
        };
    }
}
'@

function Initialize-ProfileLaunchFixture {
    if (-not ('ProjectAtlasProfileLaunch' -as [type])) {
        Add-Type -TypeDefinition $profileLaunchSource -Language CSharp
    }
}

function Wait-ForRecoveryCondition {
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Condition,

        [Parameter(Mandatory = $true)]
        [ValidateRange(1, 60)]
        [int]$TimeoutSeconds,

        [Parameter(Mandatory = $true)]
        [string]$FailureMessage
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        if (& $Condition) {
            return
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    throw $FailureMessage
}

function Invoke-CleanupRetryRecoveryScenario {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StatePath
    )

    $scenario = New-ScenarioAccount -StatePath $StatePath
    $identity = $scenario.Identity
    $state = $scenario.State
    $sid = [System.Security.Principal.SecurityIdentifier]::new($identity.Sid)
    $objectDirectory = Add-ScenarioObjectDirectoryAccess `
        -StatePath $StatePath `
        -State $state `
        -Sid $sid
    $fixtureRoot = Join-Path (Split-Path -Parent (Split-Path -Parent $StatePath)) 'acl-fixture'
    [System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
    $state.acl_paths = @($fixtureRoot)
    $state.stage = 'construction'
    Write-ScenarioState -StatePath $StatePath -State $state

    $acl = Get-Acl -LiteralPath $fixtureRoot
    $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
        $sid,
        [System.Security.AccessControl.FileSystemRights]::Modify,
        [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
            [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
        [System.Security.AccessControl.PropagationFlags]::None,
        [System.Security.AccessControl.AccessControlType]::Allow
    ))
    Set-Acl -LiteralPath $fixtureRoot -AclObject $acl
    Require `
        (@(Get-ExactAclRules -Path $fixtureRoot -Sid $sid).Count -gt 0) `
        "Cleanup retry fixture did not retain its exact-SID ACL."

    $rawDescriptor = [System.Security.AccessControl.RawSecurityDescriptor]::new(
        "D:(A;;CC;;;$($identity.Sid))"
    )
    $principalSddl = $rawDescriptor.GetSddlForm(
        [System.Security.AccessControl.AccessControlSections]::Access
    )
    New-NetFirewallRule `
        -PolicyStore PersistentStore `
        -Name $identity.FirewallRule `
        -DisplayName "ProjectAtlas optional parser recovery" `
        -Description "Ephemeral exact-principal recovery proof" `
        -Direction Outbound `
        -Action Block `
        -Enabled True `
        -Profile Any `
        -Protocol Any `
        -LocalAddress Any `
        -RemoteAddress Any `
        -LocalUser $principalSddl `
        -ErrorAction Stop | Out-Null
    Wait-ForRecoveryCondition `
        -TimeoutSeconds 10 `
        -FailureMessage "Cleanup retry firewall rule did not become active." `
        -Condition {
            @(Get-ExactFirewallRules `
                -Name $identity.FirewallRule `
                -PolicyStore PersistentStore).Count -eq 1 -and
                @(Get-ExactFirewallRules `
                    -Name $identity.FirewallRule `
                    -PolicyStore ActiveStore).Count -eq 1
        }

    Initialize-ProfileLaunchFixture
    $launchReceipt = [ProjectAtlasProfileLaunch]::Start(
        $identity.Username,
        $identity.Password,
        [string]$ConstructionParameters.PwshPath,
        $env:SystemRoot
    )
    Require `
        ($launchReceipt.ProcessId -gt 0 -and
            $launchReceipt.ProcessHandleClosed -and
            $launchReceipt.ThreadHandleClosed) `
        "Profile-backed process launcher did not close its native handles."
    Wait-ForRecoveryCondition `
        -TimeoutSeconds 15 `
        -FailureMessage "Profile-backed exact-SID process did not become observable." `
        -Condition {
            $null -ne (Get-Process `
                -Id $launchReceipt.ProcessId `
                -ErrorAction SilentlyContinue) -and
                @(Get-ExactProfile -Sid $identity.Sid).Count -eq 1
        }
    Invoke-ExactSidProcessAudit -Sid $identity.Sid -Expectation present

    $checkpointFailure = $null
    try {
        Invoke-ScenarioCleanup `
            -StatePath $StatePath `
            -AfterProcessTermination { throw 'cleanup-retry-checkpoint' }
    }
    catch {
        $checkpointFailure = $_.Exception.Message
    }
    Require `
        ($checkpointFailure -eq 'cleanup-retry-checkpoint') `
        "Cleanup retry scenario did not fail at the exact checkpoint."
    Require `
        ($null -eq (Get-Process `
            -Id $launchReceipt.ProcessId `
            -ErrorAction SilentlyContinue)) `
        "Cleanup retry checkpoint left its exact process alive."
    Invoke-ExactSidProcessAudit -Sid $identity.Sid -Expectation absent
    Wait-ForRecoveryCondition `
        -TimeoutSeconds 10 `
        -FailureMessage "Profile-backed process did not release its loaded profile." `
        -Condition {
            $profiles = @(Get-ExactProfile -Sid $identity.Sid)
            $profiles.Count -eq 1 -and -not [bool]$profiles[0].Loaded
        }
    $retainedState = Read-ScenarioState -StatePath $StatePath
    $retainedAccount = Get-ExactLocalAccount `
        -Username $identity.Username `
        -Sid $identity.Sid
    Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
        Assert-ConstructionObjectDirectoryPrincipalAccess `
            -Path $objectDirectory `
            -Sid $sid
    }
    Require `
        ($null -ne $retainedState -and
            [string]$retainedState.sid -eq $identity.Sid -and
            $null -ne $retainedAccount -and
            [string]$retainedAccount.Description -eq $accountDescription -and
            @(Get-ExactProfile -Sid $identity.Sid).Count -eq 1 -and
            @(Get-ExactFirewallRules -Name $identity.FirewallRule -PolicyStore PersistentStore).Count -eq 1 -and
            @(Get-ExactFirewallRules -Name $identity.FirewallRule -PolicyStore ActiveStore).Count -eq 1 -and
            @(Get-ExactAclRules -Path $fixtureRoot -Sid $sid).Count -gt 0) `
        "Cleanup retry checkpoint did not retain every durable recovery artifact."

    $accountRemovalFailure = $null
    try {
        Invoke-ScenarioCleanup `
            -StatePath $StatePath `
            -AfterAccountRemoval { throw 'account-removal-crash-checkpoint' }
    }
    catch {
        $accountRemovalFailure = $_.Exception.Message
    }
    Require `
        ($accountRemovalFailure -eq 'account-removal-crash-checkpoint') `
        "Cleanup retry scenario did not reach its post-account-removal checkpoint."
    $accountRemovalState = Read-ScenarioState -StatePath $StatePath
    Require `
        ($null -ne $accountRemovalState -and
            [string]$accountRemovalState.sid -eq $identity.Sid -and
            [string]$accountRemovalState.stage -eq 'processes_absent' -and
            $null -eq (Get-ExactLocalAccount `
                -Username $identity.Username `
                -Sid $identity.Sid) -and
            @(Get-ExactProfile -Sid $identity.Sid).Count -eq 0 -and
            @(Get-ExactFirewallRules `
                -Name $identity.FirewallRule `
                -PolicyStore PersistentStore).Count -eq 1 -and
            @(Get-ExactFirewallRules `
                -Name $identity.FirewallRule `
                -PolicyStore ActiveStore).Count -eq 1 -and
            @(Get-ExactAclRules -Path $fixtureRoot -Sid $sid).Count -eq 0) `
        "Post-account-removal crash did not retain only retry-safe durable state."
    Invoke-ExactSidProcessAudit -Sid $identity.Sid -Expectation absent
    Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
        Assert-ConstructionObjectDirectoryPrincipalAbsent `
            -Path $objectDirectory `
            -Sid $sid
    }

    $corruptState = @{
        schema_version = [int]$accountRemovalState.schema_version
        username = [string]$accountRemovalState.username
        sid = [string]$accountRemovalState.sid
        firewall_rule = [string]$accountRemovalState.firewall_rule
        object_directory = [string]$accountRemovalState.object_directory
        acl_paths = @($accountRemovalState.acl_paths)
        stage = 'corrupt'
    }
    $corruptTestFailure = $null
    $restoreStateFailure = $null
    try {
        try {
            Write-ScenarioState -StatePath $StatePath -State $corruptState
            $corruptStateFailure = $null
            try {
                Invoke-ScenarioCleanup -StatePath $StatePath
            }
            catch {
                $corruptStateFailure = $_.Exception.Message
            }
            Require `
                ($corruptStateFailure -eq
                    'Construction cleanup state contains invalid values.') `
                "Missing-account retry did not reject corrupted process-absence state."
        }
        catch {
            $corruptTestFailure = $_.Exception
        }
    }
    finally {
        try {
            $corruptState.stage = [string]$accountRemovalState.stage
            Write-ScenarioState `
                -StatePath $StatePath `
                -State $corruptState
        }
        catch {
            $restoreStateFailure = $_.Exception
        }
    }
    if ($null -ne $corruptTestFailure -and $null -ne $restoreStateFailure) {
        throw [System.AggregateException]::new(
            "Corrupted-state proof and valid-state restoration failed.",
            @($corruptTestFailure, $restoreStateFailure)
        )
    }
    if ($null -ne $corruptTestFailure) {
        throw $corruptTestFailure
    }
    if ($null -ne $restoreStateFailure) {
        throw $restoreStateFailure
    }

    Invoke-ScenarioCleanup -StatePath $StatePath
    Assert-ScenarioAbsent `
        -Username $identity.Username `
        -Sid $identity.Sid `
        -FirewallRule $identity.FirewallRule `
        -StatePath $StatePath `
        -AclPaths @($fixtureRoot) `
        -ObjectDirectory $objectDirectory

    $noOpTimer = [System.Diagnostics.Stopwatch]::StartNew()
    Invoke-ScenarioCleanup -StatePath $StatePath
    $noOpTimer.Stop()
    Require `
        ($noOpTimer.Elapsed.TotalSeconds -lt 5) `
        "Third cleanup was not an immediate no-op."
}

$operationFailure = $null
$cleanupFailures = [System.Collections.Generic.List[System.Exception]]::new()
$suiteSucceeded = $false
try {
    Assert-NamedObjectProbeRecordFixtures
    foreach ($statePath in $scenarioStatePaths.Values) {
        Require `
            (-not (Test-Path -LiteralPath (Split-Path -Parent $statePath))) `
            "Windows parser-pack recovery found stale scenario state."
    }

    $runnerSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    Invoke-ExactSidProcessAudit -Sid $runnerSid -Expectation present
    $auditFailure = $null
    try {
        Invoke-ExactSidProcessAudit -Sid $runnerSid -Expectation absent
    }
    catch {
        $auditFailure = $_.Exception.Message
    }
    Require `
        ($null -ne $auditFailure -and
            $auditFailure.StartsWith(
                'Bounded recovery process failed. exit=1 ',
                [System.StringComparison]::Ordinal
            ) -and
            $auditFailure.Contains(
                'Exact-SID WTS process audit did not satisfy its expected state.',
                [System.StringComparison]::Ordinal
            )) `
        "Exact-SID WTS process audit did not propagate child failure."
    $auditTimeout = $null
    try {
        Invoke-ExactSidProcessAudit `
            -Sid $runnerSid `
            -Expectation present `
            -DelayMilliseconds 30000 `
            -TimeoutSeconds 1
    }
    catch {
        $auditTimeout = $_.Exception.Message
    }
    Require `
        ($null -ne $auditTimeout -and
            $auditTimeout.StartsWith(
                'Recovery process exceeded its fixed deadline. ',
                [System.StringComparison]::Ordinal
            )) `
        "Exact-SID WTS process audit did not enforce its parent deadline."

    Invoke-LegacyJournalCompatibilityScenario `
        -StatePath $scenarioStatePaths.LegacyJournal
    Invoke-AccountJournalRecoveryScenario `
        -StatePath $scenarioStatePaths.AccountJournal
    Invoke-ConstructionAdmissionRecoveryScenario `
        -StatePath $scenarioStatePaths.LauncherAdmission
    Invoke-CleanupRetryRecoveryScenario `
        -StatePath $scenarioStatePaths.CleanupRetry
    $suiteSucceeded = $true
}
catch {
    $operationFailure = $_.Exception
}
finally {
    foreach ($statePath in $scenarioStatePaths.Values) {
        try {
            Invoke-ScenarioCleanup -StatePath $statePath
        }
        catch {
            $cleanupFailures.Add($_.Exception)
        }
    }
    foreach ($password in $recoveryPasswords) {
        try {
            $password.Dispose()
        }
        catch {
            $cleanupFailures.Add($_.Exception)
        }
    }
    $expectedProbeParent = [System.IO.Path]::GetFullPath(
        [System.IO.Path]::Combine($env:SystemRoot, 'Temp')
    ).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    foreach ($probeResultPath in $namespaceProbeResultPaths) {
        try {
            Remove-NamedObjectProbeTemporaryRecords `
                -ResultPath $probeResultPath `
                -ExpectedParent $expectedProbeParent
        }
        catch {
            $cleanupFailures.Add($_.Exception)
        }
    }
    foreach ($probePath in $namespaceProbePaths) {
        try {
            $resolvedProbePath = [System.IO.Path]::GetFullPath($probePath)
            $resolvedProbeParent = [System.IO.Path]::GetDirectoryName(
                $resolvedProbePath
            )
            Require `
                ([string]::Equals(
                    $resolvedProbeParent.TrimEnd(
                        [System.IO.Path]::DirectorySeparatorChar,
                        [System.IO.Path]::AltDirectorySeparatorChar
                    ),
                    $expectedProbeParent,
                    [System.StringComparison]::OrdinalIgnoreCase
                    ) -and
                    [System.IO.Path]::GetFileName($resolvedProbePath) -match
                        '\Aprojectatlas-object-namespace-(?:probe|canary)-[0-9a-f]{32}\.(?:ps1|json)\z') `
                "Refused to remove an unsafe named-object probe path."
            if ([System.IO.File]::Exists($resolvedProbePath)) {
                $probeItem = Get-Item -LiteralPath $resolvedProbePath -Force
                Require `
                    (-not $probeItem.PSIsContainer -and
                        (($probeItem.Attributes -band
                            [System.IO.FileAttributes]::ReparsePoint) -eq 0)) `
                    "Refused to remove an unsafe named-object probe file."
                Remove-Item -LiteralPath $resolvedProbePath -Force
            }
            Require `
                (-not [System.IO.File]::Exists($resolvedProbePath) -and
                    -not [System.IO.Directory]::Exists($resolvedProbePath)) `
                "Named-object probe file survived suite cleanup."
        }
        catch {
            $cleanupFailures.Add($_.Exception)
        }
    }
    try {
        if ([System.IO.Directory]::Exists($exactSidAuditDirectory)) {
            $resolvedAuditDirectory = [System.IO.Path]::GetFullPath(
                $exactSidAuditDirectory
            )
            $auditDirectoryItem = Get-Item `
                -LiteralPath $resolvedAuditDirectory `
                -Force
            Require `
                ($resolvedAuditDirectory.StartsWith(
                    "$RecoveryRoot$([System.IO.Path]::DirectorySeparatorChar)",
                    [System.StringComparison]::OrdinalIgnoreCase
                ) -and
                    $auditDirectoryItem.PSIsContainer -and
                    (($auditDirectoryItem.Attributes -band
                        [System.IO.FileAttributes]::ReparsePoint) -eq 0)) `
                "Refused to remove an unsafe exact-SID audit helper directory."
            Remove-Item -LiteralPath $resolvedAuditDirectory -Recurse -Force
        }
        Require `
            (-not [System.IO.Directory]::Exists($exactSidAuditDirectory) -and
                -not [System.IO.File]::Exists($exactSidAuditPath)) `
            "Exact-SID WTS audit helper survived suite cleanup."
    }
    catch {
        $cleanupFailures.Add($_.Exception)
    }
    if ($suiteSucceeded -and $cleanupFailures.Count -eq 0 -and
        [System.IO.Directory]::Exists($RecoveryRoot)) {
        try {
            $resolvedRecoveryRoot = [System.IO.Path]::GetFullPath($RecoveryRoot)
            Require `
                ($resolvedRecoveryRoot.StartsWith(
                    "$runnerTemp$([System.IO.Path]::DirectorySeparatorChar)",
                    [System.StringComparison]::OrdinalIgnoreCase
                )) `
                "Refused to remove a recovery root outside RUNNER_TEMP."
            Remove-Item -LiteralPath $resolvedRecoveryRoot -Recurse -Force
        }
        catch {
            $cleanupFailures.Add($_.Exception)
        }
    }
}

if ($null -ne $operationFailure) {
    if ($cleanupFailures.Count -ne 0) {
        throw [System.AggregateException]::new(
            "Windows parser-pack recovery operation and fallback cleanup failed.",
            @($operationFailure) + @($cleanupFailures)
        )
    }
    throw $operationFailure
}
if ($cleanupFailures.Count -ne 0) {
    throw [System.AggregateException]::new(
        "Windows parser-pack recovery fallback cleanup failed.",
        @($cleanupFailures)
    )
}
Write-Output "Windows parser-pack recovery suite passed."
