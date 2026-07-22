[CmdletBinding(DefaultParameterSetName = 'Parent')]
param(
    [Parameter(Mandatory = $true, ParameterSetName = 'Parent')]
    [ValidateSet('construction', 'recovery')]
    [string]$TargetKind,

    [Parameter(Mandatory = $true, ParameterSetName = 'Parent')]
    [hashtable]$TargetParameters,

    [Parameter(ParameterSetName = 'Parent')]
    [ValidateRange(60, 7200)]
    [int]$TimeoutSeconds = 4200,

    [Parameter(ParameterSetName = 'Parent')]
    [Parameter(Mandatory = $true, ParameterSetName = 'Child')]
    [ValidateSet('none', 'hold-before-join')]
    [string]$BootstrapTestFault = 'none',

    [Parameter(Mandatory = $true, ParameterSetName = 'Child')]
    [switch]$BrokerChild,

    [Parameter(Mandatory = $true, ParameterSetName = 'Child')]
    [string]$BrokerJobName,

    [Parameter(Mandatory = $true, ParameterSetName = 'Child')]
    [string]$PipeName,

    [Parameter(Mandatory = $true, ParameterSetName = 'Child')]
    [ValidateRange(1, [int]::MaxValue)]
    [int]$ParentProcessId,

    [Parameter(Mandatory = $true, ParameterSetName = 'Child')]
    [ValidateRange(1, [long]::MaxValue)]
    [long]$ParentStartTimeUtcTicks,

    [Parameter(Mandatory = $true, ParameterSetName = 'Child')]
    [ValidateRange(0, [int]::MaxValue)]
    [int]$ParentSessionId,

    [Parameter(Mandatory = $true, ParameterSetName = 'Child')]
    [ValidateSet('construction', 'recovery')]
    [string]$BrokerTargetKind,

    [Parameter(Mandatory = $true, ParameterSetName = 'Child')]
    [ValidateRange(1, [long]::MaxValue)]
    [long]$BootstrapDeadlineUtcTicks
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$brokerJobPattern = '\AGlobal\\ProjectAtlasParserPackBroker-[0-9a-f]{32}\z'
$pipePattern = '\AProjectAtlasParserPackBroker-[0-9a-f]{32}\z'
$maximumFrameBytes = 64 * 1024
$maximumDiagnosticCharacters = 12 * 1024
$bootstrapTimeoutSeconds = 60
$secretNamePattern = '(?i)(^GITHUB_|^ACTIONS_|^RUNNER_|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL|COOKIE|AUTH|API_KEY|PRIVATE_KEY|PROXY)'

$nativeSource = @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using Microsoft.Win32.SafeHandles;

public sealed class ProjectAtlasWindowsRunnerJob : IDisposable
{
    private const uint ErrorAlreadyExists = 183;
    private const uint JobObjectAssignProcess = 0x0001;
    private const uint JobObjectQuery = 0x0004;
    private const uint JobObjectTerminate = 0x0008;
    private const uint JobObjectLimitBreakawayOk = 0x00000800;
    private const uint JobObjectLimitSilentBreakawayOk = 0x00001000;
    private const uint JobObjectLimitKillOnJobClose = 0x00002000;
    private const int JobObjectBasicAccountingInformation = 1;
    private const int JobObjectExtendedLimitInformation = 9;
    private const uint SddlRevision1 = 1;
    private const uint TokenQuery = 0x0008;
    private const uint StillActive = 259;
    private const string JobPrefix = "Global\\ProjectAtlasParserPackBroker-";

    private IntPtr job;

    [StructLayout(LayoutKind.Sequential)]
    private struct SecurityAttributes
    {
        internal int Length;
        internal IntPtr SecurityDescriptor;
        [MarshalAs(UnmanagedType.Bool)]
        internal bool InheritHandle;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct IoCounters
    {
        internal ulong ReadOperationCount;
        internal ulong WriteOperationCount;
        internal ulong OtherOperationCount;
        internal ulong ReadTransferCount;
        internal ulong WriteTransferCount;
        internal ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct BasicLimitInformation
    {
        internal long PerProcessUserTimeLimit;
        internal long PerJobUserTimeLimit;
        internal uint LimitFlags;
        internal UIntPtr MinimumWorkingSetSize;
        internal UIntPtr MaximumWorkingSetSize;
        internal uint ActiveProcessLimit;
        internal UIntPtr Affinity;
        internal uint PriorityClass;
        internal uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ExtendedLimitInformation
    {
        internal BasicLimitInformation BasicLimitInformation;
        internal IoCounters IoInfo;
        internal UIntPtr ProcessMemoryLimit;
        internal UIntPtr JobMemoryLimit;
        internal UIntPtr PeakProcessMemoryUsed;
        internal UIntPtr PeakJobMemoryUsed;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct BasicAccountingInformation
    {
        internal long TotalUserTime;
        internal long TotalKernelTime;
        internal long ThisPeriodTotalUserTime;
        internal long ThisPeriodTotalKernelTime;
        internal uint TotalPageFaultCount;
        internal uint TotalProcesses;
        internal uint ActiveProcesses;
        internal uint TotalTerminatedProcesses;
    }

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool ConvertStringSecurityDescriptorToSecurityDescriptor(
        string stringSecurityDescriptor,
        uint stringSecurityDescriptorRevision,
        out IntPtr securityDescriptor,
        IntPtr securityDescriptorSize);

    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool OpenProcessToken(
        IntPtr process,
        uint desiredAccess,
        out SafeAccessTokenHandle token);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObject(
        ref SecurityAttributes attributes,
        string name);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr OpenJobObject(
        uint desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool inheritHandle,
        string name);

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
        out ExtendedLimitInformation information,
        uint informationLength,
        IntPtr returnLength);

    [DllImport(
        "kernel32.dll",
        EntryPoint = "QueryInformationJobObject",
        SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool QueryBasicAccountingInformation(
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
    private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool TerminateProcess(IntPtr process, uint exitCode);

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr LocalFree(IntPtr memory);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(
        SafePipeHandle pipe,
        out uint clientProcessId);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeServerProcessId(
        SafePipeHandle pipe,
        out uint serverProcessId);

    private ProjectAtlasWindowsRunnerJob(IntPtr ownedJob)
    {
        job = ownedJob;
    }

    public static ProjectAtlasWindowsRunnerJob Create(string name, string ownerSid)
    {
        ValidateName(name);
        SecurityIdentifier expectedOwner = new SecurityIdentifier(ownerSid);
        string sddl = "O:" + expectedOwner.Value +
            "D:P(A;;GA;;;SY)(A;;GA;;;" + expectedOwner.Value + ")";
        IntPtr descriptor = IntPtr.Zero;
        if (!ConvertStringSecurityDescriptorToSecurityDescriptor(
            sddl,
            SddlRevision1,
            out descriptor,
            IntPtr.Zero))
        {
            throw Failure("create-broker-job-security");
        }
        try
        {
            SecurityAttributes attributes = new SecurityAttributes();
            attributes.Length = Marshal.SizeOf<SecurityAttributes>();
            attributes.SecurityDescriptor = descriptor;
            attributes.InheritHandle = false;
            IntPtr handle = CreateJobObject(ref attributes, name);
            int createError = Marshal.GetLastWin32Error();
            if (handle == IntPtr.Zero)
            {
                throw new Win32Exception(createError, "create-broker-job");
            }
            if ((uint)createError == ErrorAlreadyExists)
            {
                CloseHandle(handle);
                throw new InvalidOperationException("broker-job-name-collision");
            }
            try
            {
                ExtendedLimitInformation limits = new ExtendedLimitInformation();
                limits.BasicLimitInformation.LimitFlags =
                    JobObjectLimitKillOnJobClose | JobObjectLimitBreakawayOk;
                if (!SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    ref limits,
                    (uint)Marshal.SizeOf<ExtendedLimitInformation>()))
                {
                    throw Failure("configure-broker-job");
                }
                return new ProjectAtlasWindowsRunnerJob(handle);
            }
            catch
            {
                CloseHandle(handle);
                throw;
            }
        }
        finally
        {
            if (descriptor != IntPtr.Zero)
            {
                LocalFree(descriptor);
            }
        }
    }

    public static void Join(string name)
    {
        ValidateName(name);
        bool inAnyJob;
        if (!IsProcessInJob(GetCurrentProcess(), IntPtr.Zero, out inAnyJob))
        {
            throw Failure("inspect-broker-bootstrap-job");
        }
        if (inAnyJob)
        {
            throw new InvalidOperationException("broker-bootstrap-retained-inherited-job");
        }

        IntPtr targetJob = OpenJobObject(
            JobObjectAssignProcess | JobObjectQuery,
            false,
            name);
        if (targetJob == IntPtr.Zero)
        {
            throw Failure("open-broker-job");
        }
        try
        {
            ValidatePolicy(targetJob);
            if (!AssignProcessToJobObject(targetJob, GetCurrentProcess()))
            {
                throw Failure("assign-broker-job");
            }
            bool inExactJob;
            if (!IsProcessInJob(GetCurrentProcess(), targetJob, out inExactJob))
            {
                throw Failure("verify-broker-job-membership");
            }
            if (!inExactJob)
            {
                throw new InvalidOperationException("broker-job-membership");
            }
        }
        finally
        {
            if (!CloseHandle(targetJob))
            {
                throw Failure("close-broker-job");
            }
        }
    }

    public static void ValidateProcessIdentity(
        Process process,
        string expectedImage,
        string expectedSid,
        long earliestStartTimeUtcTicks,
        int expectedSessionId)
    {
        if (process == null ||
            process.HasExited ||
            !string.Equals(
                process.MainModule.FileName,
                expectedImage,
                StringComparison.OrdinalIgnoreCase) ||
            process.StartTime.ToUniversalTime().Ticks < earliestStartTimeUtcTicks ||
            process.SessionId != expectedSessionId ||
            !string.Equals(GetProcessSid(process), expectedSid, StringComparison.Ordinal))
        {
            throw new InvalidOperationException("broker-process-identity");
        }
    }

    public void ValidateProcessMembership(Process process)
    {
        ThrowIfDisposed();
        if (process == null || process.HasExited)
        {
            throw new InvalidOperationException("broker-process-identity");
        }
        bool inExactJob;
        if (!IsProcessInJob(process.Handle, job, out inExactJob))
        {
            throw Failure("inspect-broker-process-membership");
        }
        if (!inExactJob)
        {
            throw new InvalidOperationException("broker-process-membership");
        }
    }

    public uint ActiveProcessCount
    {
        get
        {
            ThrowIfDisposed();
            BasicAccountingInformation accounting;
            if (!QueryBasicAccountingInformation(
                job,
                JobObjectBasicAccountingInformation,
                out accounting,
                (uint)Marshal.SizeOf<BasicAccountingInformation>(),
                IntPtr.Zero))
            {
                throw Failure("query-broker-job-accounting");
            }
            return accounting.ActiveProcesses;
        }
    }

    public void Terminate(uint exitCode)
    {
        ThrowIfDisposed();
        if (!TerminateJobObject(job, exitCode))
        {
            throw Failure("terminate-broker-job");
        }
    }

    public void Dispose()
    {
        if (job != IntPtr.Zero)
        {
            IntPtr ownedJob = job;
            job = IntPtr.Zero;
            if (!CloseHandle(ownedJob))
            {
                throw Failure("close-owned-broker-job");
            }
        }
    }

    public static int GetPipeClientProcessId(NamedPipeServerStream pipe)
    {
        uint processId;
        if (!GetNamedPipeClientProcessId(pipe.SafePipeHandle, out processId) ||
            processId == 0 || processId > Int32.MaxValue)
        {
            throw Failure("identify-broker-pipe-client");
        }
        return checked((int)processId);
    }

    public static int GetPipeServerProcessId(NamedPipeClientStream pipe)
    {
        uint processId;
        if (!GetNamedPipeServerProcessId(pipe.SafePipeHandle, out processId) ||
            processId == 0 || processId > Int32.MaxValue)
        {
            throw Failure("identify-broker-pipe-server");
        }
        return checked((int)processId);
    }

    public static string GetProcessSid(int processId)
    {
        using (Process process = Process.GetProcessById(processId))
        {
            return GetProcessSid(process);
        }
    }

    public static int GetProcessExitCode(IntPtr process)
    {
        uint exitCode;
        if (process == IntPtr.Zero || !GetExitCodeProcess(process, out exitCode))
        {
            throw Failure("read-broker-process-exit-code");
        }
        return unchecked((int)exitCode);
    }

    public static void TerminateExactProcess(IntPtr process, uint exitCode)
    {
        uint observedExitCode;
        if (process == IntPtr.Zero ||
            !GetExitCodeProcess(process, out observedExitCode))
        {
            throw Failure("inspect-exact-broker-process");
        }
        if (observedExitCode != StillActive)
        {
            return;
        }
        if (!TerminateProcess(process, exitCode))
        {
            int terminateError = Marshal.GetLastWin32Error();
            if (GetExitCodeProcess(process, out observedExitCode) &&
                observedExitCode != StillActive)
            {
                return;
            }
            throw new Win32Exception(
                terminateError,
                "terminate-exact-broker-process");
        }
    }

    public static string BuildCommandLine(string executable, string[] arguments)
    {
        StringBuilder result = new StringBuilder(Quote(executable));
        foreach (string argument in arguments)
        {
            result.Append(' ').Append(Quote(argument));
        }
        if (result.Length > 8191)
        {
            throw new InvalidOperationException("broker-bootstrap-command-line-too-long");
        }
        return result.ToString();
    }

    private static string GetProcessSid(Process process)
    {
        SafeAccessTokenHandle token;
        if (!OpenProcessToken(process.Handle, TokenQuery, out token))
        {
            throw Failure("open-broker-process-token");
        }
        using (token)
        using (WindowsIdentity identity = new WindowsIdentity(token.DangerousGetHandle()))
        {
            if (identity.User == null)
            {
                throw new InvalidOperationException("broker-process-sid");
            }
            return identity.User.Value;
        }
    }

    private static void ValidatePolicy(IntPtr targetJob)
    {
        ExtendedLimitInformation limits;
        if (!QueryInformationJobObject(
            targetJob,
            JobObjectExtendedLimitInformation,
            out limits,
            (uint)Marshal.SizeOf<ExtendedLimitInformation>(),
            IntPtr.Zero))
        {
            throw Failure("query-broker-job");
        }
        uint expected = JobObjectLimitKillOnJobClose | JobObjectLimitBreakawayOk;
        if (limits.BasicLimitInformation.LimitFlags != expected ||
            (limits.BasicLimitInformation.LimitFlags &
                JobObjectLimitSilentBreakawayOk) != 0)
        {
            throw new InvalidOperationException("broker-job-policy");
        }
    }

    private static void ValidateName(string name)
    {
        if (string.IsNullOrEmpty(name) ||
            name.Length != JobPrefix.Length + 32 ||
            !name.StartsWith(JobPrefix, StringComparison.Ordinal))
        {
            throw new InvalidOperationException("broker-job-name");
        }
        for (int index = JobPrefix.Length; index < name.Length; index++)
        {
            char character = name[index];
            if (!((character >= '0' && character <= '9') ||
                (character >= 'a' && character <= 'f')))
            {
                throw new InvalidOperationException("broker-job-name");
            }
        }
    }

    private void ThrowIfDisposed()
    {
        if (job == IntPtr.Zero)
        {
            throw new ObjectDisposedException(nameof(ProjectAtlasWindowsRunnerJob));
        }
    }

    private static Win32Exception Failure(string operation)
    {
        return new Win32Exception(Marshal.GetLastWin32Error(), operation);
    }

    private static string Quote(string value)
    {
        if (value == null)
        {
            throw new ArgumentNullException(nameof(value));
        }
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

if (-not ('ProjectAtlasWindowsRunnerJob' -as [type])) {
    Add-Type -TypeDefinition $nativeSource -Language CSharp
}

function New-DeadlineCancellationSource {
    param(
        [Parameter(Mandatory = $true)]
        [DateTime]$Deadline
    )

    $remaining = $Deadline.ToUniversalTime() - [DateTime]::UtcNow
    if ($remaining -le [TimeSpan]::Zero) {
        throw 'broker-deadline-exceeded'
    }
    return [System.Threading.CancellationTokenSource]::new($remaining)
}

function Read-BrokerFrame {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.Stream]$Stream,

        [Parameter(Mandatory = $true)]
        [DateTime]$Deadline
    )

    $header = [byte[]]::new(4)
    $headerOffset = 0
    while ($headerOffset -lt $header.Length) {
        $deadlineSource = New-DeadlineCancellationSource -Deadline $Deadline
        try {
            $read = $Stream.ReadAsync(
                $header,
                $headerOffset,
                $header.Length - $headerOffset,
                $deadlineSource.Token
            ).GetAwaiter().GetResult()
        }
        finally {
            $deadlineSource.Dispose()
        }
        if ($read -le 0) {
            throw 'broker-pipe-closed-before-frame'
        }
        $headerOffset += $read
    }
    $length = [System.BitConverter]::ToInt32($header, 0)
    if ($length -le 0 -or $length -gt $maximumFrameBytes) {
        throw 'broker-frame-length'
    }
    $payload = [byte[]]::new($length)
    $offset = 0
    while ($offset -lt $payload.Length) {
        $deadlineSource = New-DeadlineCancellationSource -Deadline $Deadline
        try {
            $read = $Stream.ReadAsync(
                $payload,
                $offset,
                $payload.Length - $offset,
                $deadlineSource.Token
            ).GetAwaiter().GetResult()
        }
        finally {
            $deadlineSource.Dispose()
        }
        if ($read -le 0) {
            throw 'broker-pipe-closed-during-frame'
        }
        $offset += $read
    }
    $json = [System.Text.UTF8Encoding]::new($false, $true).GetString($payload)
    return $json | ConvertFrom-Json -AsHashtable -Depth 12
}

function Write-BrokerFrame {
    param(
        [Parameter(Mandatory = $true)]
        [System.IO.Stream]$Stream,

        [Parameter(Mandatory = $true)]
        [hashtable]$Value,

        [Parameter(Mandatory = $true)]
        [DateTime]$Deadline
    )

    $json = $Value | ConvertTo-Json -Depth 12 -Compress
    $payload = [System.Text.UTF8Encoding]::new($false, $true).GetBytes($json)
    if ($payload.Length -le 0 -or $payload.Length -gt $maximumFrameBytes) {
        throw 'broker-frame-size'
    }
    $header = [System.BitConverter]::GetBytes([int]$payload.Length)
    foreach ($buffer in @($header, $payload)) {
        $deadlineSource = New-DeadlineCancellationSource -Deadline $Deadline
        try {
            [void]$Stream.WriteAsync(
                $buffer,
                0,
                $buffer.Length,
                $deadlineSource.Token
            ).GetAwaiter().GetResult()
        }
        finally {
            $deadlineSource.Dispose()
        }
    }
    $deadlineSource = New-DeadlineCancellationSource -Deadline $Deadline
    try {
        [void]$Stream.FlushAsync($deadlineSource.Token).GetAwaiter().GetResult()
    }
    finally {
        $deadlineSource.Dispose()
    }
}

function Assert-BrokerValue {
    param(
        [AllowNull()]
        [object]$Value,

        [ValidateRange(0, 8)]
        [int]$Depth = 0
    )

    if ($Depth -gt 8) {
        throw 'broker-value-depth'
    }
    if ($null -eq $Value -or
        $Value -is [bool] -or
        $Value -is [byte] -or
        $Value -is [int16] -or
        $Value -is [int32] -or
        $Value -is [int64] -or
        $Value -is [uint16] -or
        $Value -is [uint32]) {
        return
    }
    if ($Value -is [string]) {
        if ($Value.Length -gt 8192 -or $Value -match "`0|`r|`n") {
            throw 'broker-string-value'
        }
        return
    }
    if ($Value -is [System.Collections.IDictionary]) {
        if ($Value.Count -gt 48) {
            throw 'broker-map-entry-count'
        }
        foreach ($entry in $Value.GetEnumerator()) {
            $key = [string]$entry.Key
            if ($key -notmatch '\A[A-Za-z][A-Za-z0-9]*\z' -or
                $key -match $secretNamePattern) {
                throw 'broker-map-key'
            }
            Assert-BrokerValue -Value $entry.Value -Depth ($Depth + 1)
        }
        return
    }
    if ($Value -is [System.Collections.IEnumerable]) {
        $rows = @($Value)
        if ($rows.Count -gt 64) {
            throw 'broker-array-entry-count'
        }
        foreach ($row in $rows) {
            Assert-BrokerValue -Value $row -Depth ($Depth + 1)
        }
        return
    }
    throw 'broker-value-type'
}

function Assert-TargetParameters {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('construction', 'recovery')]
        [string]$Kind,

        [Parameter(Mandatory = $true)]
        [hashtable]$Parameters
    )

    $constructionKeys = @(
        'Mode', 'StatePath', 'SourceRoot', 'InputDirectory', 'VendorDirectory',
        'OutputDirectory', 'CargoHome', 'TemporaryDirectory', 'HomeDirectory',
        'ToolchainRoot', 'PwshPath', 'VcToolsRoot', 'WindowsSdkRoot',
        'ProjectAtlasRevision', 'CargoPackageVersion', 'IntendedReleaseVersion',
        'CargoLockSha256', 'RustcRelease', 'RustcCommitHash', 'ResolverAddress',
        'TimeoutSeconds'
    )
    $isStaticRecovery = $Kind -eq 'recovery' -and
        $Parameters.ContainsKey('StaticOnly') -and
        [bool]$Parameters.StaticOnly
    $required = switch ($Kind) {
        'construction' { @($constructionKeys | Where-Object { $_ -ne 'TimeoutSeconds' }) }
        'recovery' {
            if ($isStaticRecovery) {
                @('ProductionWrapper', 'StaticOnly')
            }
            else {
                @('ProductionWrapper', 'LauncherPath', 'ConstructionParameters',
                    'RecoveryRoot', 'RunnerTemporaryRoot')
            }
        }
    }
    $allowed = switch ($Kind) {
        'construction' { $constructionKeys }
        'recovery' { $required }
    }
    foreach ($key in @($Parameters.Keys)) {
        if ([string]$key -cnotin $allowed) {
            throw 'broker-target-parameter'
        }
    }
    foreach ($key in $required) {
        if (-not $Parameters.ContainsKey($key) -or $null -eq $Parameters[$key]) {
            throw 'broker-target-parameter'
        }
    }
    Assert-BrokerValue -Value $Parameters
}

function Get-BrokerEnvironment {
    $captured = @{}
    foreach ($name in @('WindowsSDKVersion', 'INCLUDE', 'LIB', 'LIBPATH')) {
        $value = [Environment]::GetEnvironmentVariable($name)
        if ([string]::IsNullOrWhiteSpace($value) -or
            $value.Length -gt 8192 -or
            $value -match "`0|`r|`n") {
            throw "broker-required-environment-$name"
        }
        $captured[$name] = $value
    }
    return $captured
}

function Set-BrokerEnvironment {
    param(
        [Parameter(Mandatory = $true)]
        [hashtable]$Captured,

        [Parameter(Mandatory = $true)]
        [string]$TemporaryRoot
    )

    if (($Captured.Keys | Sort-Object) -join ',' -cne
        'INCLUDE,LIB,LIBPATH,WindowsSDKVersion') {
        throw 'broker-environment-contract'
    }
    $systemRoot = [Environment]::GetFolderPath(
        [Environment+SpecialFolder]::Windows
    )
    $programFiles = [Environment]::GetFolderPath(
        [Environment+SpecialFolder]::ProgramFiles
    )
    $programFilesX86 = [Environment]::GetFolderPath(
        [Environment+SpecialFolder]::ProgramFilesX86
    )
    $commonProgramFiles = [Environment]::GetFolderPath(
        [Environment+SpecialFolder]::CommonProgramFiles
    )
    $commonProgramFilesX86 = [Environment]::GetFolderPath(
        [Environment+SpecialFolder]::CommonProgramFilesX86
    )
    $systemModuleRoot = Join-Path $systemRoot 'System32/WindowsPowerShell/v1.0/Modules'
    $pwshModuleRoot = Join-Path $PSHOME 'Modules'
    $sharedPwshModuleRoot = Join-Path $programFiles 'PowerShell/Modules'
    $temporaryItem = Get-Item -LiteralPath ([System.IO.Path]::GetFullPath($TemporaryRoot)) -Force
    if (-not $temporaryItem.PSIsContainer -or
        (($temporaryItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw 'broker-temporary-root'
    }
    $temporaryRoot = $temporaryItem.FullName.TrimEnd('\')
    $moduleRoots = @($pwshModuleRoot, $systemModuleRoot, $sharedPwshModuleRoot) |
        Where-Object { Test-Path -LiteralPath $_ -PathType Container }
    $environment = [ordered]@{
        OS = 'Windows_NT'
        SystemRoot = $systemRoot
        WINDIR = $systemRoot
        ComSpec = Join-Path $systemRoot 'System32/cmd.exe'
        PATHEXT = '.COM;.EXE;.BAT;.CMD'
        ProgramData = [Environment]::GetFolderPath(
            [Environment+SpecialFolder]::CommonApplicationData
        )
        ProgramFiles = $programFiles
        'ProgramFiles(x86)' = $programFilesX86
        CommonProgramFiles = $commonProgramFiles
        'CommonProgramFiles(x86)' = $commonProgramFilesX86
        PSModulePath = $moduleRoots -join ';'
        TEMP = $temporaryRoot
        TMP = $temporaryRoot
        WindowsSDKVersion = [string]$Captured.WindowsSDKVersion
        INCLUDE = [string]$Captured.INCLUDE
        LIB = [string]$Captured.LIB
        LIBPATH = [string]$Captured.LIBPATH
    }
    foreach ($entry in @([Environment]::GetEnvironmentVariables('Process').Keys)) {
        [Environment]::SetEnvironmentVariable([string]$entry, $null, 'Process')
    }
    foreach ($entry in $environment.GetEnumerator()) {
        if ([string]::IsNullOrWhiteSpace([string]$entry.Value) -or
            [string]$entry.Value -match "`0|`r|`n") {
            throw 'broker-environment-value'
        }
        [Environment]::SetEnvironmentVariable(
            [string]$entry.Key,
            [string]$entry.Value,
            'Process'
        )
    }
}

function Add-BoundedDiagnostic {
    param(
        [Parameter(Mandatory = $true)]
        [System.Text.StringBuilder]$Builder,

        [AllowNull()]
        [object]$Value
    )

    $text = if ($null -eq $Value) { '' } else { [string]$Value }
    $text = ($text -replace '[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]+', ' ').TrimEnd()
    if ($text.Length -eq 0 -or $Builder.Length -ge $maximumDiagnosticCharacters) {
        return
    }
    $remaining = $maximumDiagnosticCharacters - $Builder.Length
    if ($text.Length -gt $remaining) {
        $text = $text.Substring(0, $remaining)
        if ($text.Length -ne 0 -and
            [char]::IsHighSurrogate($text[$text.Length - 1])) {
            $text = $text.Substring(0, $text.Length - 1)
        }
    }
    [void]$Builder.AppendLine($text)
}

function New-ProtectedBrokerPipe {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [Parameter(Mandatory = $true)]
        [System.Security.Principal.SecurityIdentifier]$Owner
    )

    $system = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-18')
    $security = [System.IO.Pipes.PipeSecurity]::new()
    $security.SetOwner($Owner)
    $security.SetAccessRuleProtection($true, $false)
    foreach ($allowed in @($Owner, $system)) {
        $security.AddAccessRule([System.IO.Pipes.PipeAccessRule]::new(
            $allowed,
            [System.IO.Pipes.PipeAccessRights]::FullControl,
            [System.Security.AccessControl.AccessControlType]::Allow
        ))
    }
    $pipe = [System.IO.Pipes.NamedPipeServerStreamAcl]::Create(
        $Name,
        [System.IO.Pipes.PipeDirection]::InOut,
        1,
        [System.IO.Pipes.PipeTransmissionMode]::Byte,
        [System.IO.Pipes.PipeOptions]::Asynchronous,
        4096,
        4096,
        $security,
        [System.IO.HandleInheritability]::None,
        [System.IO.Pipes.PipeAccessRights]::ChangePermissions
    )
    try {
        $effective = [System.IO.Pipes.PipesAclExtensions]::GetAccessControl($pipe)
        $rules = @($effective.GetAccessRules(
            $true,
            $false,
            [System.Security.Principal.SecurityIdentifier]
        ))
        $ruleSids = @($rules | ForEach-Object { $_.IdentityReference.Value } | Sort-Object)
        if (-not $effective.AreAccessRulesProtected -or
            $effective.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -cne
                $Owner.Value -or
            $rules.Count -ne 2 -or
            ($ruleSids -join ',') -cne
                ((@($Owner.Value, $system.Value) | Sort-Object) -join ',') -or
            @($rules | Where-Object {
                $_.AccessControlType -ne
                    [System.Security.AccessControl.AccessControlType]::Allow -or
                (($_.PipeAccessRights -band
                    [System.IO.Pipes.PipeAccessRights]::FullControl) -ne
                    [System.IO.Pipes.PipeAccessRights]::FullControl)
            }).Count -ne 0) {
            throw 'broker-pipe-security'
        }
        return $pipe
    }
    catch {
        $pipe.Dispose()
        throw
    }
}

function Invoke-BrokerChild {
    if ($BrokerJobName -notmatch $brokerJobPattern -or
        $PipeName -notmatch $pipePattern -or
        [DateTime]::UtcNow.Ticks -ge $BootstrapDeadlineUtcTicks) {
        throw 'broker-child-bootstrap-identity'
    }

    $bootstrapDeadline = [DateTime]::new(
        $BootstrapDeadlineUtcTicks,
        [DateTimeKind]::Utc
    )
    $failureDeadline = $bootstrapDeadline
    $failurePhase = 'connect'
    $requestId = ''
    $clientOptions = [System.IO.Pipes.PipeOptions]::Asynchronous
    $pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
        '.',
        $PipeName,
        [System.IO.Pipes.PipeDirection]::InOut,
        $clientOptions
    )
    try {
        $remainingMilliseconds = [Math]::Max(
            1,
            [Math]::Min(
                [int]::MaxValue,
                [int](
                    ([DateTime]::new($BootstrapDeadlineUtcTicks, [DateTimeKind]::Utc) -
                        [DateTime]::UtcNow).TotalMilliseconds
                )
            )
        )
        $pipe.Connect($remainingMilliseconds)
        $failurePhase = 'parent-authentication'
        if ([ProjectAtlasWindowsRunnerJob]::GetPipeServerProcessId($pipe) -ne
            $ParentProcessId) {
            throw 'broker-pipe-server-process'
        }
        $parent = Get-Process -Id $ParentProcessId -ErrorAction Stop
        try {
            if ($parent.HasExited -or
                $parent.StartTime.ToUniversalTime().Ticks -ne $ParentStartTimeUtcTicks -or
                $parent.SessionId -ne $ParentSessionId -or
                -not $parent.MainModule.FileName.Equals(
                    [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName,
                    [System.StringComparison]::OrdinalIgnoreCase
                ) -or
                [ProjectAtlasWindowsRunnerJob]::GetProcessSid($ParentProcessId) -cne
                    [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value) {
                throw 'broker-parent-identity'
            }
        }
        finally {
            $parent.Dispose()
        }

        if ($BootstrapTestFault -eq 'hold-before-join') {
            Write-BrokerFrame `
                -Stream $pipe `
                -Deadline $bootstrapDeadline `
                -Value @{
                    schema = 1
                    kind = 'bootstrap-fault'
                    process_id = $PID
                    target_kind = $BrokerTargetKind
                }
            [System.Threading.Thread]::Sleep([System.Threading.Timeout]::Infinite)
        }

        $failurePhase = 'job-admission'
        [ProjectAtlasWindowsRunnerJob]::Join($BrokerJobName)
        Write-BrokerFrame `
            -Stream $pipe `
            -Deadline $bootstrapDeadline `
            -Value @{
                schema = 1
                kind = 'ready'
                process_id = $PID
                target_kind = $BrokerTargetKind
            }
        $failurePhase = 'request-admission'
        $request = Read-BrokerFrame -Stream $pipe -Deadline $bootstrapDeadline
        if ($request.ContainsKey('request_id') -and
            [string]$request.request_id -match '\A[0-9a-f]{32}\z') {
            $requestId = [string]$request.request_id
        }
        if ($request.Count -ne 7 -or
            [int]$request.schema -ne 1 -or
            [string]$request.kind -cne 'admit' -or
            [string]$request.request_id -notmatch '\A[0-9a-f]{32}\z' -or
            [string]$request.target_kind -cne $BrokerTargetKind -or
            [long]$request.target_deadline_utc_ticks -le [DateTime]::UtcNow.Ticks -or
            $request.parameters -isnot [hashtable] -or
            $request.environment -isnot [hashtable]) {
            throw 'broker-admission-request'
        }
        $failureDeadline = [DateTime]::new(
            [long]$request.target_deadline_utc_ticks,
            [DateTimeKind]::Utc
        )
        $targetParameters = [hashtable]$request.parameters
        Assert-TargetParameters -Kind $BrokerTargetKind -Parameters $targetParameters
        $isStaticRecovery = $BrokerTargetKind -eq 'recovery' -and
            $targetParameters.ContainsKey('StaticOnly') -and
            [bool]$targetParameters.StaticOnly
        if ($isStaticRecovery) {
            if (([hashtable]$request.environment).Count -ne 0) {
                throw 'broker-static-recovery-environment'
            }
        }
        else {
            $failurePhase = 'environment-admission'
            $targetTemporaryRoot = if ($BrokerTargetKind -eq 'construction') {
                [string]$targetParameters.TemporaryDirectory
            }
            else {
                [string]$targetParameters.RunnerTemporaryRoot
            }
            Set-BrokerEnvironment `
                -Captured ([hashtable]$request.environment) `
                -TemporaryRoot $targetTemporaryRoot
        }

        $failurePhase = 'target-admission'
        if ($BrokerTargetKind -eq 'construction') {
            $targetParameters.BrokerJobName = $BrokerJobName
            $targetPath = Join-Path $PSScriptRoot 'invoke-parser-pack-windows-construction.ps1'
        }
        elseif ($BrokerTargetKind -eq 'recovery') {
            $targetParameters.BrokerJobName = $BrokerJobName
            if (-not $isStaticRecovery) {
                $nestedParameters = [hashtable]$targetParameters.ConstructionParameters
                $nestedParameters.BrokerJobName = $BrokerJobName
                $targetParameters.ConstructionParameters = $nestedParameters
            }
            $targetPath = Join-Path $PSScriptRoot 'test-parser-pack-windows-recovery.ps1'
        }

        $targetFailure = $null
        $diagnostics = [System.Text.StringBuilder]::new()
        $failurePhase = 'target-execution'
        try {
            $targetItem = Get-Item -LiteralPath $targetPath -Force
            if ($targetItem.PSIsContainer -or
                (($targetItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) -or
                $targetItem.DirectoryName -cne $PSScriptRoot) {
                throw 'broker-target-path'
            }
            & $targetItem.FullName @targetParameters *>&1 |
                ForEach-Object {
                    Add-BoundedDiagnostic -Builder $diagnostics -Value $_
                }
        }
        catch {
            $targetFailure = $_.Exception
            Add-BoundedDiagnostic -Builder $diagnostics -Value $targetFailure.ToString()
        }

        $resultDeadline = [DateTime]::new(
            [long]$request.target_deadline_utc_ticks,
            [DateTimeKind]::Utc
        )
        Write-BrokerFrame `
            -Stream $pipe `
            -Deadline $resultDeadline `
            -Value @{
                schema = 1
                kind = 'result'
                request_id = [string]$request.request_id
                success = ($null -eq $targetFailure)
                diagnostics = $diagnostics.ToString()
            }
        if ($null -ne $targetFailure) {
            exit 1
        }
        exit 0
    }
    catch {
        $childFailure = $_.Exception
        if ($pipe.IsConnected) {
            $failureDiagnostics = [System.Text.StringBuilder]::new()
            Add-BoundedDiagnostic `
                -Builder $failureDiagnostics `
                -Value $childFailure.ToString()
            try {
                Write-BrokerFrame `
                    -Stream $pipe `
                    -Deadline $failureDeadline `
                    -Value @{
                        schema = 1
                        kind = 'broker-failure'
                        phase = $failurePhase
                        request_id = $requestId
                        diagnostics = $failureDiagnostics.ToString()
                    }
            }
            catch {
            }
        }
        exit 1
    }
    finally {
        $pipe.Dispose()
    }
}

function Invoke-BrokerParent {
    if ($env:OS -ne 'Windows_NT' -or -not [Environment]::Is64BitProcess) {
        throw 'Windows runner Job broker requires 64-bit Windows.'
    }
    Assert-TargetParameters -Kind $TargetKind -Parameters $TargetParameters
    $isStaticRecovery = $TargetKind -eq 'recovery' -and
        $TargetParameters.ContainsKey('StaticOnly') -and
        [bool]$TargetParameters.StaticOnly
    $capturedEnvironment = if ($isStaticRecovery) {
        @{}
    }
    else {
        Get-BrokerEnvironment
    }

    $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    $ownerSid = $identity.User.Value
    $suffix = [Guid]::NewGuid().ToString('N')
    $jobName = "Global\ProjectAtlasParserPackBroker-$suffix"
    $pipeName = "ProjectAtlasParserPackBroker-$suffix"
    $scriptItem = Get-Item -LiteralPath $PSCommandPath -Force
    if ($scriptItem.PSIsContainer -or
        (($scriptItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw 'broker-script-path'
    }
    $pwsh = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
    if (-not $isStaticRecovery) {
        $requestedPwsh = if ($TargetKind -eq 'construction') {
            [string]$TargetParameters.PwshPath
        }
        else {
            [string]([hashtable]$TargetParameters.ConstructionParameters).PwshPath
        }
        $requestedPwshItem = Get-Item -LiteralPath ([System.IO.Path]::GetFullPath($requestedPwsh)) -Force
        if ($requestedPwshItem.PSIsContainer -or
            (($requestedPwshItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) -or
            -not $requestedPwshItem.FullName.Equals(
                $pwsh,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            throw 'broker-powershell-runtime-identity'
        }
    }
    $parentStartTicks = [System.Diagnostics.Process]::GetCurrentProcess().StartTime.
        ToUniversalTime().Ticks
    $parentSessionId = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
    $bootstrapDeadline = [DateTime]::UtcNow.AddSeconds($bootstrapTimeoutSeconds)
    $targetDeadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)

    $job = $null
    $pipe = $null
    $process = $null
    $processHandle = [IntPtr]::Zero
    $operationFailure = $null
    $cleanupFailures = [System.Collections.Generic.List[System.Exception]]::new()
    $result = $null
    try {
        $job = [ProjectAtlasWindowsRunnerJob]::Create($jobName, $ownerSid)
        $pipe = New-ProtectedBrokerPipe -Name $pipeName -Owner $identity.User
        $childArguments = @(
            '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
            '-File', $scriptItem.FullName,
            '-BrokerChild',
            '-BrokerJobName', $jobName,
            '-PipeName', $pipeName,
            '-ParentProcessId', $PID,
            '-ParentStartTimeUtcTicks', $parentStartTicks,
            '-ParentSessionId', $parentSessionId,
            '-BrokerTargetKind', $TargetKind,
            '-BootstrapDeadlineUtcTicks', $bootstrapDeadline.Ticks,
            '-BootstrapTestFault', $BootstrapTestFault
        )
        $commandLine = [ProjectAtlasWindowsRunnerJob]::BuildCommandLine(
            $pwsh,
            [string[]]$childArguments
        )
        $startup = New-CimInstance `
            -ClassName Win32_ProcessStartup `
            -Property @{
                ShowWindow = [uint16]0
                CreateFlags = [uint32](0x01000000 -bor 0x08000000)
            } `
            -ClientOnly
        $launchStartTicks = [DateTime]::UtcNow.Ticks
        $created = Invoke-CimMethod `
            -ClassName Win32_Process `
            -MethodName Create `
            -Arguments @{
                CommandLine = $commandLine
                CurrentDirectory = $scriptItem.DirectoryName
                ProcessStartupInformation = $startup
            } `
            -OperationTimeoutSec $bootstrapTimeoutSeconds
        if ([uint32]$created.ReturnValue -ne 0 -or [uint32]$created.ProcessId -eq 0) {
            throw "broker-wmi-create-$([uint32]$created.ReturnValue)"
        }
        $processId = [int]$created.ProcessId
        $reportedProcesses = @(
            Get-CimInstance `
                -ClassName Win32_Process `
                -Filter "ProcessId = $processId" `
                -OperationTimeoutSec $bootstrapTimeoutSeconds
        )
        if ($reportedProcesses.Count -ne 1 -or
            [string]$reportedProcesses[0].CommandLine -cne $commandLine -or
            $null -eq $reportedProcesses[0].CreationDate -or
            ([DateTime]$reportedProcesses[0].CreationDate).ToUniversalTime().Ticks -lt
                $launchStartTicks) {
            throw 'broker-wmi-process-identity'
        }
        $process = Get-Process -Id $processId -ErrorAction Stop
        [ProjectAtlasWindowsRunnerJob]::ValidateProcessIdentity(
            $process,
            $pwsh,
            $ownerSid,
            $launchStartTicks,
            $parentSessionId
        )
        $processHandle = $process.Handle

        $connectionSource = New-DeadlineCancellationSource -Deadline $bootstrapDeadline
        try {
            [void]$pipe.WaitForConnectionAsync($connectionSource.Token).
                GetAwaiter().GetResult()
        }
        finally {
            $connectionSource.Dispose()
        }
        $clientProcessId = [ProjectAtlasWindowsRunnerJob]::GetPipeClientProcessId($pipe)
        if ($clientProcessId -ne $processId) {
            throw 'broker-wmi-pipe-process-mismatch'
        }
        $ready = Read-BrokerFrame -Stream $pipe -Deadline $bootstrapDeadline
        if ([string]$ready.kind -ceq 'broker-failure') {
            if ($ready.Count -ne 5 -or
                [int]$ready.schema -ne 1 -or
                [string]$ready.phase -cnotin @(
                    'parent-authentication', 'job-admission'
                ) -or
                -not [string]::IsNullOrEmpty([string]$ready.request_id) -or
                [string]$ready.diagnostics -match "`0" -or
                ([string]$ready.diagnostics).Length -gt
                    $maximumDiagnosticCharacters + 2) {
                throw 'broker-bootstrap-failure-receipt'
            }
            throw "broker-child-$([string]$ready.phase)-failed: $([string]$ready.diagnostics)"
        }
        if ($ready.Count -ne 4 -or
            [int]$ready.schema -ne 1 -or
            [string]$ready.kind -cne 'ready' -or
            [int]$ready.process_id -ne $processId -or
            [string]$ready.target_kind -cne $TargetKind) {
            throw 'broker-ready-receipt'
        }
        $job.ValidateProcessMembership($process)
        if ($job.ActiveProcessCount -ne 1) {
            throw 'broker-job-admission-process-count'
        }
        Write-BrokerFrame `
            -Stream $pipe `
            -Deadline $bootstrapDeadline `
            -Value @{
                schema = 1
                kind = 'admit'
                request_id = $suffix
                target_kind = $TargetKind
                target_deadline_utc_ticks = $targetDeadline.Ticks
                parameters = $TargetParameters
                environment = $capturedEnvironment
            }
        $result = Read-BrokerFrame -Stream $pipe -Deadline $targetDeadline
        if ([string]$result.kind -ceq 'broker-failure') {
            if ($result.Count -ne 5 -or
                [int]$result.schema -ne 1 -or
                [string]$result.phase -cnotin @(
                    'request-admission', 'environment-admission',
                    'target-admission', 'target-execution'
                ) -or
                [string]$result.request_id -cne $suffix -or
                [string]$result.diagnostics -match "`0" -or
                ([string]$result.diagnostics).Length -gt
                    $maximumDiagnosticCharacters + 2) {
                throw 'broker-target-failure-receipt'
            }
            throw "broker-child-$([string]$result.phase)-failed: $([string]$result.diagnostics)"
        }
        if ($result.Count -ne 5 -or
            [int]$result.schema -ne 1 -or
            [string]$result.kind -cne 'result' -or
            [string]$result.request_id -cne $suffix -or
            $result.success -isnot [bool] -or
            [string]$result.diagnostics -match "`0" -or
            ([string]$result.diagnostics).Length -gt $maximumDiagnosticCharacters + 2) {
            throw 'broker-result-receipt'
        }
        $remainingMilliseconds = [Math]::Max(
            1,
            [Math]::Min(
                [int]::MaxValue,
                [int](($targetDeadline - [DateTime]::UtcNow).TotalMilliseconds)
            )
        )
        if (-not $process.WaitForExit($remainingMilliseconds)) {
            throw 'broker-process-exit-timeout'
        }
        $brokerExitCode = [ProjectAtlasWindowsRunnerJob]::GetProcessExitCode($processHandle)
        if ([bool]$result.success -ne ($brokerExitCode -eq 0)) {
            throw "broker-result-exit-mismatch-$brokerExitCode-$([bool]$result.success)"
        }
        if (-not [bool]$result.success) {
            throw "broker-target-failed: $([string]$result.diagnostics)"
        }
        if ($job.ActiveProcessCount -ne 0) {
            throw 'broker-job-retained-process'
        }
    }
    catch {
        $operationFailure = $_.Exception
    }
    finally {
        if ($null -ne $job) {
            if ($null -ne $operationFailure) {
                try {
                    if ($processHandle -ne [IntPtr]::Zero) {
                        [ProjectAtlasWindowsRunnerJob]::TerminateExactProcess(
                            $processHandle,
                            125
                        )
                    }
                }
                catch {
                    $cleanupFailures.Add($_.Exception)
                }
                try {
                    $job.Terminate(125)
                }
                catch {
                    $cleanupFailures.Add($_.Exception)
                }
            }
            try {
                $job.Dispose()
                $job = $null
            }
            catch {
                $cleanupFailures.Add($_.Exception)
            }
        }
        if ($null -ne $process) {
            try {
                if (-not $process.HasExited -and -not $process.WaitForExit(5000)) {
                    throw 'broker-process-reap-timeout'
                }
            }
            catch {
                $cleanupFailures.Add($_.Exception)
            }
            finally {
                $process.Dispose()
            }
        }
        if ($null -ne $pipe) {
            try {
                $pipe.Dispose()
            }
            catch {
                $cleanupFailures.Add($_.Exception)
            }
        }
    }
    if ($null -ne $operationFailure) {
        if ($cleanupFailures.Count -ne 0) {
            throw [System.AggregateException]::new(
                'Windows runner Job broker operation and cleanup failed.',
                @($operationFailure) + @($cleanupFailures)
            )
        }
        throw $operationFailure
    }
    if ($cleanupFailures.Count -ne 0) {
        throw [System.AggregateException]::new(
            'Windows runner Job broker cleanup failed.',
            @($cleanupFailures)
        )
    }
    if (-not [string]::IsNullOrWhiteSpace([string]$result.diagnostics)) {
        Write-Output ([string]$result.diagnostics).TrimEnd()
    }
}

if ($BrokerChild) {
    Invoke-BrokerChild
}
else {
    Invoke-BrokerParent
}
