[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet(
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc"
    )]
    [string]$Target,

    [Parameter(Mandatory = $true)]
    [string]$SourceRoot,

    [Parameter(Mandatory = $true)]
    [string]$InputDirectory,

    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [Parameter(Mandatory = $true)]
    [string]$ProjectAtlasRevision,

    [Parameter(Mandatory = $true)]
    [string]$CargoPackageVersion,

    [Parameter(Mandatory = $true)]
    [string]$IntendedReleaseVersion,

    [Parameter(Mandatory = $true)]
    [string]$CargoLockSha256,

    [Parameter(Mandatory = $true)]
    [string]$RustcRelease,

    [Parameter(Mandatory = $true)]
    [string]$RustcCommitHash,

    [Parameter(Mandatory = $true)]
    [ValidateSet(
        "linux-network-namespace",
        "windows-principal-firewall"
    )]
    [string]$NetworkIsolation,

    [Parameter(Mandatory = $true)]
    [string]$ResolverAddress,

    [ValidatePattern('\A(?:|Local\\ProjectAtlasParserPack-[0-9a-f]{32})\z')]
    [string]$SeededSemaphoreName = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$targetIsolation = @{
    "x86_64-unknown-linux-gnu" = "linux-network-namespace"
    "x86_64-pc-windows-msvc" = "windows-principal-firewall"
}
$sourceRevisionPattern = '\A[0-9a-f]{40}\z'
$sha256Pattern = '\A[0-9a-f]{64}\z'
$rustcReleasePattern = '\A[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?\z'
$commandDiagnosticTailBytes = 24 * 1024
$constructionDiagnosticMaxBytes = 64 * 1024
$reusableCargoTargetMaxEntries = 200000
$reusableCargoTargetMaxBytes = [uint64](8GB)

function Get-CanonicalDirectory {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [string]$Role
    )

    $resolved = [System.IO.Path]::GetFullPath($Path)
    $item = Get-Item -LiteralPath $resolved -Force
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

    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or
        (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) -or
        $item.Length -le 0) {
        throw "$Role must be one non-empty regular file."
    }
    return $item.FullName
}

function Assert-ReusableCargoTarget {
    param(
        [Parameter(Mandatory = $true)]
        [string]$OutputDirectory,

        [Parameter(Mandatory = $true)]
        [string]$BuildDirectory,

        [Parameter(Mandatory = $true)]
        [ValidateRange(1, 1000000)]
        [int]$MaximumEntries,

        [Parameter(Mandatory = $true)]
        [ValidateRange(1, [long]::MaxValue)]
        [long]$MaximumBytes
    )

    $expected = [System.IO.Path]::GetFullPath(
        [System.IO.Path]::Combine($OutputDirectory, "build")
    )
    $candidate = [System.IO.Path]::GetFullPath($BuildDirectory)
    $comparison = if ($IsWindows) {
        [System.StringComparison]::OrdinalIgnoreCase
    }
    else {
        [System.StringComparison]::Ordinal
    }
    if (-not $candidate.Equals($expected, $comparison)) {
        throw "Reusable Cargo target is outside the fixed construction output."
    }

    $root = Get-Item -LiteralPath $candidate -Force
    if (-not $root.PSIsContainer -or
        (($root.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "Reusable Cargo target root is not one direct non-reparse directory."
    }
    $prefix = "$($root.FullName.TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    ))$([System.IO.Path]::DirectorySeparatorChar)"
    $entryCount = 0
    [uint64]$totalBytes = 0
    foreach ($entry in (Get-ChildItem -LiteralPath $root.FullName -Force -Recurse)) {
        $entryCount++
        if ($entryCount -gt $MaximumEntries) {
            throw "Reusable Cargo target exceeds its entry limit."
        }
        if (-not $entry.FullName.StartsWith($prefix, $comparison) -or
            (($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
            throw "Reusable Cargo target contains path indirection."
        }
        if ($entry.PSIsContainer) {
            continue
        }
        if (-not ($entry -is [System.IO.FileInfo])) {
            throw "Reusable Cargo target contains an unexpected entry type."
        }
        $length = [uint64]$entry.Length
        if ($length -gt ([uint64]$MaximumBytes - $totalBytes)) {
            throw "Reusable Cargo target exceeds its byte limit."
        }
        $totalBytes += $length
    }

    return [pscustomobject]@{
        entries = $entryCount
        bytes = $totalBytes
    }
}

function Initialize-ReusableCargoTarget {
    param(
        [Parameter(Mandatory = $true)]
        [string]$OutputDirectory,

        [Parameter(Mandatory = $true)]
        [string]$BuildDirectory,

        [Parameter(Mandatory = $true)]
        [ValidateRange(1, 1000000)]
        [int]$MaximumEntries,

        [Parameter(Mandatory = $true)]
        [ValidateRange(1, [long]::MaxValue)]
        [long]$MaximumBytes
    )

    $existing = Get-Item -LiteralPath $BuildDirectory -Force -ErrorAction SilentlyContinue
    if ($null -eq $existing) {
        [System.IO.Directory]::CreateDirectory($BuildDirectory) | Out-Null
        return [pscustomobject]@{
            disposition = "miss"
            entries = 0
            bytes = 0
        }
    }

    try {
        $metrics = Assert-ReusableCargoTarget `
            -OutputDirectory $OutputDirectory `
            -BuildDirectory $BuildDirectory `
            -MaximumEntries $MaximumEntries `
            -MaximumBytes $MaximumBytes
        return [pscustomobject]@{
            disposition = "hit"
            entries = $metrics.entries
            bytes = $metrics.bytes
        }
    }
    catch {
        $rejection = [string]$_.Exception.Message
        foreach ($privatePath in @($BuildDirectory, $OutputDirectory)) {
            $rejection = $rejection.Replace($privatePath, "<private>")
        }
        if ($rejection.Length -gt 512) {
            $rejection = $rejection.Substring(0, 512)
        }
        $quarantine = [System.IO.Path]::Combine(
            $OutputDirectory,
            "rejected-build-$([guid]::NewGuid().ToString("N"))"
        )
        if ($existing.PSIsContainer) {
            [System.IO.Directory]::Move($BuildDirectory, $quarantine)
        }
        else {
            [System.IO.File]::Move($BuildDirectory, $quarantine)
        }
        Write-Warning "Reusable Cargo target rejected: $rejection"
        [System.IO.Directory]::CreateDirectory($BuildDirectory) | Out-Null
        return [pscustomobject]@{
            disposition = "rejected"
            entries = 0
            bytes = 0
        }
    }
}

function Add-BoundedDiagnosticTail {
    param(
        [Parameter(Mandatory = $true)]
        [byte[]]$Tail,

        [Parameter(Mandatory = $true)]
        [int]$CurrentLength,

        [Parameter(Mandatory = $true)]
        [byte[]]$Chunk,

        [Parameter(Mandatory = $true)]
        [int]$Count
    )

    if ($Count -ge $Tail.Length) {
        [System.Array]::Copy($Chunk, $Count - $Tail.Length, $Tail, 0, $Tail.Length)
        return $Tail.Length
    }
    $overflow = [Math]::Max(0, ($CurrentLength + $Count) - $Tail.Length)
    $retained = $CurrentLength - $overflow
    if ($retained -gt 0 -and $overflow -gt 0) {
        [System.Array]::Copy($Tail, $overflow, $Tail, 0, $retained)
    }
    [System.Array]::Copy($Chunk, 0, $Tail, $retained, $Count)
    return $retained + $Count
}

function Write-BoundedConstructionDiagnostic {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Role,

        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$StandardOutput,

        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$StandardError
    )

    $diagnostic =
        "role: $Role`nstdout tail:`n$StandardOutput`nstderr tail:`n$StandardError`n"
    foreach ($root in @($source, $inputs, $output)) {
        $diagnostic = $diagnostic.Replace($root, "<contained-root>")
    }
    $diagnosticBytes = [System.Text.UTF8Encoding]::new($false).GetBytes($diagnostic)
    if ($diagnosticBytes.Length -gt $constructionDiagnosticMaxBytes) {
        throw "Bounded construction diagnostic exceeded its byte limit."
    }
    [System.IO.File]::WriteAllBytes(
        $script:constructionDiagnosticPath,
        $diagnosticBytes
    )
}

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Executable,

        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,

        [Parameter(Mandatory = $true)]
        [string]$Role
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Executable
    $startInfo.UseShellExecute = $false
    $captureDiagnostic = $Target -eq "x86_64-pc-windows-msvc"
    $startInfo.RedirectStandardOutput = $captureDiagnostic
    $startInfo.RedirectStandardError = $captureDiagnostic
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }
    $stdoutTail = [byte[]]::new($commandDiagnosticTailBytes)
    $stderrTail = [byte[]]::new($commandDiagnosticTailBytes)
    $stdoutLength = 0
    $stderrLength = 0
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw "process-start"
        }
        if ($captureDiagnostic) {
            $stdoutBuffer = [byte[]]::new(4096)
            $stderrBuffer = [byte[]]::new(4096)
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
            while ($null -ne $stdoutRead -or $null -ne $stderrRead) {
                $readCompleted = $false
                if ($null -ne $stdoutRead -and $stdoutRead.IsCompleted) {
                    $count = $stdoutRead.GetAwaiter().GetResult()
                    if ($count -eq 0) {
                        $stdoutRead = $null
                    }
                    else {
                        $stdoutLength = Add-BoundedDiagnosticTail `
                            -Tail $stdoutTail `
                            -CurrentLength $stdoutLength `
                            -Chunk $stdoutBuffer `
                            -Count $count
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
                        $stderrLength = Add-BoundedDiagnosticTail `
                            -Tail $stderrTail `
                            -CurrentLength $stderrLength `
                            -Chunk $stderrBuffer `
                            -Count $count
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
        }
        $process.WaitForExit()
        $commandExitCode = $process.ExitCode
    }
    catch {
        try {
            Write-ConstructionStatus `
                -Stage $script:constructionStage `
                -State "failed" `
                -ExitCode 1
        }
        catch {
            # The parent rejects missing or malformed status and never emits
            # exception text from inside the contained process.
        }
        throw "$Role failed to start or wait."
    }
    finally {
        $process.Dispose()
    }
    if ($commandExitCode -ne 0) {
        if ($captureDiagnostic) {
            $ascii = [System.Text.Encoding]::ASCII
            $stdout = $ascii.GetString($stdoutTail, 0, $stdoutLength)
            $stderr = $ascii.GetString($stderrTail, 0, $stderrLength)
            Write-BoundedConstructionDiagnostic `
                -Role $Role `
                -StandardOutput $stdout `
                -StandardError $stderr
        }
        try {
            Write-ConstructionStatus `
                -Stage $script:constructionStage `
                -State "failed" `
                -ExitCode $commandExitCode
        }
        catch {
            # The parent rejects missing or malformed status and never emits
            # exception text from inside the contained process.
        }
        throw "$Role failed with exit code $commandExitCode."
    }
}

function Write-ConstructionStatus {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet(
            "validate-inputs",
            "network-denial-canaries",
            "output-preparation",
            "cargo-jobserver-bootstrap",
            "reusable-cargo-target-clean",
            "optional-parser-worker-build",
            "release-tool-builds",
            "runtime-containment-broker-build",
            "artifact-input-validation",
            "parallel-artifact-construction",
            "deterministic-archive-comparison",
            "publication",
            "reusable-cargo-target-validation"
        )]
        [string]$Stage,

        [Parameter(Mandatory = $true)]
        [ValidateSet("running", "failed")]
        [string]$State,

        [Nullable[int]]$ExitCode
    )

    if (($State -eq "running" -and $null -ne $ExitCode) -or
        ($State -eq "failed" -and ($null -eq $ExitCode -or $ExitCode -eq 0))) {
        throw "Construction status has an invalid exit-code state."
    }
    $status = [ordered]@{
        schema_version = 1
        stage = $Stage
        state = $State
        exit_code = if ($null -eq $ExitCode) { $null } else { [int]$ExitCode }
    }
    $json = ($status | ConvertTo-Json -Compress) + "`n"
    $encoding = [System.Text.UTF8Encoding]::new($false)
    if ($encoding.GetByteCount($json) -gt 1024) {
        throw "Construction status exceeds its byte bound."
    }
    $temporaryPath = "$script:constructionStatusPath.tmp"
    [System.IO.File]::WriteAllText($temporaryPath, $json, $encoding)
    [System.IO.File]::Move($temporaryPath, $script:constructionStatusPath, $true)
    if ($State -eq "failed") {
        $script:constructionFailureRecorded = $true
        $script:constructionFailureExitCode = [int]$ExitCode
    }
}

function Open-ContainedCargoJobserver {
    param(
        [Parameter(Mandatory = $true)]
        [System.Security.Principal.SecurityIdentifier]$Sid,

        [Parameter(Mandatory = $true)]
        [ValidatePattern('\ALocal\\ProjectAtlasParserPack-[0-9a-f]{32}\z')]
        [string]$Name
    )

    $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    if ($null -eq $identity.Owner -or
        -not [string]::Equals(
            $identity.Owner.Value,
            $Sid.Value,
            [System.StringComparison]::Ordinal
        )) {
        throw "Contained Cargo jobserver requires the construction SID as the token default owner."
    }

    if ($null -eq ('ProjectAtlasCargoJobserverNative' -as [type])) {
        $nativeSource = @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class ProjectAtlasCargoJobserverNative
{
    private const uint SynchronizeAndModify = 0x00100002;
    private const int ErrorAlreadyExists = 183;

    [DllImport(
        "kernel32.dll",
        CharSet = CharSet.Unicode,
        SetLastError = true,
        EntryPoint = "OpenSemaphoreW")]
    private static extern IntPtr OpenSemaphore(
        uint desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool inheritHandle,
        string name);

    [DllImport(
        "kernel32.dll",
        CharSet = CharSet.Unicode,
        SetLastError = true,
        EntryPoint = "CreateSemaphoreExW")]
    private static extern IntPtr ProbeExistingSemaphore(
        IntPtr securityAttributes,
        int initialCount,
        int maximumCount,
        string name,
        uint flags,
        uint desiredAccess);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    public static SafeWaitHandle OpenExisting(string name)
    {
        if (string.IsNullOrEmpty(name))
        {
            throw new ArgumentException("contained-cargo-jobserver-input");
        }
        IntPtr rawHandle = OpenSemaphore(SynchronizeAndModify, false, name);
        if (rawHandle == IntPtr.Zero)
        {
            throw new Win32Exception(
                Marshal.GetLastWin32Error(),
                "open-contained-cargo-jobserver");
        }
        return new SafeWaitHandle(rawHandle, true);
    }

    public static void RequireExistingObject(string name)
    {
        IntPtr handle = ProbeExistingSemaphore(
            IntPtr.Zero,
            1,
            1,
            name,
            0,
            SynchronizeAndModify);
        int createError = Marshal.GetLastWin32Error();
        if (handle == IntPtr.Zero)
        {
            throw new Win32Exception(
                createError,
                "probe-contained-cargo-jobserver");
        }
        if (!CloseHandle(handle))
        {
            throw new Win32Exception(
                Marshal.GetLastWin32Error(),
                "close-contained-cargo-jobserver-probe");
        }
        if (createError != ErrorAlreadyExists)
        {
            throw new InvalidOperationException(
                "contained-cargo-jobserver-seed-missing");
        }
    }
}
'@
        Add-Type -TypeDefinition $nativeSource -Language CSharp -ErrorAction Stop
    }

    try {
        $semaphore = [ProjectAtlasCargoJobserverNative]::OpenExisting($Name)
        [ProjectAtlasCargoJobserverNative]::RequireExistingObject($Name)
    }
    catch {
        throw "Contained Cargo jobserver seed could not be opened."
    }
    return $semaphore
}

function Invoke-ContainedCargoJobserverCanary {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Pwsh,

        [Parameter(Mandatory = $true)]
        [ValidatePattern('\ALocal\\ProjectAtlasParserPack-[0-9a-f]{32}\z')]
        [string]$Name,

        [Parameter(Mandatory = $true)]
        [System.Security.Principal.SecurityIdentifier]$Sid,

        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $canarySource = @'
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('\ALocal\\ProjectAtlasParserPack-[0-9a-f]{32}\z')]
    [string]$Name,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedSid
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
if ([System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value -ne $ExpectedSid) {
    exit 31
}
$nativeSource = @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class ProjectAtlasCargoJobserverCanary
{
    private const uint SynchronizeAndModify = 0x00100002;

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
        IntPtr handle = OpenSemaphore(SynchronizeAndModify, false, name);
        if (handle == IntPtr.Zero)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "open-contained-cargo-jobserver");
        }
        if (!CloseHandle(handle))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "close-contained-cargo-jobserver");
        }
    }
}
"@
Add-Type -TypeDefinition $nativeSource -Language CSharp
[ProjectAtlasCargoJobserverCanary]::OpenAndClose($Name)
exit 0
'@
    if ([System.IO.File]::Exists($Path) -or [System.IO.Directory]::Exists($Path)) {
        throw "Contained Cargo jobserver canary path already exists."
    }
    [System.IO.File]::WriteAllText(
        $Path,
        $canarySource,
        [System.Text.UTF8Encoding]::new($false)
    )
    try {
        Invoke-Checked `
            -Executable $Pwsh `
            -Arguments @(
                "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                "-File", $Path,
                "-Name", $Name,
                "-ExpectedSid", $Sid.Value
            ) `
            -Role "contained Cargo jobserver descendant canary"
    }
    finally {
        if ([System.IO.File]::Exists($Path)) {
            [System.IO.File]::Delete($Path)
        }
    }
}

function Close-ContainedCargoJobserver {
    if ($null -ne $script:constructionJobserver) {
        $script:constructionJobserver.Dispose()
        $script:constructionJobserver = $null
    }
    if ($Target -eq "x86_64-pc-windows-msvc") {
        Remove-Item -LiteralPath Env:CARGO_MAKEFLAGS -ErrorAction SilentlyContinue
    }
    if (-not [string]::IsNullOrEmpty([string]$script:constructionJobserverCanaryPath) -and
        [System.IO.File]::Exists($script:constructionJobserverCanaryPath)) {
        [System.IO.File]::Delete($script:constructionJobserverCanaryPath)
    }
    $script:constructionJobserverCanaryPath = $null
}

function Assert-CargoConstructionEnvironment {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Target
    )

    if ($Target -eq "x86_64-pc-windows-msvc") {
        if ([string]$env:CARGO_BUILD_JOBS -cne "4" -or
            (Test-Path -LiteralPath Env:CARGO_MAKEFLAGS)) {
            throw "Windows construction must create its Cargo jobserver inside the contained child."
        }
    }
}

if ($targetIsolation[$Target] -ne $NetworkIsolation) {
    throw "NetworkIsolation does not match Target."
}
if ($ProjectAtlasRevision -notmatch $sourceRevisionPattern -or
    $CargoLockSha256 -notmatch $sha256Pattern -or
    $RustcCommitHash -notmatch $sourceRevisionPattern -or
    $RustcRelease -notmatch $rustcReleasePattern) {
    throw "Candidate identity contains a non-canonical revision, digest, or toolchain value."
}
if ($CargoPackageVersion -ne $IntendedReleaseVersion) {
    throw "Cargo package version must equal the intended parser-pack release version."
}
foreach ($forbiddenVariable in @("TSLP_LANGUAGES", "TSLP_ALLOW_FAILED_GRAMMARS")) {
    if (Test-Path -LiteralPath "Env:$forbiddenVariable") {
        throw "Forbidden optional-parser build override is present: $forbiddenVariable"
    }
}
Assert-CargoConstructionEnvironment -Target $Target
if ($Target -eq "x86_64-pc-windows-msvc" -and
    $SeededSemaphoreName -cnotmatch '\ALocal\\ProjectAtlasParserPack-[0-9a-f]{32}\z') {
    throw "Windows construction requires one protected seeded Cargo jobserver."
}

$source = Get-CanonicalDirectory -Path $SourceRoot -Role "SourceRoot"
$inputs = Get-CanonicalDirectory -Path $InputDirectory -Role "InputDirectory"
$output = Get-CanonicalDirectory -Path $OutputDirectory -Role "OutputDirectory"
$script:constructionStatusPath = [System.IO.Path]::Combine(
    $output,
    "construction-status.json"
)
$script:constructionDiagnosticPath = [System.IO.Path]::Combine(
    $output,
    "construction-diagnostic.txt"
)
$script:constructionCacheDispositionPath = [System.IO.Path]::Combine(
    $output,
    "construction-cache-disposition.json"
)
$script:constructionStage = "validate-inputs"
$script:constructionFailureRecorded = $false
$script:constructionFailureExitCode = 1
$script:constructionJobserver = $null
$script:constructionJobserverName = $null
$script:constructionJobserverCanaryPath = $null
trap {
    try {
        Close-ContainedCargoJobserver
    }
    catch {
        # Process teardown still closes the exact process-owned semaphore handle.
    }
    if (-not $script:constructionFailureRecorded) {
        $failureExitCode = 1
        $lastNativeExit = Get-Variable -Name LASTEXITCODE -ValueOnly -ErrorAction SilentlyContinue
        if ($null -ne $lastNativeExit -and $lastNativeExit -gt 0) {
            $failureExitCode = [int]$lastNativeExit
        }
        try {
            Write-ConstructionStatus `
                -Stage $script:constructionStage `
                -State "failed" `
                -ExitCode $failureExitCode
        }
        catch {
            # The parent treats a missing or malformed marker as a generic
            # contained-construction failure and never emits exception text.
        }
    }
    exit $script:constructionFailureExitCode
}
Write-ConstructionStatus -Stage $script:constructionStage -State "running"
$networkCheck = Get-RegularFile `
    -Path ([System.IO.Path]::Combine(
        $source,
        ".github",
        "scripts",
        "check-parser-pack-network-boundary.ps1"
    )) `
    -Role "network boundary checker"
$processPath = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
$pwsh = Get-RegularFile -Path $processPath -Role "PowerShell runtime"
$script:constructionStage = "network-denial-canaries"
Write-ConstructionStatus -Stage $script:constructionStage -State "running"
Invoke-Checked `
    -Executable $pwsh `
    -Arguments @(
        "-NoProfile",
        "-File",
        $networkCheck,
        "-Mode",
        "require-denied",
        "-ResolverAddress",
        $ResolverAddress
    ) `
    -Role "contained network-denial canaries"

$cargoCommand = Get-Command cargo -CommandType Application -ErrorAction Stop
$cargo = Get-RegularFile -Path $cargoCommand.Source -Role "Cargo executable"
$buildDirectory = [System.IO.Path]::Combine($output, "build")
$workingDirectory = [System.IO.Path]::Combine($output, "work")
$publishDirectory = [System.IO.Path]::Combine($output, "publish")
$script:constructionStage = "output-preparation"
Write-ConstructionStatus -Stage $script:constructionStage -State "running"
$cargoTarget = Initialize-ReusableCargoTarget `
    -OutputDirectory $output `
    -BuildDirectory $buildDirectory `
    -MaximumEntries $reusableCargoTargetMaxEntries `
    -MaximumBytes $reusableCargoTargetMaxBytes
[System.IO.File]::WriteAllText(
    $script:constructionCacheDispositionPath,
    (($cargoTarget | ConvertTo-Json -Compress) + "`n"),
    [System.Text.UTF8Encoding]::new($false)
)
foreach ($directory in @($workingDirectory, $publishDirectory)) {
    if ([System.IO.Directory]::Exists($directory) -or [System.IO.File]::Exists($directory)) {
        throw "Contained construction output already exists: $directory"
    }
    [System.IO.Directory]::CreateDirectory($directory) | Out-Null
}

if ($Target -eq "x86_64-pc-windows-msvc") {
    $script:constructionStage = "cargo-jobserver-bootstrap"
    Write-ConstructionStatus -Stage $script:constructionStage -State "running"
    $constructionSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
    $script:constructionJobserverName = $SeededSemaphoreName
    $script:constructionJobserver = Open-ContainedCargoJobserver `
        -Sid $constructionSid `
        -Name $script:constructionJobserverName
    $env:CARGO_MAKEFLAGS =
        "-j --jobserver-fds=$($script:constructionJobserverName) --jobserver-auth=$($script:constructionJobserverName)"
    $script:constructionJobserverCanaryPath = [System.IO.Path]::Combine(
        $workingDirectory,
        "cargo-jobserver-canary.ps1"
    )
    Invoke-ContainedCargoJobserverCanary `
        -Pwsh $pwsh `
        -Name $script:constructionJobserverName `
        -Sid $constructionSid `
        -Path $script:constructionJobserverCanaryPath
    $script:constructionJobserverCanaryPath = $null
}

$env:CARGO_NET_OFFLINE = "true"
$env:CARGO_TARGET_DIR = $buildDirectory
$env:TSLP_OFFLINE = "1"
$env:TSLP_LINK_MODE = "dynamic"

$ownedPackages = @(
    "projectatlas-lints",
    "projectatlas-cli",
    "projectatlas-core",
    "projectatlas-db",
    "projectatlas-fs",
    "projectatlas-service",
    "projectatlas-symbols"
)
if ($cargoTarget.disposition -eq "hit") {
    $cleanArguments = @(
        "clean",
        "--frozen",
        "--release",
        "--target-dir",
        $buildDirectory
    )
    foreach ($package in $ownedPackages) {
        $cleanArguments += @("--package", $package)
    }
    $script:constructionStage = "reusable-cargo-target-clean"
    Write-ConstructionStatus -Stage $script:constructionStage -State "running"
    Invoke-Checked `
        -Executable $cargo `
        -Arguments $cleanArguments `
        -Role "ProjectAtlas candidate artifact cleanup before reuse"
}

$workerBuildArguments = @(
        "build",
        "--frozen",
        "--offline",
        "--release",
        "--package",
        "projectatlas-cli",
        "--bin",
        "projectatlas-parser-worker",
        "--features",
        "projectatlas-cli/optional-parser-worker"
    )
    if ($Target -eq "x86_64-unknown-linux-gnu") {
        # The untrusted grammar is loaded only after Landlock. Keep every
        # audit-allowed system runtime DSO eagerly mapped by the trusted worker
        # so post-containment loading needs read access only to the pack root.
        $workerBuildArguments[0] = "rustc"
        $workerBuildArguments += @(
            "--",
            "-Clink-arg=-Wl,--push-state,--no-as-needed",
            "-Clink-arg=-Wl,-l:libgcc_s.so.1",
            "-Clink-arg=-Wl,-l:libm.so.6",
            "-Clink-arg=-Wl,-l:libstdc++.so.6",
            "-Clink-arg=-Wl,--pop-state",
            "-Clink-arg=-Wl,-z,now",
            "-Clink-arg=-Wl,-z,relro"
        )
    }
    else {
        $workerBuildArguments += @(
            "--package",
            "projectatlas-core",
            "--example",
            "assemble_optional_parser_artifact",
            "--example",
            "optional_parser_pack_release"
        )
    }
    $script:constructionStage = "optional-parser-worker-build"
    Write-ConstructionStatus -Stage $script:constructionStage -State "running"
    Invoke-Checked `
        -Executable $cargo `
        -Arguments $workerBuildArguments `
        -Role "optional parser worker build"
    if ($Target -eq "x86_64-unknown-linux-gnu") {
        $script:constructionStage = "release-tool-builds"
        Write-ConstructionStatus -Stage $script:constructionStage -State "running"
        Invoke-Checked `
            -Executable $cargo `
            -Arguments @(
                "build",
                "--frozen",
                "--offline",
                "--release",
                "--package",
                "projectatlas-core",
                "--package",
                "projectatlas-cli",
                "--features",
                "projectatlas-cli/optional-parser-supervisor",
                "--example",
                "assemble_optional_parser_artifact",
                "--example",
                "optional_parser_pack_release"
            ) `
            -Role "parser-pack release tool builds"
    }

    if ($Target -eq "x86_64-pc-windows-msvc") {
        $script:constructionStage = "runtime-containment-broker-build"
        Write-ConstructionStatus -Stage $script:constructionStage -State "running"
        $brokerBuilder = Get-RegularFile `
            -Path ([System.IO.Path]::Combine(
                $source,
                ".github",
                "scripts",
                "build-parser-pack-runtime-containment.ps1"
            )) `
            -Role "runtime-containment broker builder"
        $brokerOutput = [System.IO.Path]::Combine(
            $buildDirectory,
            "release",
            "projectatlas-parser-containment.exe"
        )
        $windowsPowerShell = Get-RegularFile `
            -Path ([System.IO.Path]::Combine(
                $env:SystemRoot,
                "System32",
                "WindowsPowerShell",
                "v1.0",
                "powershell.exe"
            )) `
            -Role "Windows PowerShell broker compiler"
        Invoke-Checked `
            -Executable $windowsPowerShell `
            -Arguments @(
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy", "Bypass",
                "-File", $brokerBuilder,
                "-OutputPath", $brokerOutput
            ) `
            -Role "runtime-containment broker build and contract audit"
    }
$executableSuffix = ""
if ($Target -eq "x86_64-pc-windows-msvc") {
    $executableSuffix = ".exe"
}
$worker = Get-RegularFile `
    -Path ([System.IO.Path]::Combine(
        $buildDirectory,
        "release",
        "projectatlas-parser-worker$executableSuffix"
    )) `
    -Role "built parser worker"
$containmentBroker = "-"
if ($Target -eq "x86_64-pc-windows-msvc") {
    $containmentBroker = Get-RegularFile `
        -Path ([System.IO.Path]::Combine(
            $buildDirectory,
            "release",
            "projectatlas-parser-containment.exe"
        )) `
        -Role "built runtime-containment broker"
}
$assembler = Get-RegularFile `
    -Path ([System.IO.Path]::Combine(
        $buildDirectory,
        "release",
        "examples",
        "assemble_optional_parser_artifact$executableSuffix"
    )) `
    -Role "built artifact assembler"
$releaseTool = Get-RegularFile `
    -Path ([System.IO.Path]::Combine(
        $buildDirectory,
        "release",
        "examples",
        "optional_parser_pack_release$executableSuffix"
    )) `
    -Role "built release verifier"

$script:constructionStage = "artifact-input-validation"
Write-ConstructionStatus -Stage $script:constructionStage -State "running"
$contextPath = [System.IO.Path]::Combine($workingDirectory, "assembly-context.json")
$context = [ordered]@{
    candidate = [ordered]@{
        projectatlas_revision = $ProjectAtlasRevision
        cargo_package_version = $CargoPackageVersion
        intended_release_version = $IntendedReleaseVersion
        cargo_lock_sha256 = $CargoLockSha256
        rustc_release = $RustcRelease
        rustc_commit_hash = $RustcCommitHash
        source_state = "clean"
    }
    construction = [ordered]@{
        cargo_frozen = $true
        cargo_offline = $true
        dependency_offline = $true
        language_selector_absent = $true
        failed_grammar_override_absent = $true
        network_denial = [ordered]@{
            mechanism = $NetworkIsolation
            dns_denied = $true
            direct_tcp_denied = $true
            https_denied = $true
        }
    }
}
$utf8WithoutBom = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText(
    $contextPath,
    (($context | ConvertTo-Json -Depth 6) + "`n"),
    $utf8WithoutBom
)

$acceptedManifest = Get-RegularFile `
    -Path ([System.IO.Path]::Combine(
        $source,
        "packaging",
        "parser-pack",
        "accepted-capabilities.json"
    )) `
    -Role "accepted parser-pack manifest"
$fixtureCorpus = Get-RegularFile `
    -Path ([System.IO.Path]::Combine(
        $source,
        "fixtures",
        "languages",
        "optional-parser-pack-corpus.json"
    )) `
    -Role "optional parser fixture corpus"
$sourceEvidence = Get-RegularFile `
    -Path ([System.IO.Path]::Combine(
        $source,
        "packaging",
        "parser-pack",
        "sources",
        "tree-sitter-language-pack-1.13.2.json"
    )) `
    -Role "optional parser source evidence"
$projectLicense = Get-RegularFile `
    -Path ([System.IO.Path]::Combine($source, "LICENSE")) `
    -Role "ProjectAtlas license"
$bundleIntake = Get-RegularFile `
    -Path ([System.IO.Path]::Combine(
        $source,
        "packaging",
        "parser-pack",
        "sources",
        "tree-sitter-language-pack-1.13.2-platform-bundles.json"
    )) `
    -Role "platform bundle intake"
$importPolicy = Get-RegularFile `
    -Path ([System.IO.Path]::Combine(
        $source,
        "packaging",
        "parser-pack",
        "native-import-policy.json"
    )) `
    -Role "native import policy"
$sourceBundle = Get-RegularFile `
    -Path ([System.IO.Path]::Combine($inputs, "source-bundle.tar.zst")) `
    -Role "acquired native source bundle"
$upstreamParsers = Get-RegularFile `
    -Path ([System.IO.Path]::Combine($inputs, "parsers.json")) `
    -Role "acquired upstream parser manifest"

$stagedDirectories = @(
    [System.IO.Path]::Combine($workingDirectory, "staged-a"),
    [System.IO.Path]::Combine($workingDirectory, "staged-b")
)
$archiveDirectories = @(
    [System.IO.Path]::Combine($workingDirectory, "archive-a"),
    [System.IO.Path]::Combine($workingDirectory, "archive-b")
)
$archiveName = "projectatlas-broad-parser-$Target.tar.zst"
$archives = @(
    [System.IO.Path]::Combine($archiveDirectories[0], $archiveName),
    [System.IO.Path]::Combine($archiveDirectories[1], $archiveName)
)
$artifactConstructions = for ($index = 0; $index -lt 2; $index += 1) {
    [System.IO.Directory]::CreateDirectory($archiveDirectories[$index]) | Out-Null
    [pscustomobject]@{
        index = $index
        assembler = $assembler
        assembly_arguments = @(
            $acceptedManifest,
            $fixtureCorpus,
            $sourceEvidence,
            $projectLicense,
            $bundleIntake,
            $importPolicy,
            $contextPath,
            $sourceBundle,
            $upstreamParsers,
            $worker,
            $containmentBroker,
            $Target,
            $stagedDirectories[$index]
        )
        release_tool = $releaseTool
        staged_directory = $stagedDirectories[$index]
        archive = $archives[$index]
    }
}
$script:constructionStage = "parallel-artifact-construction"
Write-ConstructionStatus -Stage $script:constructionStage -State "running"
$artifactConstructions |
    ForEach-Object -Parallel {
        $assembler = [string]$_.assembler
        $assemblyArguments = [string[]]$_.assembly_arguments
        & $assembler @assemblyArguments
        if ($LASTEXITCODE -ne 0) {
            throw "artifact assembly $($_.index) failed with exit code $LASTEXITCODE."
        }
        $releaseTool = [string]$_.release_tool
        & $releaseTool "create" ([string]$_.staged_directory) ([string]$_.archive)
        if ($LASTEXITCODE -ne 0) {
            throw "deterministic archive creation $($_.index) failed with exit code $LASTEXITCODE."
        }
    } `
    -ThrottleLimit 2

$script:constructionStage = "deterministic-archive-comparison"
Write-ConstructionStatus -Stage $script:constructionStage -State "running"
$archiveMeasurements = @(
    $artifactConstructions |
        ForEach-Object -Parallel {
            $archive = Get-Item -LiteralPath ([string]$_.archive) -Force
            [pscustomobject]@{
                index = [int]$_.index
                path = $archive.FullName
                byte_length = $archive.Length
                sha256 = (
                    Get-FileHash -Algorithm SHA256 -LiteralPath $archive.FullName
                ).Hash.ToLowerInvariant()
            }
        } `
        -ThrottleLimit 2
)
$archiveA = $archiveMeasurements | Where-Object index -EQ 0
$archiveB = $archiveMeasurements | Where-Object index -EQ 1
if ($archiveA.byte_length -ne $archiveB.byte_length -or
    $archiveA.sha256 -ne $archiveB.sha256) {
    throw "Independent parser-pack assembly did not produce byte-identical archives."
}

$script:constructionStage = "publication"
Write-ConstructionStatus -Stage $script:constructionStage -State "running"
$publishedArchive = [System.IO.Path]::Combine(
    $publishDirectory,
    "projectatlas-broad-parser-$Target.tar.zst"
)
$publishedVerifier = [System.IO.Path]::Combine(
    $publishDirectory,
    "optional_parser_pack_release$executableSuffix"
)
$publishedVerifierDigest = [System.IO.Path]::Combine(
    $publishDirectory,
    "optional_parser_pack_release.sha256"
)
$publishedManifest = [System.IO.Path]::Combine(
    $publishDirectory,
    "accepted-capabilities.json"
)
$publishedNetworkCheck = [System.IO.Path]::Combine(
    $publishDirectory,
    "check-parser-pack-network-boundary.ps1"
)
[System.IO.File]::Move($archiveA.path, $publishedArchive)
[System.IO.File]::Copy($releaseTool, $publishedVerifier, $false)
[System.IO.File]::WriteAllText(
    $publishedVerifierDigest,
    ((Get-FileHash -Algorithm SHA256 -LiteralPath $releaseTool).Hash.ToLowerInvariant() + "`n"),
    $utf8WithoutBom
)
[System.IO.File]::Copy($acceptedManifest, $publishedManifest, $false)
[System.IO.File]::Copy($networkCheck, $publishedNetworkCheck, $false)
$script:constructionStage = "reusable-cargo-target-validation"
Write-ConstructionStatus -Stage $script:constructionStage -State "running"
Assert-ReusableCargoTarget `
    -OutputDirectory $output `
    -BuildDirectory $buildDirectory `
    -MaximumEntries $reusableCargoTargetMaxEntries `
    -MaximumBytes $reusableCargoTargetMaxBytes |
    Out-Null
Close-ContainedCargoJobserver
[System.IO.File]::Delete($script:constructionStatusPath)

[pscustomobject]@{
    target = $Target
    network_isolation = $NetworkIsolation
    archive = [System.IO.Path]::GetFileName($publishedArchive)
    archive_bytes = $archiveA.byte_length
    archive_sha256 = $archiveA.sha256
    verifier = [System.IO.Path]::GetFileName($publishedVerifier)
    verifier_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $releaseTool).Hash.ToLowerInvariant()
    accepted_manifest = [System.IO.Path]::GetFileName($publishedManifest)
    independent_assemblies = 2
} | ConvertTo-Json -Compress
