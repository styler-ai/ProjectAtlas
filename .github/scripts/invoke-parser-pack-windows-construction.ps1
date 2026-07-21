[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("construct", "cleanup")]
    [string]$Mode,

    [Parameter(Mandatory = $true)]
    [string]$StatePath,

    [string]$SourceRoot,
    [string]$InputDirectory,
    [string]$VendorDirectory,
    [string]$OutputDirectory,
    [string]$CargoHome,
    [string]$TemporaryDirectory,
    [string]$HomeDirectory,
    [string]$ToolchainRoot,
    [string]$PwshPath,
    [string]$VcToolsRoot,
    [string]$WindowsSdkRoot,
    [string]$ProjectAtlasRevision,
    [string]$CargoPackageVersion,
    [string]$IntendedReleaseVersion,
    [string]$CargoLockSha256,
    [string]$RustcRelease,
    [string]$RustcCommitHash,
    [string]$ResolverAddress,

    [ValidateRange(60, 7200)]
    [int]$TimeoutSeconds = 3600
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$stateSchemaVersion = 1
$usernamePattern = '\Apa[0-9a-f]{12}\z'
$ruleNamePattern = '\AProjectAtlas-ParserPack-Construction-[0-9a-f]{12}\z'
$sidPattern = '\AS-1-5-21-[0-9]+-[0-9]+-[0-9]+-[0-9]+\z'
$placeholderSid = "S-1-5-21-0-0-0-0"
$expectedIsolation = "windows-principal-firewall"
$secretEnvironmentPattern = '(?i)(^GITHUB_|^ACTIONS_|^RUNNER_|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL|COOKIE|AUTH|API_KEY|PRIVATE_KEY|PROXY)'

$StatePath = [System.IO.Path]::GetFullPath($StatePath)
$stateDirectory = Split-Path -Parent $StatePath
if ((Split-Path -Leaf $stateDirectory) -ne "parser-pack-windows-construction-state" -or
    (Split-Path -Leaf $StatePath) -ne "state.json") {
    throw "Construction state must use its dedicated state directory and file name."
}

function Get-CanonicalDirectory {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [string]$Role
    )

    $item = Get-Item -LiteralPath ([System.IO.Path]::GetFullPath($Path)) -Force
    if (-not $item.PSIsContainer -or
        (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "$Role must be one existing non-reparse directory."
    }
    return $item.FullName
}

function Get-RegularFile {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [string]$Role
    )

    $item = Get-Item -LiteralPath ([System.IO.Path]::GetFullPath($Path)) -Force
    if ($item.PSIsContainer -or
        (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) -or
        $item.Length -le 0) {
        throw "$Role must be one non-empty regular file."
    }
    return $item.FullName
}

function Write-ProtectedState {
    param(
        [Parameter(Mandatory = $true)]
        [hashtable]$State
    )

    [System.IO.Directory]::CreateDirectory($stateDirectory) | Out-Null
    $directoryItem = Get-Item -LiteralPath $stateDirectory -Force
    if (-not $directoryItem.PSIsContainer -or
        (($directoryItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "Construction state directory must be one non-reparse directory."
    }
    $json = ($State | ConvertTo-Json -Depth 6 -Compress) + "`n"
    if ([System.Text.Encoding]::UTF8.GetByteCount($json) -gt (64 * 1024)) {
        throw "Construction cleanup state exceeds its byte bound."
    }
    $currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
    $systemSid = [System.Security.Principal.SecurityIdentifier]::new("S-1-5-18")
    $administratorsSid = [System.Security.Principal.SecurityIdentifier]::new("S-1-5-32-544")
    $directoryAcl = [System.Security.AccessControl.DirectorySecurity]::new()
    $directoryAcl.SetAccessRuleProtection($true, $false)
    $directoryAcl.SetOwner($currentSid)
    foreach ($principal in @($currentSid, $systemSid, $administratorsSid)) {
        $directoryAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $principal,
            [System.Security.AccessControl.FileSystemRights]::FullControl,
            [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
                [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [System.Security.AccessControl.PropagationFlags]::None,
            [System.Security.AccessControl.AccessControlType]::Allow
        ))
    }
    Set-Acl -LiteralPath $stateDirectory -AclObject $directoryAcl

    $temporaryState = Join-Path $stateDirectory ".state.json.$([Guid]::NewGuid().ToString('N')).tmp"
    try {
        $stateBytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json)
        $stateStream = [System.IO.FileStream]::new(
            $temporaryState,
            [System.IO.FileMode]::CreateNew,
            [System.IO.FileAccess]::Write,
            [System.IO.FileShare]::None,
            4096,
            [System.IO.FileOptions]::WriteThrough
        )
        try {
            $stateStream.Write($stateBytes, 0, $stateBytes.Length)
            $stateStream.Flush($true)
        }
        finally {
            $stateStream.Dispose()
            [Array]::Clear($stateBytes, 0, $stateBytes.Length)
        }
        $fileAcl = [System.Security.AccessControl.FileSecurity]::new()
        $fileAcl.SetAccessRuleProtection($true, $false)
        $fileAcl.SetOwner($currentSid)
        foreach ($principal in @($currentSid, $systemSid, $administratorsSid)) {
            $fileAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
                $principal,
                [System.Security.AccessControl.FileSystemRights]::FullControl,
                [System.Security.AccessControl.AccessControlType]::Allow
            ))
        }
        Set-Acl -LiteralPath $temporaryState -AclObject $fileAcl
        if (Test-Path -LiteralPath $StatePath -PathType Leaf) {
            # Both paths share the protected state directory, so the overwrite move
            # atomically replaces the journal without a PowerShell-coerced backup path.
            [System.IO.File]::Move($temporaryState, $StatePath, $true)
        }
        else {
            [System.IO.File]::Move($temporaryState, $StatePath)
        }
    }
    finally {
        if (Test-Path -LiteralPath $temporaryState) {
            Remove-Item -LiteralPath $temporaryState -Force
        }
    }
}

function Assert-StateAcl {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $acl = Get-Acl -LiteralPath $Path
    if (-not $acl.AreAccessRulesProtected) {
        throw "Construction state ACL still inherits ambient access."
    }
    $expectedSids = @(
        [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value,
        "S-1-5-18",
        "S-1-5-32-544"
    ) | Sort-Object -Unique
    $rules = @($acl.GetAccessRules(
        $true,
        $false,
        [System.Security.Principal.SecurityIdentifier]
    ))
    if ($rules.Count -ne $expectedSids.Count) {
        throw "Construction state ACL has an unexpected rule count."
    }
    foreach ($rule in $rules) {
        if ($rule.AccessControlType -ne [System.Security.AccessControl.AccessControlType]::Allow -or
            $expectedSids -notcontains $rule.IdentityReference.Value -or
            (([int64]$rule.FileSystemRights -band
                [int64][System.Security.AccessControl.FileSystemRights]::FullControl) -ne
                [int64][System.Security.AccessControl.FileSystemRights]::FullControl)) {
            throw "Construction state ACL contains an unexpected principal or access mask."
        }
    }
}

function Read-CleanupState {
    if (-not (Test-Path -LiteralPath $StatePath -PathType Leaf)) {
        return $null
    }
    $item = Get-Item -LiteralPath $StatePath -Force
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
        $item.Length -le 0 -or
        $item.Length -gt (64 * 1024)) {
        throw "Construction cleanup state is not one bounded regular file."
    }
    $directoryItem = Get-Item -LiteralPath $stateDirectory -Force
    if (-not $directoryItem.PSIsContainer -or
        (($directoryItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "Construction cleanup state directory is unsafe."
    }
    Assert-StateAcl -Path $stateDirectory
    Assert-StateAcl -Path $StatePath
    $state = [System.IO.File]::ReadAllText($item.FullName) | ConvertFrom-Json -Depth 6
    $expectedKeys = @("acl_paths", "firewall_rule", "schema_version", "sid", "stage", "username")
    $actualKeys = @($state.PSObject.Properties.Name | Sort-Object)
    if (Compare-Object -ReferenceObject $expectedKeys -DifferenceObject $actualKeys) {
        throw "Construction cleanup state has an unexpected schema."
    }
    if ($state.schema_version -ne $stateSchemaVersion -or
        [string]$state.username -notmatch $usernamePattern -or
        [string]$state.firewall_rule -notmatch $ruleNamePattern -or
        [string]$state.sid -notmatch $sidPattern -or
        [string]$state.stage -notin @("identity", "filesystem", "network", "construction") -or
        $state.acl_paths -isnot [System.Array] -or
        $state.acl_paths.Count -gt 64) {
        throw "Construction cleanup state contains invalid values."
    }
    return $state
}

function Remove-StateStorage {
    if (-not (Test-Path -LiteralPath $stateDirectory)) {
        return
    }
    $directoryItem = Get-Item -LiteralPath $stateDirectory -Force
    if (-not $directoryItem.PSIsContainer -or
        (($directoryItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "Construction state cleanup found an unsafe state directory."
    }
    foreach ($child in @(Get-ChildItem -LiteralPath $stateDirectory -Force)) {
        if ($child.PSIsContainer -or
            (($child.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) -or
            ($child.FullName -ne $StatePath -and $child.Name -notmatch '\A\.state\.json\.[0-9a-f]{32}\.tmp\z')) {
            throw "Construction state cleanup found an unexpected entry."
        }
        Remove-Item -LiteralPath $child.FullName -Force
    }
    if (@(Get-ChildItem -LiteralPath $stateDirectory -Force).Count -ne 0) {
        throw "Construction state directory did not become empty."
    }
    Remove-Item -LiteralPath $stateDirectory -Force
}

function Find-LocalUserBySid {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Sid
    )

    $matches = @(
        Get-LocalUser -ErrorAction Stop |
            Where-Object { $null -ne $_.Sid -and $_.Sid.Value -eq $Sid }
    )
    if ($matches.Count -gt 1) {
        throw "Local user SID resolved to more than one account."
    }
    if ($matches.Count -eq 0) {
        return $null
    }
    return $matches[0]
}

function Find-LocalUserByName {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $matches = @(Get-LocalUser -ErrorAction Stop | Where-Object { $_.Name -ceq $Name })
    if ($matches.Count -gt 1) {
        throw "Local user name resolved to more than one account."
    }
    if ($matches.Count -eq 0) {
        return $null
    }
    return $matches[0]
}

function Get-PrincipalProcesses {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Sid
    )

    $owned = [System.Collections.Generic.List[object]]::new()
    foreach ($process in @(Get-CimInstance -ClassName Win32_Process -ErrorAction Stop)) {
        try {
            $owner = Invoke-CimMethod -InputObject $process -MethodName GetOwnerSid -ErrorAction Stop
            if ($owner.ReturnValue -eq 0 -and [string]$owner.Sid -eq $Sid) {
                $owned.Add($process)
            }
        }
        catch {
            throw "Could not prove the owner of one running process."
        }
    }
    return @($owned)
}

function Remove-PrincipalAcl {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [System.Security.Principal.SecurityIdentifier]$Sid
    )

    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }
    $item = Get-Item -LiteralPath $Path -Force
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Construction ACL cleanup rejects reparse-point substitution."
    }
    $acl = Get-Acl -LiteralPath $item.FullName
    $rules = @($acl.GetAccessRules(
        $true,
        $false,
        [System.Security.Principal.SecurityIdentifier]
    ) | Where-Object { $_.IdentityReference -eq $Sid })
    foreach ($rule in $rules) {
        $acl.RemoveAccessRuleSpecific($rule)
    }
    Set-Acl -LiteralPath $item.FullName -AclObject $acl
    $remaining = @(
        (Get-Acl -LiteralPath $item.FullName).GetAccessRules(
            $true,
            $false,
            [System.Security.Principal.SecurityIdentifier]
        ) | Where-Object { $_.IdentityReference -eq $Sid }
    )
    if ($remaining.Count -ne 0) {
        throw "Construction principal ACL cleanup could not be verified."
    }
}

function Invoke-Cleanup {
    $state = Read-CleanupState
    if ($null -eq $state) {
        Remove-StateStorage
        return
    }
    $cleanupErrors = [System.Collections.Generic.List[string]]::new()
    if ([string]$state.sid -eq $placeholderSid) {
        try {
            $unboundAccount = Find-LocalUserByName -Name ([string]$state.username)
            if ($null -ne $unboundAccount) {
                if ($null -eq $unboundAccount.Sid -or $unboundAccount.Sid.Value -notmatch $sidPattern) {
                    throw "invalid-created-account-sid"
                }
                if ([string]$unboundAccount.Description -ne "ProjectAtlas optional parser pack construction") {
                    throw "created-account-description-mismatch"
                }
                $state.sid = $unboundAccount.Sid.Value
                Write-ProtectedState -State @{
                    schema_version = [int]$state.schema_version
                    username = [string]$state.username
                    sid = [string]$state.sid
                    firewall_rule = [string]$state.firewall_rule
                    acl_paths = @($state.acl_paths)
                    stage = [string]$state.stage
                }
            }
        }
        catch {
            throw "Construction cleanup could not bind the created account SID; any firewall rule remains active."
        }
    }

    $sid = [System.Security.Principal.SecurityIdentifier]::new([string]$state.sid)
    try {
        $account = Find-LocalUserBySid -Sid $sid.Value
        $namedAccount = Find-LocalUserByName -Name ([string]$state.username)
    }
    catch {
        throw "Construction cleanup could not query the exact account; any firewall rule remains active."
    }
    if ($null -ne $account -and $account.Name -ne [string]$state.username) {
        throw "Construction cleanup account identity changed."
    }
    if ($null -ne $account -and
        [string]$account.Description -ne "ProjectAtlas optional parser pack construction") {
        throw "Construction cleanup account description changed."
    }
    if ($null -ne $namedAccount -and $namedAccount.Sid.Value -ne $sid.Value) {
        throw "Construction cleanup account name was rebound to a different SID."
    }
    if ($null -ne $account) {
        try {
            Disable-LocalUser -SID $sid -ErrorAction Stop
        }
        catch {
            $cleanupErrors.Add("disable-account")
        }
    }

    $zeroProcesses = $false
    try {
        $processDeadline = [DateTime]::UtcNow.AddSeconds(30)
        do {
            $processes = @(Get-PrincipalProcesses -Sid $sid.Value)
            foreach ($process in $processes) {
                try {
                    Stop-Process -Id ([int]$process.ProcessId) -Force -ErrorAction Stop
                }
                catch {
                    $cleanupErrors.Add("terminate-process")
                }
            }
            if ($processes.Count -ne 0) {
                Start-Sleep -Milliseconds 250
            }
        } while ($processes.Count -ne 0 -and [DateTime]::UtcNow -lt $processDeadline)
        $zeroProcesses = @(Get-PrincipalProcesses -Sid $sid.Value).Count -eq 0
        if (-not $zeroProcesses) {
            $cleanupErrors.Add("principal-processes-present")
        }
    }
    catch {
        $cleanupErrors.Add("query-principal-processes")
    }

    foreach ($path in @($state.acl_paths)) {
        try {
            Remove-PrincipalAcl -Path ([string]$path) -Sid $sid
        }
        catch {
            $cleanupErrors.Add("remove-acl")
        }
    }

    try {
        $profiles = @(Get-CimInstance -ClassName Win32_UserProfile -Filter "SID='$($sid.Value)'" -ErrorAction Stop)
        if ($profiles.Count -gt 1) {
            throw "multiple-profiles"
        }
        if ($profiles.Count -eq 1) {
            if ($profiles[0].Loaded) {
                throw "profile-loaded"
            }
            Remove-CimInstance -InputObject $profiles[0] -ErrorAction Stop
        }
        if (@(Get-CimInstance -ClassName Win32_UserProfile -Filter "SID='$($sid.Value)'" -ErrorAction Stop).Count -ne 0) {
            throw "profile-present"
        }
    }
    catch {
        $cleanupErrors.Add("remove-profile")
    }

    $accountAbsent = $false
    try {
        $account = Find-LocalUserBySid -Sid $sid.Value
        if ($null -ne $account) {
            $account | Remove-LocalUser -ErrorAction Stop
        }
        if ($null -ne (Find-LocalUserBySid -Sid $sid.Value) -or
            $null -ne (Find-LocalUserByName -Name ([string]$state.username))) {
            throw "account-present"
        }
        $accountAbsent = $true
    }
    catch {
        $cleanupErrors.Add("remove-account")
    }

    if ($zeroProcesses -and $accountAbsent) {
        try {
            $persistentRules = @(
                Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop |
                    Where-Object { $_.Name -ceq [string]$state.firewall_rule }
            )
            if ($persistentRules.Count -gt 1) {
                throw "duplicate-persistent-rules"
            }
            if ($persistentRules.Count -eq 1) {
                $persistentRules[0] | Remove-NetFirewallRule -ErrorAction Stop
            }
            if (@(
                Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop |
                    Where-Object { $_.Name -ceq [string]$state.firewall_rule }
            ).Count -ne 0 -or @(
                Get-NetFirewallRule -PolicyStore ActiveStore -ErrorAction Stop |
                    Where-Object { $_.Name -ceq [string]$state.firewall_rule }
            ).Count -ne 0) {
                throw "firewall-rule-present"
            }
        }
        catch {
            $cleanupErrors.Add("remove-firewall")
        }
    }
    else {
        $cleanupErrors.Add("retain-firewall")
    }

    if ($cleanupErrors.Count -ne 0) {
        $failures = @($cleanupErrors | Sort-Object -Unique) -join ','
        throw "Construction cleanup failed: $failures."
    }
    Remove-StateStorage
}

if ($Mode -eq "cleanup") {
    Invoke-Cleanup
    exit 0
}

if ($env:OS -ne "Windows_NT" -or -not [Environment]::Is64BitProcess) {
    throw "Windows parser-pack construction requires 64-bit Windows PowerShell."
}
$principal = [System.Security.Principal.WindowsPrincipal]::new(
    [System.Security.Principal.WindowsIdentity]::GetCurrent()
)
if (-not $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Windows parser-pack construction requires an elevated parent."
}
foreach ($requiredCommand in @(
    "New-LocalUser",
    "Get-LocalUser",
    "Add-LocalGroupMember",
    "New-NetFirewallRule",
    "Get-NetFirewallRule",
    "Get-NetFirewallSecurityFilter",
    "Get-NetFirewallInterfaceFilter",
    "Get-NetFirewallInterfaceTypeFilter"
)) {
    if ($null -eq (Get-Command $requiredCommand -ErrorAction SilentlyContinue)) {
        throw "Windows parser-pack construction is missing one required operating-system command."
    }
}
if ((Get-Service -Name MpsSvc -ErrorAction Stop).Status -ne "Running") {
    throw "Windows Firewall is not active for every profile."
}
$profiles = @(Get-NetFirewallProfile -PolicyStore ActiveStore -ErrorAction Stop)
$profileNames = @($profiles | ForEach-Object { [string]$_.Name } | Sort-Object)
if ($profiles.Count -ne 3 -or
    (Compare-Object -ReferenceObject @("Domain", "Private", "Public") -DifferenceObject $profileNames) -or
    @($profiles | Where-Object { [string]$_.Enabled -ne "True" }).Count -ne 0) {
    throw "Windows Firewall does not expose exactly three enabled expected profiles."
}
foreach ($allowRule in @(Get-NetFirewallRule `
    -PolicyStore ActiveStore `
    -Direction Outbound `
    -Action Allow `
    -Enabled True `
    -ErrorAction Stop)) {
    if (@($allowRule | Get-NetFirewallSecurityFilter -ErrorAction Stop | Where-Object {
        [string]$_.OverrideBlockRules -eq "True"
    }).Count -ne 0) {
        throw "An active outbound authenticated bypass rule could override construction denial."
    }
}
if (Test-Path -LiteralPath $stateDirectory) {
    throw "Construction cleanup state directory already exists."
}

$source = Get-CanonicalDirectory -Path $SourceRoot -Role "SourceRoot"
$inputs = Get-CanonicalDirectory -Path $InputDirectory -Role "InputDirectory"
$vendor = Get-CanonicalDirectory -Path $VendorDirectory -Role "VendorDirectory"
$output = Get-CanonicalDirectory -Path $OutputDirectory -Role "OutputDirectory"
$cargo = Get-CanonicalDirectory -Path $CargoHome -Role "CargoHome"
$temporary = Get-CanonicalDirectory -Path $TemporaryDirectory -Role "TemporaryDirectory"
$constructionHome = Get-CanonicalDirectory -Path $HomeDirectory -Role "HomeDirectory"
$toolchain = Get-CanonicalDirectory -Path $ToolchainRoot -Role "ToolchainRoot"
$pwsh = Get-RegularFile -Path $PwshPath -Role "PowerShell runtime"
$pwshRoot = Get-CanonicalDirectory -Path (Split-Path -Parent $pwsh) -Role "PowerShell root"
$vcTools = Get-CanonicalDirectory -Path $VcToolsRoot -Role "Visual C++ tools root"
$windowsSdk = Get-CanonicalDirectory -Path $WindowsSdkRoot -Role "Windows SDK root"
$networkCheck = Get-RegularFile `
    -Path (Join-Path $source ".github/scripts/check-parser-pack-network-boundary.ps1") `
    -Role "network boundary checker"
$constructionScript = Get-RegularFile `
    -Path (Join-Path $source ".github/scripts/run-parser-pack-contained-construction.ps1") `
    -Role "contained construction script"

foreach ($requiredValue in @(
    $ProjectAtlasRevision,
    $CargoPackageVersion,
    $IntendedReleaseVersion,
    $CargoLockSha256,
    $RustcRelease,
    $RustcCommitHash,
    $ResolverAddress
)) {
    if ([string]::IsNullOrWhiteSpace($requiredValue)) {
        throw "Windows parser-pack construction received an empty identity value."
    }
}
foreach ($requiredEnvironment in @("INCLUDE", "LIB", "LIBPATH")) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($requiredEnvironment))) {
        throw "Visual Studio environment is missing $requiredEnvironment."
    }
}

$nativeSource = @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security;
using System.Text;

public static class ProjectAtlasConstructionProcess
{
    private const uint CreateSuspended = 0x00000004;
    private const uint CreateNoWindow = 0x08000000;
    private const uint CreateUnicodeEnvironment = 0x00000400;
    private const uint JobObjectLimitKillOnJobClose = 0x00002000;
    private const int JobObjectExtendedLimitInformation = 9;
    private const int JobObjectBasicAccountingInformation = 1;
    private const uint WaitObject0 = 0;
    private const uint WaitTimeout = 258;
    private const uint WindowStationAllAccess = 0x000F037F;
    private const uint DesktopAllAccess = 0x000F01FF;
    private const uint SddlRevision1 = 1;

    public static uint LastTotalProcesses { get; private set; }

    [StructLayout(LayoutKind.Sequential)]
    private struct SecurityAttributes
    {
        public int Length;
        public IntPtr SecurityDescriptor;
        [MarshalAs(UnmanagedType.Bool)]
        public bool InheritHandle;
    }

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
        public uint YCountChars;
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

    [StructLayout(LayoutKind.Sequential)]
    private struct IoCounters
    {
        public ulong ReadOperationCount;
        public ulong WriteOperationCount;
        public ulong OtherOperationCount;
        public ulong ReadTransferCount;
        public ulong WriteTransferCount;
        public ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct BasicLimitInformation
    {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize;
        public UIntPtr MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ExtendedLimitInformation
    {
        public BasicLimitInformation BasicLimitInformation;
        public IoCounters IoInfo;
        public UIntPtr ProcessMemoryLimit;
        public UIntPtr JobMemoryLimit;
        public UIntPtr PeakProcessMemoryUsed;
        public UIntPtr PeakJobMemoryUsed;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct BasicAccountingInformation
    {
        public long TotalUserTime;
        public long TotalKernelTime;
        public long ThisPeriodTotalUserTime;
        public long ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount;
        public uint TotalProcesses;
        public uint ActiveProcesses;
        public uint TotalTerminatedProcesses;
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

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ConvertStringSecurityDescriptorToSecurityDescriptor(
        string stringSecurityDescriptor,
        uint stringSecurityDescriptorRevision,
        out IntPtr securityDescriptor,
        IntPtr securityDescriptorSize);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr LocalFree(IntPtr memory);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowStation(
        string windowStation,
        uint flags,
        uint desiredAccess,
        ref SecurityAttributes attributes);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseWindowStation(IntPtr windowStation);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr GetProcessWindowStation();

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetProcessWindowStation(IntPtr windowStation);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateDesktop(
        string desktop,
        string device,
        IntPtr deviceMode,
        uint flags,
        uint desiredAccess,
        ref SecurityAttributes attributes);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseDesktop(IntPtr desktop);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObject(IntPtr attributes, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetInformationJobObject(
        IntPtr job,
        int informationClass,
        ref ExtendedLimitInformation information,
        uint informationLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool QueryInformationJobObject(
        IntPtr job,
        int informationClass,
        out BasicAccountingInformation information,
        uint informationLength,
        IntPtr returnLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool TerminateJobObject(IntPtr job, uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool TerminateProcess(IntPtr process, uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint ResumeThread(IntPtr thread);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    public static int Run(
        string username,
        string principalSid,
        SecureString password,
        string executable,
        string[] arguments,
        string workingDirectory,
        string environmentBlock,
        int timeoutSeconds)
    {
        LastTotalProcesses = 0;
        IntPtr job = IntPtr.Zero;
        IntPtr environment = IntPtr.Zero;
        IntPtr securityDescriptor = IntPtr.Zero;
        IntPtr windowStation = IntPtr.Zero;
        IntPtr desktop = IntPtr.Zero;
        IntPtr originalWindowStation = IntPtr.Zero;
        ProcessInformation process = new ProcessInformation();
        bool processCreated = false;
        bool assignedToJob = false;
        bool parentStationChanged = false;
        try
        {
            job = CreateJobObject(IntPtr.Zero, null);
            if (job == IntPtr.Zero)
            {
                throw Failure("create-job");
            }
            ExtendedLimitInformation limits = new ExtendedLimitInformation();
            limits.BasicLimitInformation.LimitFlags = JobObjectLimitKillOnJobClose;
            if (!SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                ref limits,
                (uint)Marshal.SizeOf<ExtendedLimitInformation>()))
            {
                throw Failure("configure-job");
            }

            string objectSddl = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;" + principalSid + ")";
            if (!ConvertStringSecurityDescriptorToSecurityDescriptor(
                objectSddl,
                SddlRevision1,
                out securityDescriptor,
                IntPtr.Zero))
            {
                throw Failure("create-user-object-security");
            }
            SecurityAttributes attributes = new SecurityAttributes();
            attributes.Length = Marshal.SizeOf<SecurityAttributes>();
            attributes.SecurityDescriptor = securityDescriptor;
            attributes.InheritHandle = false;
            string windowStationName = "ProjectAtlasParserPack-" + Guid.NewGuid().ToString("N");
            string desktopName = "Default";
            windowStation = CreateWindowStation(
                windowStationName,
                0,
                WindowStationAllAccess,
                ref attributes);
            if (windowStation == IntPtr.Zero)
            {
                throw Failure("create-window-station");
            }
            originalWindowStation = GetProcessWindowStation();
            if (originalWindowStation == IntPtr.Zero || !SetProcessWindowStation(windowStation))
            {
                throw Failure("select-window-station");
            }
            parentStationChanged = true;
            int desktopError = 0;
            try
            {
                desktop = CreateDesktop(
                    desktopName,
                    null,
                    IntPtr.Zero,
                    0,
                    DesktopAllAccess,
                    ref attributes);
                if (desktop == IntPtr.Zero)
                {
                    desktopError = Marshal.GetLastWin32Error();
                }
            }
            finally
            {
                if (!SetProcessWindowStation(originalWindowStation))
                {
                    throw Failure("restore-window-station");
                }
                parentStationChanged = false;
            }
            if (desktop == IntPtr.Zero)
            {
                throw new Win32Exception(desktopError, "create-desktop");
            }

            environment = Marshal.StringToHGlobalUni(environmentBlock);
            StartupInfo startup = new StartupInfo();
            startup.Size = Marshal.SizeOf<StartupInfo>();
            startup.Desktop = windowStationName + "\\" + desktopName;
            StringBuilder commandLine = new StringBuilder(BuildCommandLine(executable, arguments));
            uint flags = CreateSuspended | CreateNoWindow | CreateUnicodeEnvironment;
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
                    0,
                    executable,
                    commandLine,
                    flags,
                    environment,
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
                throw new Win32Exception(createError, "create-process");
            }
            processCreated = true;
            if (!AssignProcessToJobObject(job, process.Process))
            {
                throw Failure("assign-job");
            }
            assignedToJob = true;
            bool inJob;
            if (!IsProcessInJob(process.Process, job, out inJob) || !inJob)
            {
                throw Failure("verify-job");
            }
            if (ResumeThread(process.Thread) == UInt32.MaxValue)
            {
                throw Failure("resume-process");
            }

            Stopwatch timer = Stopwatch.StartNew();
            uint wait = WaitForSingleObject(process.Process, (uint)checked(timeoutSeconds * 1000));
            if (wait == WaitTimeout)
            {
                TerminateJobObject(job, 124);
                WaitForSingleObject(process.Process, 30000);
                return 124;
            }
            if (wait != WaitObject0)
            {
                throw Failure("wait-process");
            }
            uint exitCode;
            if (!GetExitCodeProcess(process.Process, out exitCode))
            {
                throw Failure("read-exit-code");
            }
            while (true)
            {
                BasicAccountingInformation accounting;
                if (!QueryInformationJobObject(
                    job,
                    JobObjectBasicAccountingInformation,
                    out accounting,
                    (uint)Marshal.SizeOf<BasicAccountingInformation>(),
                    IntPtr.Zero))
                {
                    throw Failure("query-job");
                }
                LastTotalProcesses = accounting.TotalProcesses;
                if (accounting.ActiveProcesses == 0)
                {
                    break;
                }
                if (timer.Elapsed.TotalSeconds >= timeoutSeconds)
                {
                    TerminateJobObject(job, 124);
                    return 124;
                }
                System.Threading.Thread.Sleep(50);
            }
            return unchecked((int)exitCode);
        }
        finally
        {
            if (parentStationChanged && originalWindowStation != IntPtr.Zero)
            {
                SetProcessWindowStation(originalWindowStation);
            }
            if (processCreated && assignedToJob && job != IntPtr.Zero)
            {
                TerminateJobObject(job, 125);
            }
            else if (processCreated && process.Process != IntPtr.Zero)
            {
                TerminateProcess(process.Process, 125);
                WaitForSingleObject(process.Process, 30000);
            }
            if (process.Thread != IntPtr.Zero)
            {
                CloseHandle(process.Thread);
            }
            if (process.Process != IntPtr.Zero)
            {
                CloseHandle(process.Process);
            }
            if (job != IntPtr.Zero)
            {
                CloseHandle(job);
            }
            if (environment != IntPtr.Zero)
            {
                Marshal.FreeHGlobal(environment);
            }
            if (desktop != IntPtr.Zero)
            {
                CloseDesktop(desktop);
            }
            if (windowStation != IntPtr.Zero)
            {
                CloseWindowStation(windowStation);
            }
            if (securityDescriptor != IntPtr.Zero)
            {
                LocalFree(securityDescriptor);
            }
        }
    }

    private static Win32Exception Failure(string operation)
    {
        return new Win32Exception(Marshal.GetLastWin32Error(), operation);
    }

    private static string BuildCommandLine(string executable, string[] arguments)
    {
        StringBuilder command = new StringBuilder(Quote(executable));
        foreach (string argument in arguments)
        {
            command.Append(' ').Append(Quote(argument));
        }
        return command.ToString();
    }

    private static string Quote(string value)
    {
        if (value.Length != 0 && value.IndexOfAny(new[] { ' ', '\t', '\n', '\v', '"' }) < 0)
        {
            return value;
        }
        StringBuilder result = new StringBuilder("\"");
        int slashes = 0;
        foreach (char character in value)
        {
            if (character == '\\')
            {
                slashes++;
                continue;
            }
            if (character == '"')
            {
                result.Append('\\', slashes * 2 + 1).Append('"');
                slashes = 0;
                continue;
            }
            result.Append('\\', slashes).Append(character);
            slashes = 0;
        }
        result.Append('\\', slashes * 2).Append('"');
        return result.ToString();
    }
}
'@
Add-Type -TypeDefinition $nativeSource -Language CSharp

function Add-PrincipalAcl {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [System.Security.Principal.SecurityIdentifier]$Sid,

        [Parameter(Mandatory = $true)]
        [System.Security.AccessControl.FileSystemRights]$Rights
    )

    $item = Get-Item -LiteralPath $Path -Force
    $acl = Get-Acl -LiteralPath $item.FullName
    if ($item.PSIsContainer) {
        $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
            $Sid,
            $Rights,
            [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
                [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [System.Security.AccessControl.PropagationFlags]::None,
            [System.Security.AccessControl.AccessControlType]::Allow
        )
    }
    else {
        $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
            $Sid,
            $Rights,
            [System.Security.AccessControl.AccessControlType]::Allow
        )
    }
    $acl.AddAccessRule($rule)
    Set-Acl -LiteralPath $item.FullName -AclObject $acl
}

function New-EnvironmentBlock {
    param(
        [Parameter(Mandatory = $true)]
        [hashtable]$Values
    )

    if ($Values.Count -gt 64) {
        throw "Construction environment has too many entries."
    }
    $rows = [System.Collections.Generic.List[string]]::new()
    foreach ($entry in @($Values.GetEnumerator() | Sort-Object Key)) {
        if ([string]$entry.Key -match $secretEnvironmentPattern -or
            [string]$entry.Key -notmatch '\A[A-Za-z_][A-Za-z0-9_()]*\z' -or
            [string]$entry.Value -match "`0|`r|`n") {
            throw "Construction environment contains one forbidden name or value."
        }
        $rows.Add("$($entry.Key)=$($entry.Value)")
    }
    $block = ($rows -join "`0") + "`0`0"
    if ([System.Text.Encoding]::Unicode.GetByteCount($block) -gt (64 * 1024)) {
        throw "Construction environment exceeds its byte bound."
    }
    return $block
}

function New-ConstructionJobserverSecurity {
    param(
        [Parameter(Mandatory = $true)]
        [System.Security.Principal.SecurityIdentifier]$Sid
    )

    $security = [System.Security.AccessControl.SemaphoreSecurity]::new()
    $security.SetAccessRuleProtection($true, $false)
    $rights = [System.Security.AccessControl.SemaphoreRights]::Synchronize -bor
        [System.Security.AccessControl.SemaphoreRights]::Modify
    $security.AddAccessRule(
        [System.Security.AccessControl.SemaphoreAccessRule]::new(
            $Sid,
            $rights,
            [System.Security.AccessControl.AccessControlType]::Allow
        )
    )
    return $security
}

function New-ConstructionJobserver {
    param(
        [Parameter(Mandatory = $true)]
        [System.Security.Principal.SecurityIdentifier]$Sid,

        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $security = New-ConstructionJobserverSecurity -Sid $Sid
    $createdNew = $false
    try {
        $semaphore = [System.Threading.SemaphoreAcl]::Create(
            1,
            1,
            $Name,
            [ref]$createdNew,
            $security
        )
    }
    catch {
        throw "Construction jobserver could not be created."
    }
    if (-not $createdNew) {
        $semaphore.Dispose()
        throw "Construction jobserver name was not unique."
    }
    return $semaphore
}

function Invoke-AsConstructionPrincipal {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,

        [Parameter(Mandatory = $true)]
        [int]$CommandTimeoutSeconds
    )

    return [ProjectAtlasConstructionProcess]::Run(
        $script:constructionUsername,
        $script:constructionSid,
        $script:constructionPassword,
        $pwsh,
        $Arguments,
        $source,
        $script:constructionEnvironment,
        $CommandTimeoutSeconds
    )
}

function Assert-FirewallRule {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [Parameter(Mandatory = $true)]
        [string]$ExpectedSddl
    )

    foreach ($store in @("PersistentStore", "ActiveStore")) {
        $rules = @(Get-NetFirewallRule -PolicyStore $store -Name $Name -ErrorAction Stop)
        if ($rules.Count -ne 1 -or
            [string]$rules[0].Direction -ne "Outbound" -or
            [string]$rules[0].Action -ne "Block" -or
            [string]$rules[0].Enabled -ne "True" -or
            [string]$rules[0].Profile -ne "Any") {
            throw "Construction firewall rule is not active with the required action."
        }
        $security = @($rules[0] | Get-NetFirewallSecurityFilter)
        $addresses = @($rules[0] | Get-NetFirewallAddressFilter)
        $ports = @($rules[0] | Get-NetFirewallPortFilter)
        $applications = @($rules[0] | Get-NetFirewallApplicationFilter)
        $services = @($rules[0] | Get-NetFirewallServiceFilter)
        $interfaces = @($rules[0] | Get-NetFirewallInterfaceFilter)
        $interfaceTypes = @($rules[0] | Get-NetFirewallInterfaceTypeFilter)
        if ($security.Count -ne 1 -or [string]$security[0].LocalUser -ne $ExpectedSddl -or
            [string]$security[0].RemoteUser -ne "Any" -or
            [string]$security[0].OverrideBlockRules -eq "True" -or
            $addresses.Count -ne 1 -or [string]$addresses[0].LocalAddress -ne "Any" -or
            [string]$addresses[0].RemoteAddress -ne "Any" -or
            $ports.Count -ne 1 -or [string]$ports[0].Protocol -ne "Any" -or
            [string]$ports[0].LocalPort -ne "Any" -or [string]$ports[0].RemotePort -ne "Any" -or
            $applications.Count -ne 1 -or [string]$applications[0].Program -ne "Any" -or
            (-not [string]::IsNullOrEmpty([string]$applications[0].Package) -and
                [string]$applications[0].Package -ne "Any") -or
            $services.Count -ne 1 -or [string]$services[0].Service -ne "Any" -or
            $interfaces.Count -ne 1 -or [string]$interfaces[0].InterfaceAlias -ne "Any" -or
            $interfaceTypes.Count -ne 1 -or [string]$interfaceTypes[0].InterfaceType -ne "Any") {
            throw "Construction firewall rule filters do not match the closed boundary."
        }
    }
}

function Write-BoundedConstructionFailure {
    param(
        [Parameter(Mandatory = $true)]
        [int]$ObservedExitCode,

        [Parameter(Mandatory = $true)]
        [string]$Username,

        [Parameter(Mandatory = $true)]
        [string]$Sid,

        [Parameter(Mandatory = $true)]
        [string]$FirewallRule
    )

    $statusItem = Get-Item -LiteralPath (Join-Path $output "construction-status.json") -Force
    if ($statusItem.PSIsContainer -or
        (($statusItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) -or
        $statusItem.Length -le 0 -or
        $statusItem.Length -gt 1024) {
        throw "Contained construction failed without a bounded status file."
    }
    $status = [System.IO.File]::ReadAllText($statusItem.FullName) | ConvertFrom-Json -Depth 4
    $expectedKeys = @("exit_code", "schema_version", "stage", "state")
    $actualKeys = @($status.PSObject.Properties.Name | Sort-Object)
    $allowedStages = @(
        "validate-inputs",
        "network-denial-canaries",
        "output-preparation",
        "optional-parser-worker-build",
        "artifact-assembler-build",
        "release-verifier-build",
        "runtime-containment-broker-build",
        "artifact-input-validation",
        "artifact-assembly-a",
        "archive-creation-a",
        "artifact-assembly-b",
        "archive-creation-b",
        "deterministic-archive-comparison",
        "publication"
    )
    $integerTypes = @([int], [long])
    if ($status -isnot [pscustomobject] -or
        (Compare-Object -ReferenceObject $expectedKeys -DifferenceObject $actualKeys) -or
        $integerTypes -notcontains $status.schema_version.GetType() -or
        $status.schema_version -ne 1 -or
        $status.state -ne "failed" -or
        $allowedStages -notcontains $status.stage -or
        $integerTypes -notcontains $status.exit_code.GetType() -or
        [int]$status.exit_code -ne $ObservedExitCode -or
        $ObservedExitCode -eq 0) {
        throw "Contained construction failed without a valid bounded status record."
    }

    $diagnosticPath = Join-Path $output "construction-diagnostic.txt"
    if (Test-Path -LiteralPath $diagnosticPath) {
        $diagnosticItem = Get-Item -LiteralPath $diagnosticPath -Force
        if ($diagnosticItem.PSIsContainer -or
            (($diagnosticItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) -or
            $diagnosticItem.Length -le 0 -or
            $diagnosticItem.Length -gt (64 * 1024)) {
            throw "Contained construction produced an invalid bounded diagnostic file."
        }
        $strictUtf8 = [System.Text.UTF8Encoding]::new($false, $true)
        $diagnostic = [System.IO.File]::ReadAllText($diagnosticItem.FullName, $strictUtf8)
        if ($diagnostic -match '[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]') {
            throw "Contained construction diagnostic contains forbidden control bytes."
        }
        $privateValues = @(
            $source, $inputs, $vendor, $output, $cargo, $temporary, $constructionHome,
            $toolchain, $pwshRoot, $vcTools, $windowsSdk,
            $Username, $Sid, $FirewallRule, $script:constructionJobserverName
        ) | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) } |
            Sort-Object { ([string]$_).Length } -Descending -Unique
        foreach ($privateValue in $privateValues) {
            $diagnostic = $diagnostic.Replace([string]$privateValue, "<private>")
        }
        foreach ($line in [System.Text.RegularExpressions.Regex]::Split(
            $diagnostic,
            "\r\n|\n|\r"
        )) {
            Write-Host "[contained-construction] $line"
        }
    }
    return [string]$status.stage
}

$identityBytes = [byte[]]::new(16)
[System.Security.Cryptography.RandomNumberGenerator]::Fill($identityBytes)
$randomHex = [Convert]::ToHexString($identityBytes).ToLowerInvariant()
$script:constructionUsername = "pa$($randomHex.Substring(0, 12))"
$firewallRule = "ProjectAtlas-ParserPack-Construction-$($randomHex.Substring(0, 12))"
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
$script:constructionPassword = [System.Security.SecureString]::new()
foreach ($character in $passwordCharacters) {
    $script:constructionPassword.AppendChar($character)
}
$script:constructionPassword.MakeReadOnly()
[Array]::Clear($passwordCharacters, 0, $passwordCharacters.Length)
[Array]::Clear($passwordBytes, 0, $passwordBytes.Length)

$state = @{
    schema_version = $stateSchemaVersion
    username = $script:constructionUsername
    sid = $placeholderSid
    firewall_rule = $firewallRule
    acl_paths = @()
    stage = "identity"
}
Write-ProtectedState -State $state

$operationError = $null
$script:constructionJobserver = $null
$script:constructionJobserverName = $null
try {
    $account = New-LocalUser `
        -Name $script:constructionUsername `
        -Password $script:constructionPassword `
        -AccountExpires ([DateTime]::UtcNow.AddHours(2)) `
        -PasswordNeverExpires `
        -UserMayNotChangePassword `
        -Description "ProjectAtlas optional parser pack construction" `
        -ErrorAction Stop
    $sid = $account.Sid
    if ($sid.Value -notmatch $sidPattern) {
        throw "Construction account did not receive one local-user SID."
    }
    $state.sid = $sid.Value
    $state.stage = "filesystem"
    Write-ProtectedState -State $state
    $script:constructionSid = $sid.Value
    $administrators = [System.Security.Principal.SecurityIdentifier]::new("S-1-5-32-544")
    if (@(Get-LocalGroupMember -SID $administrators | Where-Object { $_.SID -eq $sid }).Count -ne 0) {
        throw "Construction account unexpectedly belongs to Administrators."
    }
    $users = [System.Security.Principal.SecurityIdentifier]::new("S-1-5-32-545")
    if (@(Get-LocalGroupMember -SID $users | Where-Object { $_.SID -eq $sid }).Count -eq 0) {
        Add-LocalGroupMember -SID $users -Member $account -ErrorAction Stop
    }
    if (@(Get-LocalGroupMember -SID $users | Where-Object { $_.SID -eq $sid }).Count -ne 1) {
        throw "Construction account did not receive ordinary local-user membership."
    }
    $readRoots = @($source, $inputs, $vendor, $pwshRoot, $toolchain, $vcTools, $windowsSdk)
    $writeRoots = @($output, $cargo, $temporary, $constructionHome)
    $aclPaths = [System.Collections.Generic.List[string]]::new()
    foreach ($path in $readRoots) {
        $aclPaths.Add($path)
        $state.acl_paths = @($aclPaths | Sort-Object -Unique)
        Write-ProtectedState -State $state
        Add-PrincipalAcl -Path $path -Sid $sid -Rights ReadAndExecute
    }
    foreach ($path in $writeRoots) {
        $aclPaths.Add($path)
        $state.acl_paths = @($aclPaths | Sort-Object -Unique)
        Write-ProtectedState -State $state
        Add-PrincipalAcl -Path $path -Sid $sid -Rights Modify
    }
    $toolchainBin = Join-Path $toolchain "bin"
    foreach ($file in @(
        (Join-Path $toolchainBin "cargo.exe"),
        (Join-Path $toolchainBin "rustc.exe")
    ) + @(Get-ChildItem -LiteralPath $toolchainBin -Filter "*.dll" -File | Select-Object -ExpandProperty FullName)) {
        $regularFile = Get-RegularFile -Path $file -Role "toolchain executable closure"
        $aclPaths.Add($regularFile)
        $state.acl_paths = @($aclPaths | Sort-Object -Unique)
        Write-ProtectedState -State $state
        Add-PrincipalAcl -Path $regularFile -Sid $sid -Rights ReadAndExecute
    }
    $state.acl_paths = @($aclPaths | Sort-Object -Unique)
    $state.stage = "network"
    Write-ProtectedState -State $state

    $msvcBin = Join-Path $vcTools "bin/Hostx64/x64"
    $sdkVersion = $env:WindowsSDKVersion.TrimEnd('\')
    $sdkBin = Join-Path $windowsSdk "bin/$sdkVersion/x64"
    $boundedPath = @(
        $toolchainBin,
        $msvcBin,
        $sdkBin,
        (Join-Path $env:SystemRoot "System32"),
        $env:SystemRoot
    ) -join ';'
    $appData = Join-Path $constructionHome "AppData/Roaming"
    $localAppData = Join-Path $constructionHome "AppData/Local"
    foreach ($directory in @($appData, $localAppData)) {
        [System.IO.Directory]::CreateDirectory($directory) | Out-Null
    }
    $script:constructionJobserverName = "Local\ProjectAtlasParserPack-$randomHex"
    $script:constructionJobserver = New-ConstructionJobserver `
        -Sid $sid `
        -Name $script:constructionJobserverName
    $environment = @{
        SystemRoot = $env:SystemRoot
        WINDIR = $env:WINDIR
        ComSpec = $env:ComSpec
        OS = "Windows_NT"
        PATHEXT = $env:PATHEXT
        ProgramData = $env:ProgramData
        ProgramFiles = $env:ProgramFiles
        'ProgramFiles(x86)' = ${env:ProgramFiles(x86)}
        CommonProgramFiles = $env:CommonProgramFiles
        'CommonProgramFiles(x86)' = ${env:CommonProgramFiles(x86)}
        PATH = $boundedPath
        INCLUDE = $env:INCLUDE
        LIB = $env:LIB
        LIBPATH = $env:LIBPATH
        RUSTC = Join-Path $toolchainBin "rustc.exe"
        CARGO_HOME = $cargo
        CARGO_NET_OFFLINE = "true"
        CARGO_INCREMENTAL = "0"
        CARGO_MAKEFLAGS = "-j --jobserver-fds=$($script:constructionJobserverName) --jobserver-auth=$($script:constructionJobserverName)"
        CARGO_TERM_COLOR = "never"
        TSLP_OFFLINE = "1"
        HOME = $constructionHome
        USERPROFILE = $constructionHome
        APPDATA = $appData
        LOCALAPPDATA = $localAppData
        TEMP = $temporary
        TMP = $temporary
    }
    $script:constructionEnvironment = New-EnvironmentBlock -Values $environment

    $networkArguments = @(
        "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", $networkCheck,
        "-Mode", "require-reachable",
        "-ResolverAddress", $ResolverAddress
    )
    if ((Invoke-AsConstructionPrincipal -Arguments $networkArguments -CommandTimeoutSeconds 45) -ne 0) {
        throw "Construction principal did not pass the pre-denial reachability baseline."
    }

    $rawDescriptor = [System.Security.AccessControl.RawSecurityDescriptor]::new(
        "D:(A;;CC;;;$($sid.Value))"
    )
    $principalSddl = $rawDescriptor.GetSddlForm(
        [System.Security.AccessControl.AccessControlSections]::Access
    )
    New-NetFirewallRule `
        -PolicyStore PersistentStore `
        -Name $firewallRule `
        -DisplayName "ProjectAtlas optional parser construction" `
        -Description "Ephemeral exact-principal outbound denial" `
        -Direction Outbound `
        -Action Block `
        -Enabled True `
        -Profile Any `
        -Protocol Any `
        -LocalAddress Any `
        -RemoteAddress Any `
        -LocalUser $principalSddl `
        -ErrorAction Stop | Out-Null
    Assert-FirewallRule -Name $firewallRule -ExpectedSddl $principalSddl

    $parentNetwork = & $pwsh -NoProfile -File $networkCheck `
        -Mode require-reachable `
        -ResolverAddress $ResolverAddress
    if ($LASTEXITCODE -ne 0) {
        throw "Principal-scoped firewall rule disrupted the runner network boundary."
    }
    $networkArguments[8] = "require-denied"
    if ((Invoke-AsConstructionPrincipal -Arguments $networkArguments -CommandTimeoutSeconds 45) -ne 0) {
        throw "Construction principal retained direct network egress."
    }
    $probePath = Join-Path $temporary "construction-boundary-probe.ps1"
    $probeSource = @'
param(
    [Parameter(Mandatory = $true)][string]$ExpectedSid,
    [Parameter(Mandatory = $true)][string]$JobserverName
)
$ErrorActionPreference = "Stop"
$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [System.Security.Principal.WindowsPrincipal]::new($identity)
if ($identity.User.Value -ne $ExpectedSid -or
    $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
    exit 21
}
$handle = [System.IO.File]::Open(
    "\\.\NUL",
    [System.IO.FileMode]::Open,
    [System.IO.FileAccess]::ReadWrite,
    [System.IO.FileShare]::ReadWrite
)
$handle.Dispose()
$start = [System.Diagnostics.ProcessStartInfo]::new()
$start.FileName = $env:ComSpec
$start.UseShellExecute = $false
$start.ArgumentList.Add("/d")
$start.ArgumentList.Add("/c")
$start.ArgumentList.Add("start /b /wait cmd.exe /d /c exit 0")
$child = [System.Diagnostics.Process]::Start($start)
$child.WaitForExit()
if ($child.ExitCode -ne 0) { exit 22 }
foreach ($entry in [Environment]::GetEnvironmentVariables().Keys) {
    if ([string]$entry -match '(?i)(^GITHUB_|^ACTIONS_|^RUNNER_|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL|COOKIE|AUTH|API_KEY|PRIVATE_KEY|PROXY)') {
        exit 23
    }
}
$jobserverRights = [System.Security.AccessControl.SemaphoreRights]::Synchronize -bor
    [System.Security.AccessControl.SemaphoreRights]::Modify
$inheritedJobserver = $null
try {
    $inheritedJobserver = [System.Threading.SemaphoreAcl]::OpenExisting(
        $JobserverName,
        $jobserverRights
    )
}
catch [System.Threading.WaitHandleCannotBeOpenedException] {
    exit 24
}
catch [System.UnauthorizedAccessException] {
    exit 25
}
catch {
    exit 26
}
try {
    if (-not $inheritedJobserver.WaitOne(0) -or $inheritedJobserver.Release() -ne 0) {
        exit 27
    }
}
finally {
    $inheritedJobserver.Dispose()
}
$rustcStart = [System.Diagnostics.ProcessStartInfo]::new()
$rustcStart.FileName = $env:RUSTC
$rustcStart.UseShellExecute = $false
$rustcStart.CreateNoWindow = $true
$rustcStart.RedirectStandardOutput = $true
$rustcStart.RedirectStandardError = $true
$rustcStart.ArgumentList.Add("--print")
$rustcStart.ArgumentList.Add("cfg")
$rustc = [System.Diagnostics.Process]::Start($rustcStart)
$rustcOutput = $rustc.StandardOutput.ReadToEnd()
$rustcError = $rustc.StandardError.ReadToEnd()
$rustc.WaitForExit()
if ($rustc.ExitCode -ne 0 -or
    -not [string]::IsNullOrEmpty($rustcError) -or
    $rustcOutput -notmatch '(?m)^target_os="windows"$') {
    exit 28
}
$rustc.Dispose()
exit 0
'@
    [System.IO.File]::WriteAllText($probePath, $probeSource, [System.Text.UTF8Encoding]::new($false))
    $probeArguments = @(
        "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", $probePath,
        "-ExpectedSid", $sid.Value,
        "-JobserverName", $script:constructionJobserverName
    )
    $probeExitCode = Invoke-AsConstructionPrincipal -Arguments $probeArguments -CommandTimeoutSeconds 30
    if ($probeExitCode -ne 0) {
        $probeFailure = switch ($probeExitCode) {
            21 { "identity" }
            22 { "descendant" }
            23 { "environment" }
            24 { "jobserver-missing" }
            25 { "jobserver-access" }
            26 { "jobserver-open" }
            27 { "jobserver-token" }
            28 { "rustc-jobserver" }
            default { "unexpected-exit" }
        }
        throw "Construction principal boundary probe failed at $probeFailure."
    }
    if ([ProjectAtlasConstructionProcess]::LastTotalProcesses -lt 3) {
        throw "Construction principal boundary probe did not contain its descendant process tree."
    }

    $constructionArguments = @(
        "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
        "-File", $constructionScript,
        "-Target", "x86_64-pc-windows-msvc",
        "-SourceRoot", $source,
        "-InputDirectory", $inputs,
        "-OutputDirectory", $output,
        "-ProjectAtlasRevision", $ProjectAtlasRevision,
        "-CargoPackageVersion", $CargoPackageVersion,
        "-IntendedReleaseVersion", $IntendedReleaseVersion,
        "-CargoLockSha256", $CargoLockSha256,
        "-RustcRelease", $RustcRelease,
        "-RustcCommitHash", $RustcCommitHash,
        "-NetworkIsolation", $expectedIsolation,
        "-ResolverAddress", $ResolverAddress
    )
    $state.stage = "construction"
    Write-ProtectedState -State $state
    $constructionExitCode = Invoke-AsConstructionPrincipal `
        -Arguments $constructionArguments `
        -CommandTimeoutSeconds $TimeoutSeconds
    if ($constructionExitCode -ne 0) {
        $failedStage = Write-BoundedConstructionFailure `
            -ObservedExitCode $constructionExitCode `
            -Username $script:constructionUsername `
            -Sid $sid.Value `
            -FirewallRule $firewallRule
        throw "Construction process failed at $failedStage with exit code $constructionExitCode."
    }
    Assert-FirewallRule -Name $firewallRule -ExpectedSddl $principalSddl
    if ((Invoke-AsConstructionPrincipal -Arguments $networkArguments -CommandTimeoutSeconds 45) -ne 0) {
        throw "Construction principal did not retain physical egress denial after construction."
    }
}
catch {
    $operationError = $_
}
finally {
    try {
        Invoke-Cleanup
    }
    catch {
        if ($null -eq $operationError) {
            $operationError = $_
        }
        else {
            $operationError = [System.AggregateException]::new(
                "Parser-pack construction and mandatory cleanup both failed.",
                @($operationError.Exception, $_.Exception)
            )
        }
    }
    try {
        if ($null -ne $script:constructionJobserver) {
            $script:constructionJobserver.Dispose()
        }
    }
    catch {
        $jobserverCleanupError = [System.InvalidOperationException]::new(
            "Construction jobserver cleanup failed."
        )
        if ($null -eq $operationError) {
            $operationError = $jobserverCleanupError
        }
        else {
            $operationError = [System.AggregateException]::new(
                "Parser-pack construction cleanup retained more than one failure.",
                @($operationError.Exception, $jobserverCleanupError)
            )
        }
    }
    $script:constructionPassword.Dispose()
}

if ($null -ne $operationError) {
    throw $operationError
}

& $pwsh -NoProfile -File $networkCheck `
    -Mode require-reachable `
    -ResolverAddress $ResolverAddress
if ($LASTEXITCODE -ne 0) {
    throw "Runner network did not recover after construction cleanup."
}
