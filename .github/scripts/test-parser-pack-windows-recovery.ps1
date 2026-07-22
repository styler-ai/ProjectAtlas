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
        'ValidateConstructionToken(logonToken, principalSid);',
        [System.StringComparison]::Ordinal
    )
    $processCreationIndex = $nativeText.IndexOf(
        'created = CreateProcessWithToken(',
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
    $tokenValidationIndex = $nativeText.IndexOf(
        'ValidateConstructionToken(constructionToken, principalSid);',
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
    Require `
        ($creationFlagsIndex -ge 0 -and
            $principalLogonIndex -gt $creationFlagsIndex -and
            $principalTokenValidationIndex -gt $principalLogonIndex -and
            $processCreationIndex -gt $principalTokenValidationIndex -and
            $processCreatedIndex -gt $processCreationIndex -and
            $processTokenOpenIndex -gt $processCreatedIndex -and
            $tokenValidationIndex -gt $processTokenOpenIndex -and
            $retainedJobInjectionIndex -gt $tokenValidationIndex -and
            $inheritedJobCheckIndex -gt $retainedJobInjectionIndex -and
            $ownJobAssignmentIndex -gt $inheritedJobCheckIndex -and
            $nativeText.Contains('return CreateSuspended | CreateNoWindow | CreateUnicodeEnvironment;') -and
            $nativeText.Contains('EntryPoint = "LogonUserW"') -and
            $nativeText.Contains('EntryPoint = "CreateProcessWithTokenW"') -and
            -not $nativeText.Contains('EntryPoint = "CreateProcessWithLogonW"') -and
            -not $nativeText.Contains('CreateBreakawayFromJob = 0x01000000;') -and
            $nativeText.Contains('ValidateCurrentBrokerJob(brokerJobName);') -and
            $nativeText.Contains('limits.BasicLimitInformation.LimitFlags != expectedFlags') -and
            $nativeText.Contains('JobObjectLimitKillOnJobClose | JobObjectLimitBreakawayOk') -and
            $nativeText.Contains('construction-broker-job-required') -and
            $nativeText.Contains('construction-broker-job-membership') -and
            $nativeText.Contains('construction-broker-job-policy') -and
            $nativeText.Contains('Marshal.ZeroFreeGlobalAllocUnicode(passwordPointer);') -and
            $nativeText.Contains('LogonTokenHandleOwned') -and
            $nativeText.Contains('LogonTokenHandleClosed') -and
            $nativeText.Contains('MaximumLogonCommandLineCharacters = 1023;') -and
            $nativeText.Contains('construction-process-retained-inherited-job')) `
        "Construction admission no longer validates the suspended alternate-logon child before assigning its owned Job."

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
    Require `
        ($zeroProcessIndex -ge 0 -and
            $processAbsenceIndex -gt $zeroProcessIndex -and
            $checkpointIndex -gt $processAbsenceIndex -and
            $durableCleanupIndex -gt $checkpointIndex -and
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

$productionAst = Get-ProductionWrapperAst
$nativeSourceAssignment = Assert-ProductionRecoveryContracts -Ast $productionAst
Assert-AccountJournalConstructionContract -Ast $productionAst
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

        [string[]]$AclPaths = @()
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
        schema_version = 1
        username = $identity.Username
        sid = $placeholderSid
        firewall_rule = $identity.FirewallRule
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
    if ([string]$proxyJournal.username -cne $Name -or
        [string]$proxyJournal.sid -ne $proxyPlaceholderSid -or
        [string]$proxyJournal.firewall_rule -notmatch
            '\AProjectAtlas-ParserPack-Construction-[0-9a-f]{12}\z' -or
        [string]$proxyJournal.stage -ne 'identity' -or
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
    $normalArguments = [string[]]@(
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
        $invalidArguments[4] = $normalArguments
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
                throw "Construction normal admission invocation failed. type=$($normalFailure.GetType().Name) native_error_code=$nativeError message=$normalMessage"
            }
            Require ($normalExitCode -eq 0) "Construction normal admission child failed."
            Require `
                ([ProjectAtlasConstructionProcess]::LastTotalProcesses -ge 1) `
                "Construction normal admission did not contain its child."
            Assert-AdmissionReceipt `
                -ReceiptType $receiptType `
                -Receipt $receipt `
                -ExpectTermination $false
            Invoke-ExactSidProcessAudit -Sid $identity.Sid -Expectation absent
            continue
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
        -StatePath $StatePath
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

    $corruptState = @{
        schema_version = [int]$accountRemovalState.schema_version
        username = [string]$accountRemovalState.username
        sid = [string]$accountRemovalState.sid
        firewall_rule = [string]$accountRemovalState.firewall_rule
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
        -AclPaths @($fixtureRoot)

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
