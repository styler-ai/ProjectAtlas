# Purpose: Install or update the ProjectAtlas plugin runtime and Windows MCP configs.

param(
    [string]$ProjectRoot,
    [string]$Repository = "https://github.com/styler-ai/ProjectAtlas",
    [string]$ProjectAtlasVersion,
    [string]$ReleaseBaseUrl = "https://github.com/styler-ai/ProjectAtlas/releases/download",
    [string]$RuntimePath,
    [switch]$ReleaseBinaryOnly
)

$ErrorActionPreference = "Stop"

function Resolve-DefaultProjectRoot {
    (Get-Location).Path
}

function Test-Truthy {
    param(
        [string]$Value
    )
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $false
    }
    return @("1", "true", "yes", "on") -contains $Value.ToLowerInvariant()
}

function Assert-ProjectAtlasDirectPath {
    param(
        [string]$Path,
        [string]$Label
    )
    $item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($item -and (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "$Label must not be a symlink, junction, or reparse point: $Path"
    }
}

function Assert-ProjectAtlasDirectFilePath {
    param(
        [string]$Path,
        [string]$Label
    )
    Assert-ProjectAtlasDirectPath $Path $Label
    $item = Get-Item -Force -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($item -and -not ($item -is [System.IO.FileInfo])) {
        throw "$Label must be a regular file: $Path"
    }
}

function Resolve-PluginReleaseVersion {
    $scriptDirectory = Split-Path -Parent $PSCommandPath
    $pluginRoot = Split-Path -Parent $scriptDirectory
    $manifestPath = Join-Path $pluginRoot ".codex-plugin\plugin.json"
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        return $null
    }
    try {
        $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
        if ($manifest.version) {
            return "v$($manifest.version)"
        }
    }
    catch {
        return $null
    }
    return $null
}

function Find-Cargo {
    $cargoHome = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path -LiteralPath $cargoHome) {
        return $cargoHome
    }
    $cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cargoCommand) {
        return $cargoCommand.Source
    }
    return $null
}

function Convert-ProjectAtlasVersionTag {
    param(
        [string]$Version
    )
    if ([string]::IsNullOrWhiteSpace($Version)) {
        return $null
    }
    return $Version.Trim().TrimStart("v")
}

function Initialize-ProjectAtlasRuntimeProbe {
    if ("ProjectAtlas.Installer.RuntimeProbeProcess" -as [type]) {
        return
    }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Threading;

namespace ProjectAtlas.Installer
{
    public sealed class RuntimeProbeProcess : IDisposable
    {
        private const uint CreateSuspended = 0x00000004;
        private const uint CreateNoWindow = 0x08000000;
        private const uint ExtendedStartupInfoPresent = 0x00080000;
        private const uint StartfUseStdHandles = 0x00000100;
        private static readonly IntPtr ProcThreadAttributeHandleList = new IntPtr(0x00020002);
        private const uint GenericRead = 0x80000000;
        private const uint GenericWrite = 0x40000000;
        private const uint FileShareRead = 0x00000001;
        private const uint FileShareWrite = 0x00000002;
        private const uint FileShareDelete = 0x00000004;
        private const uint CreateAlways = 2;
        private const uint OpenExisting = 3;
        private const uint FileAttributeNormal = 0x00000080;
        private const uint JobObjectBasicAccountingInformation = 1;
        private const uint JobObjectExtendedLimitInformation = 9;
        private const uint JobObjectLimitKillOnJobClose = 0x00002000;
        private const uint WaitObject0 = 0;
        private const uint WaitTimeout = 258;
        private const uint StillActive = 259;

        [StructLayout(LayoutKind.Sequential)]
        private struct SecurityAttributes
        {
            internal int Length;
            internal IntPtr SecurityDescriptor;
            [MarshalAs(UnmanagedType.Bool)]
            internal bool InheritHandle;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct StartupInfo
        {
            internal int Size;
            internal string Reserved;
            internal string Desktop;
            internal string Title;
            internal int X;
            internal int Y;
            internal int XSize;
            internal int YSize;
            internal int XCountChars;
            internal int YCountChars;
            internal int FillAttribute;
            internal uint Flags;
            internal short ShowWindow;
            internal short ReservedBytes;
            internal IntPtr ReservedPointer;
            internal IntPtr StandardInput;
            internal IntPtr StandardOutput;
            internal IntPtr StandardError;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ProcessInformation
        {
            internal IntPtr Process;
            internal IntPtr Thread;
            internal uint ProcessId;
            internal uint ThreadId;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct StartupInfoEx
        {
            internal StartupInfo StartupInfo;
            internal IntPtr AttributeList;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct IoCounters
        {
            internal ulong ReadOperations;
            internal ulong WriteOperations;
            internal ulong OtherOperations;
            internal ulong ReadBytes;
            internal ulong WriteBytes;
            internal ulong OtherBytes;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct JobObjectBasicAccountingInformationValue
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

        [StructLayout(LayoutKind.Sequential)]
        private struct JobObjectBasicLimitInformation
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
        private struct JobObjectExtendedLimitInformationValue
        {
            internal JobObjectBasicLimitInformation BasicLimitInformation;
            internal IoCounters IoInfo;
            internal UIntPtr ProcessMemoryLimit;
            internal UIntPtr JobMemoryLimit;
            internal UIntPtr PeakProcessMemoryUsed;
            internal UIntPtr PeakJobMemoryUsed;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr CreateFile(
            string fileName,
            uint desiredAccess,
            uint shareMode,
            ref SecurityAttributes securityAttributes,
            uint creationDisposition,
            uint flagsAndAttributes,
            IntPtr templateFile);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CreateProcessW(
            string applicationName,
            StringBuilder commandLine,
            IntPtr processAttributes,
            IntPtr threadAttributes,
            [MarshalAs(UnmanagedType.Bool)] bool inheritHandles,
            uint creationFlags,
            IntPtr environment,
            string currentDirectory,
            ref StartupInfoEx startupInfo,
            out ProcessInformation processInformation);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool InitializeProcThreadAttributeList(
            IntPtr attributeList,
            int attributeCount,
            int flags,
            ref IntPtr size);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool UpdateProcThreadAttribute(
            IntPtr attributeList,
            uint flags,
            IntPtr attribute,
            IntPtr value,
            IntPtr size,
            IntPtr previousValue,
            IntPtr returnSize);

        [DllImport("kernel32.dll")]
        private static extern void DeleteProcThreadAttributeList(IntPtr attributeList);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr CreateJobObject(IntPtr jobAttributes, string name);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetInformationJobObject(
            IntPtr job,
            uint informationClass,
            ref JobObjectExtendedLimitInformationValue information,
            uint informationLength);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool QueryInformationJobObject(
            IntPtr job,
            uint informationClass,
            out JobObjectBasicAccountingInformationValue information,
            uint informationLength,
            IntPtr returnLength);

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

        private IntPtr process;
        private IntPtr job;
        private IntPtr standardInput;
        private IntPtr standardOutput;
        private IntPtr standardError;
        private bool disposed;

        private RuntimeProbeProcess(
            IntPtr process,
            IntPtr job,
            IntPtr standardInput,
            IntPtr standardOutput,
            IntPtr standardError)
        {
            this.process = process;
            this.job = job;
            this.standardInput = standardInput;
            this.standardOutput = standardOutput;
            this.standardError = standardError;
        }

        public static RuntimeProbeProcess Start(
            string filePath,
            string[] arguments,
            string standardOutputPath,
            string standardErrorPath)
        {
            if (String.IsNullOrWhiteSpace(filePath))
                throw new ArgumentException("Runtime path is required.", "filePath");

            string application = Path.GetFullPath(filePath);
            string[] commandArguments = arguments ?? new string[0];
            string commandLineOverride = null;
            string extension = Path.GetExtension(application);
            if (String.Equals(extension, ".cmd", StringComparison.OrdinalIgnoreCase)
                || String.Equals(extension, ".bat", StringComparison.OrdinalIgnoreCase))
            {
                string systemCommand = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.System),
                    "cmd.exe");
                RequireDirectFile(systemCommand, "Windows command host");
                StringBuilder command = new StringBuilder("\"");
                command.Append(QuoteForCommandHost(application));
                foreach (string argument in commandArguments)
                    command.Append(' ').Append(QuoteForCommandHost(argument));
                command.Append('"');
                application = systemCommand;
                commandLineOverride =
                    QuoteWindowsArgument(systemCommand) + " /d /s /v:off /c " + command;
                commandArguments = new string[0];
            }
            else if (String.Equals(extension, ".ps1", StringComparison.OrdinalIgnoreCase))
            {
                string powershell = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.System),
                    "WindowsPowerShell",
                    "v1.0",
                    "powershell.exe");
                RequireDirectFile(powershell, "Windows PowerShell host");
                string[] scriptArguments = new string[arguments.Length + 7];
                scriptArguments[0] = "-NoLogo";
                scriptArguments[1] = "-NoProfile";
                scriptArguments[2] = "-NonInteractive";
                scriptArguments[3] = "-ExecutionPolicy";
                scriptArguments[4] = "Bypass";
                scriptArguments[5] = "-File";
                scriptArguments[6] = application;
                Array.Copy(arguments, 0, scriptArguments, 7, arguments.Length);
                application = powershell;
                commandArguments = scriptArguments;
            }
            else
            {
                RequireDirectFile(application, "ProjectAtlas runtime probe");
            }

            SecurityAttributes inheritable = new SecurityAttributes();
            inheritable.Length = Marshal.SizeOf(typeof(SecurityAttributes));
            inheritable.InheritHandle = true;
            IntPtr input = IntPtr.Zero;
            IntPtr output = IntPtr.Zero;
            IntPtr error = IntPtr.Zero;
            IntPtr attributeList = IntPtr.Zero;
            IntPtr inheritedHandles = IntPtr.Zero;
            IntPtr containmentJob = IntPtr.Zero;
            ProcessInformation created = new ProcessInformation();
            bool assignedToJob = false;
            bool attributeListInitialized = false;
            try
            {
                input = CreateFile(
                    "NUL",
                    GenericRead,
                    FileShareRead | FileShareWrite,
                    ref inheritable,
                    OpenExisting,
                    FileAttributeNormal,
                    IntPtr.Zero);
                output = CreateFile(
                    standardOutputPath,
                    GenericWrite,
                    FileShareRead | FileShareDelete,
                    ref inheritable,
                    CreateAlways,
                    FileAttributeNormal,
                    IntPtr.Zero);
                error = CreateFile(
                    standardErrorPath,
                    GenericWrite,
                    FileShareRead | FileShareDelete,
                    ref inheritable,
                    CreateAlways,
                    FileAttributeNormal,
                    IntPtr.Zero);
                if (input == new IntPtr(-1) || output == new IntPtr(-1) || error == new IntPtr(-1))
                    throw Win32Failure("open bounded runtime probe streams");

                IntPtr attributeListSize = IntPtr.Zero;
                InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref attributeListSize);
                if (attributeListSize == IntPtr.Zero)
                    throw Win32Failure("size runtime probe handle list");
                attributeList = Marshal.AllocHGlobal(attributeListSize);
                if (!InitializeProcThreadAttributeList(attributeList, 1, 0, ref attributeListSize))
                    throw Win32Failure("initialize runtime probe handle list");
                attributeListInitialized = true;
                inheritedHandles = Marshal.AllocHGlobal(IntPtr.Size * 3);
                Marshal.WriteIntPtr(inheritedHandles, 0, input);
                Marshal.WriteIntPtr(inheritedHandles, IntPtr.Size, output);
                Marshal.WriteIntPtr(inheritedHandles, IntPtr.Size * 2, error);
                if (!UpdateProcThreadAttribute(
                    attributeList,
                    0,
                    ProcThreadAttributeHandleList,
                    inheritedHandles,
                    new IntPtr(IntPtr.Size * 3),
                    IntPtr.Zero,
                    IntPtr.Zero))
                    throw Win32Failure("set runtime probe handle list");

                StartupInfoEx startup = new StartupInfoEx();
                startup.StartupInfo.Size = Marshal.SizeOf(typeof(StartupInfoEx));
                startup.StartupInfo.Flags = StartfUseStdHandles;
                startup.StartupInfo.StandardInput = input;
                startup.StartupInfo.StandardOutput = output;
                startup.StartupInfo.StandardError = error;
                startup.AttributeList = attributeList;
                StringBuilder commandLine = new StringBuilder(
                    commandLineOverride ?? BuildCommandLine(application, commandArguments));
                if (!CreateProcessW(
                    application,
                    commandLine,
                    IntPtr.Zero,
                    IntPtr.Zero,
                    true,
                    CreateSuspended | CreateNoWindow | ExtendedStartupInfoPresent,
                    IntPtr.Zero,
                    null,
                    ref startup,
                    out created))
                    throw Win32Failure("create suspended runtime probe");

                containmentJob = CreateJobObject(IntPtr.Zero, null);
                if (containmentJob == IntPtr.Zero)
                    throw Win32Failure("create runtime probe job");
                JobObjectExtendedLimitInformationValue limits =
                    new JobObjectExtendedLimitInformationValue();
                limits.BasicLimitInformation.LimitFlags = JobObjectLimitKillOnJobClose;
                if (!SetInformationJobObject(
                    containmentJob,
                    JobObjectExtendedLimitInformation,
                    ref limits,
                    (uint)Marshal.SizeOf(typeof(JobObjectExtendedLimitInformationValue))))
                    throw Win32Failure("configure runtime probe job");
                if (!AssignProcessToJobObject(containmentJob, created.Process))
                    throw Win32Failure("assign runtime probe job");
                assignedToJob = true;
                if (ResumeThread(created.Thread) == UInt32.MaxValue)
                    throw Win32Failure("resume runtime probe");
                CloseHandle(created.Thread);
                created.Thread = IntPtr.Zero;

                RuntimeProbeProcess result = new RuntimeProbeProcess(
                    created.Process,
                    containmentJob,
                    input,
                    output,
                    error);
                created.Process = IntPtr.Zero;
                containmentJob = IntPtr.Zero;
                input = IntPtr.Zero;
                output = IntPtr.Zero;
                error = IntPtr.Zero;
                return result;
            }
            finally
            {
                Exception cleanupFailure = null;
                try
                {
                    if (created.Process != IntPtr.Zero)
                    {
                        uint wait = WaitForSingleObject(created.Process, 0);
                        if (wait == WaitTimeout)
                        {
                            bool terminated = assignedToJob && containmentJob != IntPtr.Zero
                                ? TerminateJobObject(containmentJob, 1)
                                : TerminateProcess(created.Process, 1);
                            if (!terminated)
                                throw Win32Failure("terminate failed runtime probe construction");
                            wait = WaitForSingleObject(created.Process, 5000);
                        }
                        if (wait == WaitTimeout)
                            throw new TimeoutException("Failed runtime probe construction did not stop.");
                        if (wait != WaitObject0)
                            throw Win32Failure("wait for failed runtime probe construction");
                    }
                }
                catch (Exception failure)
                {
                    cleanupFailure = failure;
                }
                try { CloseIfValid(containmentJob, "close failed runtime probe job"); }
                catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
                if (attributeListInitialized)
                    DeleteProcThreadAttributeList(attributeList);
                if (attributeList != IntPtr.Zero)
                    Marshal.FreeHGlobal(attributeList);
                if (inheritedHandles != IntPtr.Zero)
                    Marshal.FreeHGlobal(inheritedHandles);
                try { CloseIfValid(created.Thread, "close failed runtime probe thread"); }
                catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
                try { CloseIfValid(created.Process, "close failed runtime probe process"); }
                catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
                try { CloseIfValid(input, "close failed runtime probe input"); }
                catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
                try { CloseIfValid(output, "close failed runtime probe output"); }
                catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
                try { CloseIfValid(error, "close failed runtime probe error"); }
                catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
                if (cleanupFailure != null)
                    throw cleanupFailure;
            }
        }

        public bool WaitForExit(int timeoutMilliseconds)
        {
            ThrowIfDisposed();
            uint result = WaitForSingleObject(process, checked((uint)timeoutMilliseconds));
            if (result == WaitObject0)
                return true;
            if (result == WaitTimeout)
                return false;
            throw Win32Failure("wait for runtime probe");
        }

        public int ExitCode
        {
            get
            {
                ThrowIfDisposed();
                uint code;
                if (!GetExitCodeProcess(process, out code))
                    throw Win32Failure("read runtime probe exit code");
                if (code == StillActive)
                    throw new InvalidOperationException("Runtime probe is still active.");
                return unchecked((int)code);
            }
        }

        public void Stop(int timeoutMilliseconds)
        {
            ThrowIfDisposed();
            if (ActiveProcesses() == 0)
                return;
            if (!TerminateJobObject(job, 1))
                throw Win32Failure("terminate runtime probe job");
            DateTime deadline = DateTime.UtcNow.AddMilliseconds(timeoutMilliseconds);
            while (ActiveProcesses() != 0)
            {
                if (DateTime.UtcNow >= deadline)
                    throw new TimeoutException("Runtime probe job did not stop within its cleanup bound.");
                Thread.Sleep(10);
            }
        }

        public void Dispose()
        {
            if (disposed)
                return;
            Exception cleanupFailure = null;
            try
            {
                Stop(5000);
            }
            catch (Exception failure)
            {
                cleanupFailure = failure;
            }
            disposed = true;
            try { CloseIfValid(job, "close runtime probe job"); }
            catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
            try { CloseIfValid(process, "close runtime probe process"); }
            catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
            try { CloseIfValid(standardInput, "close runtime probe input"); }
            catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
            try { CloseIfValid(standardOutput, "close runtime probe output"); }
            catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
            try { CloseIfValid(standardError, "close runtime probe error"); }
            catch (Exception failure) { if (cleanupFailure == null) cleanupFailure = failure; }
            job = IntPtr.Zero;
            process = IntPtr.Zero;
            standardInput = IntPtr.Zero;
            standardOutput = IntPtr.Zero;
            standardError = IntPtr.Zero;
            if (cleanupFailure != null)
                throw cleanupFailure;
        }

        private uint ActiveProcesses()
        {
            JobObjectBasicAccountingInformationValue accounting;
            if (!QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                out accounting,
                (uint)Marshal.SizeOf(typeof(JobObjectBasicAccountingInformationValue)),
                IntPtr.Zero))
                throw Win32Failure("query runtime probe job");
            return accounting.ActiveProcesses;
        }

        private void ThrowIfDisposed()
        {
            if (disposed)
                throw new ObjectDisposedException("RuntimeProbeProcess");
        }

        private static string BuildCommandLine(string application, string[] arguments)
        {
            StringBuilder command = new StringBuilder(QuoteWindowsArgument(application));
            foreach (string argument in arguments)
                command.Append(' ').Append(QuoteWindowsArgument(argument));
            return command.ToString();
        }

        private static string QuoteWindowsArgument(string argument)
        {
            if (argument.Length != 0 && argument.IndexOfAny(new char[] { ' ', '\t', '\n', '\v', '"' }) < 0)
                return argument;
            StringBuilder quoted = new StringBuilder("\"");
            int backslashes = 0;
            foreach (char character in argument)
            {
                if (character == '\\')
                {
                    backslashes++;
                }
                else if (character == '"')
                {
                    quoted.Append('\\', backslashes * 2 + 1).Append(character);
                    backslashes = 0;
                }
                else
                {
                    quoted.Append('\\', backslashes).Append(character);
                    backslashes = 0;
                }
            }
            quoted.Append('\\', backslashes * 2).Append('"');
            return quoted.ToString();
        }

        private static string QuoteForCommandHost(string argument)
        {
            if (argument.IndexOf('"') >= 0
                || argument.IndexOf('%') >= 0
                || argument.IndexOf('\r') >= 0
                || argument.IndexOf('\n') >= 0)
                throw new ArgumentException("Runtime probe argument contains unsupported command-host characters.");
            if (argument.Length != 0
                && argument.IndexOfAny(new char[] { ' ', '\t', '&', '|', '<', '>', '^', '(', ')' }) < 0)
                return argument;
            return "\"" + argument + "\"";
        }

        private static void RequireDirectFile(string path, string label)
        {
            FileInfo file = new FileInfo(path);
            if (!file.Exists || (file.Attributes & FileAttributes.ReparsePoint) != 0)
                throw new InvalidOperationException(label + " must be a direct regular file: " + path);
        }

        private static Win32Exception Win32Failure(string operation)
        {
            return new Win32Exception(Marshal.GetLastWin32Error(), operation);
        }

        private static void CloseIfValid(IntPtr handle, string operation)
        {
            if (handle != IntPtr.Zero && handle != new IntPtr(-1))
                CloseHandleChecked(handle, operation);
        }

        private static void CloseHandleChecked(IntPtr handle, string operation)
        {
            if (!CloseHandle(handle))
                throw Win32Failure(operation);
        }
    }

    public sealed class ProcessRetirementResult
    {
        public ProcessRetirementResult(string state, int errorCode)
        {
            State = state;
            ErrorCode = errorCode;
        }

        public string State { get; private set; }
        public int ErrorCode { get; private set; }
    }

    public static class ObsoleteMcpProcess
    {
        private const uint WaitObject0 = 0;
        private const uint WaitTimeout = 258;
        private const int ErrorAccessDenied = 5;
        // ProcessCommandLineInformation reads the command from the held handle,
        // avoiding a final PID-only lookup before the one allowed termination.
        private const int ProcessCommandLineInformation = 60;
        private const int StatusInfoLengthMismatch = unchecked((int)0xC0000004);
        // Win32_Process CreationDate can lose sub-microsecond FILETIME precision.
        private const long CreationTimeRepresentationToleranceTicks = 10;

        [DllImport("shell32.dll", SetLastError = true)]
        private static extern IntPtr CommandLineToArgvW(
            [MarshalAs(UnmanagedType.LPWStr)] string commandLine,
            out int argumentCount);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr LocalFree(IntPtr memory);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool TerminateProcess(IntPtr process, uint exitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);

        [DllImport("ntdll.dll")]
        private static extern int NtQueryInformationProcess(
            IntPtr process,
            int informationClass,
            IntPtr information,
            int informationLength,
            out int returnLength);

        [DllImport("ntdll.dll")]
        private static extern uint RtlNtStatusToDosError(int status);

        [StructLayout(LayoutKind.Sequential)]
        private struct UnicodeString
        {
            internal ushort Length;
            internal ushort MaximumLength;
            internal IntPtr Buffer;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ProcessBasicInformation
        {
            internal IntPtr Reserved1;
            internal IntPtr PebBaseAddress;
            internal IntPtr Reserved2_0;
            internal IntPtr Reserved2_1;
            internal IntPtr UniqueProcessId;
            internal IntPtr InheritedFromUniqueProcessId;
        }

        public static string[] ParseCommandLine(string commandLine)
        {
            if (String.IsNullOrWhiteSpace(commandLine))
                return new string[0];
            int argumentCount;
            IntPtr arguments = CommandLineToArgvW(commandLine, out argumentCount);
            if (arguments == IntPtr.Zero)
                throw new Win32Exception(Marshal.GetLastWin32Error(), "parse process command line");
            try
            {
                string[] result = new string[argumentCount];
                for (int index = 0; index < argumentCount; index++)
                {
                    IntPtr argument = Marshal.ReadIntPtr(arguments, index * IntPtr.Size);
                    result[index] = Marshal.PtrToStringUni(argument);
                }
                return result;
            }
            finally
            {
                LocalFree(arguments);
            }
        }

        public static ProcessRetirementResult Retire(
            int processId,
            long expectedCreationFileTimeUtc,
            string expectedPath,
            string[] expectedArguments,
            string expectedImageSha256,
            int expectedParentProcessId,
            long expectedParentCreationFileTimeUtc,
            string expectedParentPath,
            string[] expectedParentArguments,
            string expectedParentImageSha256,
            int timeoutMilliseconds)
        {
            Process candidate;
            try
            {
                candidate = Process.GetProcessById(processId);
            }
            catch (ArgumentException)
            {
                return new ProcessRetirementResult("exited", 0);
            }
            try
            {
                using (candidate)
                {
                    if (candidate.HasExited)
                        return new ProcessRetirementResult("exited", 0);
                    IntPtr handle = candidate.Handle;
                    long creationFileTimeUtc;
                    string imagePath;
                    try
                    {
                        creationFileTimeUtc = candidate.StartTime.ToUniversalTime().ToFileTimeUtc();
                        imagePath = candidate.MainModule.FileName;
                    }
                    catch (Win32Exception failure)
                    {
                        return Failure("inspection_failed", failure.NativeErrorCode);
                    }
                    if (Math.Abs(creationFileTimeUtc - expectedCreationFileTimeUtc)
                        > CreationTimeRepresentationToleranceTicks)
                        return new ProcessRetirementResult("identity_changed_creation", 0);
                    if (!String.Equals(
                            Path.GetFullPath(imagePath),
                            Path.GetFullPath(expectedPath),
                            StringComparison.OrdinalIgnoreCase))
                        return new ProcessRetirementResult("identity_changed_path", 0);
                    if (candidate.HasExited)
                        return new ProcessRetirementResult("exited", 0);
                    string commandLine;
                    try
                    {
                        commandLine = ReadCommandLine(handle);
                    }
                    catch (Win32Exception failure)
                    {
                        return Failure("inspection_failed", failure.NativeErrorCode);
                    }
                    string[] actualArguments = ParseCommandLine(commandLine);
                    if (!ArgumentsEqual(actualArguments, expectedArguments, true))
                        return new ProcessRetirementResult("identity_changed_command", 0);
                    if (ReadParentProcessId(handle) != expectedParentProcessId)
                        return new ProcessRetirementResult("identity_changed_parent", 0);
                    if (candidate.HasExited)
                        return new ProcessRetirementResult("exited", 0);

                    Process parent;
                    try
                    {
                        parent = Process.GetProcessById(expectedParentProcessId);
                    }
                    catch (ArgumentException)
                    {
                        return new ProcessRetirementResult("owner_parent_exited", 0);
                    }
                    using (parent)
                    {
                        IntPtr parentHandle = parent.Handle;
                        long parentCreationFileTimeUtc;
                        string parentImagePath;
                        try
                        {
                            parentCreationFileTimeUtc = parent.StartTime.ToUniversalTime().ToFileTimeUtc();
                            parentImagePath = parent.MainModule.FileName;
                        }
                        catch (Win32Exception failure)
                        {
                            return Failure("parent_inspection_failed", failure.NativeErrorCode);
                        }
                        if (Math.Abs(parentCreationFileTimeUtc - expectedParentCreationFileTimeUtc)
                            > CreationTimeRepresentationToleranceTicks)
                            return new ProcessRetirementResult("identity_changed_parent_creation", 0);
                        if (!String.Equals(
                                Path.GetFullPath(parentImagePath),
                                Path.GetFullPath(expectedParentPath),
                                StringComparison.OrdinalIgnoreCase))
                            return new ProcessRetirementResult("identity_changed_parent_path", 0);
                        if (parent.HasExited)
                            return new ProcessRetirementResult("owner_parent_exited", 0);
                        string[] actualParentArguments;
                        try
                        {
                            actualParentArguments = ParseCommandLine(ReadCommandLine(parentHandle));
                        }
                        catch (Win32Exception failure)
                        {
                            return Failure("parent_inspection_failed", failure.NativeErrorCode);
                        }
                        if (!ArgumentsEqual(actualParentArguments, expectedParentArguments, false))
                            return new ProcessRetirementResult("identity_changed_parent_command", 0);
                        if (parent.HasExited)
                            return new ProcessRetirementResult("owner_parent_exited", 0);
                        if (candidate.HasExited)
                            return new ProcessRetirementResult("exited", 0);
                        if (!String.Equals(
                                ComputeImageSha256(expectedPath),
                                expectedImageSha256,
                                StringComparison.OrdinalIgnoreCase))
                            return new ProcessRetirementResult("identity_changed_file", 0);
                        if (!String.Equals(
                                ComputeImageSha256(parentImagePath),
                                expectedParentImageSha256,
                                StringComparison.OrdinalIgnoreCase))
                            return new ProcessRetirementResult("identity_changed_parent_file", 0);
                        if (parent.HasExited)
                            return new ProcessRetirementResult("owner_parent_exited", 0);
                        if (candidate.HasExited)
                            return new ProcessRetirementResult("exited", 0);
                        if (!TerminateProcess(handle, 0))
                        {
                            int errorCode = Marshal.GetLastWin32Error();
                            if (WaitForSingleObject(handle, 0) == WaitObject0)
                                return new ProcessRetirementResult("exited", 0);
                            return Failure("retirement_failed", errorCode);
                        }
                    }
                    uint wait = WaitForSingleObject(handle, checked((uint)timeoutMilliseconds));
                    if (wait == WaitObject0)
                        return new ProcessRetirementResult("retired", 0);
                    if (wait == WaitTimeout)
                        return new ProcessRetirementResult("retirement_timeout", 0);
                    return Failure("retirement_wait_failed", Marshal.GetLastWin32Error());
                }
            }
            catch (ArgumentException)
            {
                return new ProcessRetirementResult("inspection_failed", 0);
            }
            catch (Win32Exception failure)
            {
                return Failure("inspection_failed", failure.NativeErrorCode);
            }
            catch (InvalidOperationException)
            {
                try
                {
                    if (candidate.HasExited)
                        return new ProcessRetirementResult("exited", 0);
                }
                catch (InvalidOperationException)
                {
                }
                return new ProcessRetirementResult("inspection_failed", 0);
            }
            catch (IOException)
            {
                return new ProcessRetirementResult("inspection_failed", 0);
            }
            catch (UnauthorizedAccessException)
            {
                return new ProcessRetirementResult("access_denied", ErrorAccessDenied);
            }
        }

        private static string ReadCommandLine(IntPtr process)
        {
            int returnLength;
            int status = NtQueryInformationProcess(
                process,
                ProcessCommandLineInformation,
                IntPtr.Zero,
                0,
                out returnLength);
            if (status != StatusInfoLengthMismatch || returnLength <= Marshal.SizeOf(typeof(UnicodeString)))
                throw NtFailure("size process command line", status);
            IntPtr buffer = Marshal.AllocHGlobal(returnLength);
            try
            {
                status = NtQueryInformationProcess(
                    process,
                    ProcessCommandLineInformation,
                    buffer,
                    returnLength,
                    out returnLength);
                if (status < 0)
                    throw NtFailure("read process command line", status);
                UnicodeString commandLine = (UnicodeString)Marshal.PtrToStructure(
                    buffer,
                    typeof(UnicodeString));
                if (commandLine.Buffer == IntPtr.Zero || commandLine.Length == 0)
                    throw new Win32Exception(87, "Process command line is empty.");
                return Marshal.PtrToStringUni(commandLine.Buffer, commandLine.Length / 2);
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }

        private static int ReadParentProcessId(IntPtr process)
        {
            int size = Marshal.SizeOf(typeof(ProcessBasicInformation));
            IntPtr buffer = Marshal.AllocHGlobal(size);
            try
            {
                int returnLength;
                int status = NtQueryInformationProcess(
                    process,
                    0,
                    buffer,
                    size,
                    out returnLength);
                if (status < 0 || returnLength < size)
                    throw NtFailure("read process parent", status);
                ProcessBasicInformation information = (ProcessBasicInformation)Marshal.PtrToStructure(
                    buffer,
                    typeof(ProcessBasicInformation));
                return checked((int)information.InheritedFromUniqueProcessId.ToInt64());
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }

        private static Win32Exception NtFailure(string operation, int status)
        {
            return new Win32Exception(checked((int)RtlNtStatusToDosError(status)), operation);
        }

        private static bool ArgumentsEqual(
            string[] actual,
            string[] expected,
            bool projectAtlasMcpArguments)
        {
            if (actual == null || expected == null || actual.Length != expected.Length)
                return false;
            for (int index = 0; index < actual.Length; index++)
            {
                bool pathValue = projectAtlasMcpArguments
                    && (index == 0
                    || (index > 0
                        && (String.Equals(expected[index - 1], "--db", StringComparison.Ordinal)
                            || String.Equals(expected[index - 1], "--config", StringComparison.Ordinal))));
                StringComparison comparison = pathValue
                    ? StringComparison.OrdinalIgnoreCase
                    : StringComparison.Ordinal;
                string actualArgument = pathValue ? Path.GetFullPath(actual[index]) : actual[index];
                string expectedArgument = pathValue ? Path.GetFullPath(expected[index]) : expected[index];
                if (!String.Equals(actualArgument, expectedArgument, comparison))
                    return false;
            }
            return true;
        }

        public static string ComputeImageSha256(string path)
        {
            using (FileStream input = new FileStream(
                path,
                FileMode.Open,
                FileAccess.Read,
                FileShare.Read | FileShare.Delete))
            using (SHA256 sha256 = SHA256.Create())
            {
                byte[] digest = sha256.ComputeHash(input);
                StringBuilder hex = new StringBuilder(digest.Length * 2);
                foreach (byte value in digest)
                    hex.Append(value.ToString("x2", System.Globalization.CultureInfo.InvariantCulture));
                return hex.ToString();
            }
        }

        private static ProcessRetirementResult Failure(string fallbackState, int errorCode)
        {
            return new ProcessRetirementResult(
                errorCode == ErrorAccessDenied ? "access_denied" : fallbackState,
                errorCode);
        }
    }
}
'@
}

function Test-ProjectAtlasJsonObject {
    param(
        [AllowNull()]
        [object]$Value
    )
    return $null -ne $Value `
        -and $Value.GetType() -eq [System.Management.Automation.PSCustomObject]
}

function Invoke-ProjectAtlasBoundedJsonCommand {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )
    if (-not $FilePath -or -not (Test-Path -LiteralPath $FilePath)) {
        return $null
    }
    $probeTimeoutMs = 5000
    $maximumOutputBytes = 1024 * 1024
    $probeId = [Guid]::NewGuid().ToString("N")
    $standardOutput = Join-Path ([IO.Path]::GetTempPath()) "projectatlas-command-probe-$probeId.stdout"
    $standardError = Join-Path ([IO.Path]::GetTempPath()) "projectatlas-command-probe-$probeId.stderr"
    $probeFiles = @($standardOutput, $standardError)
    $process = $null
    $probePayload = $null
    $probeCleanupSucceeded = $false
    try {
        Initialize-ProjectAtlasRuntimeProbe
        $process = [ProjectAtlas.Installer.RuntimeProbeProcess]::Start(
            $FilePath,
            $Arguments,
            $standardOutput,
            $standardError
        )
        $probeClock = [Diagnostics.Stopwatch]::StartNew()
        do {
            $exited = $process.WaitForExit(25)
            $outputLimitExceeded = $false
            foreach ($probeFile in $probeFiles) {
                if ((Test-Path -LiteralPath $probeFile) `
                    -and (Get-Item -LiteralPath $probeFile).Length -gt $maximumOutputBytes) {
                    $outputLimitExceeded = $true
                    break
                }
            }
            if ($outputLimitExceeded -or (-not $exited -and $probeClock.ElapsedMilliseconds -ge $probeTimeoutMs)) {
                $process.Stop($probeTimeoutMs)
                return $null
            }
        }
        while (-not $exited)
        $exitCode = $process.ExitCode
        # The job survives the launcher, so this also reaps asynchronously spawned descendants.
        $process.Stop($probeTimeoutMs)
        if ($exitCode -ne 0) {
            return $null
        }
        $process.Dispose()
        $process = $null
        foreach ($probeFile in $probeFiles) {
            if ((Get-Item -LiteralPath $probeFile -ErrorAction Stop).Length -gt $maximumOutputBytes) {
                return $null
            }
        }
        $jsonStream = $null
        try {
            $jsonStream = [IO.File]::Open(
                $standardOutput,
                [IO.FileMode]::Open,
                [IO.FileAccess]::Read,
                ([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
            )
            $jsonLength = $jsonStream.Length
            if ($jsonLength -lt 0 -or $jsonLength -gt $maximumOutputBytes) {
                return $null
            }
            $jsonBytes = [byte[]]::new([int]$jsonLength)
            $jsonOffset = 0
            while ($jsonOffset -lt $jsonBytes.Length) {
                $jsonRead = $jsonStream.Read(
                    $jsonBytes,
                    $jsonOffset,
                    $jsonBytes.Length - $jsonOffset
                )
                if ($jsonRead -eq 0) {
                    return $null
                }
                $jsonOffset += $jsonRead
            }
            if ($jsonStream.Length -ne $jsonLength) {
                return $null
            }
        }
        finally {
            if ($jsonStream) {
                $jsonStream.Dispose()
            }
        }
        $jsonText = [Text.UTF8Encoding]::new($false, $true).GetString($jsonBytes)
        if (-not $jsonText.TrimStart().StartsWith("{", [System.StringComparison]::Ordinal)) {
            return $null
        }
        $payload = ConvertFrom-Json -InputObject $jsonText
        if (-not (Test-ProjectAtlasJsonObject $payload)) {
            return $null
        }
        $probePayload = $payload
    }
    catch {
        return $null
    }
    finally {
        $probeCleanupFailure = $null
        if ($process) {
            try {
                $process.Dispose()
            }
            catch {
                $probeCleanupFailure = $_
            }
        }
        $cleanupClock = [Diagnostics.Stopwatch]::StartNew()
        $remainingProbeFiles = @($probeFiles)
        try {
            do {
                Remove-Item -LiteralPath $standardOutput, $standardError -Force -ErrorAction SilentlyContinue
                $remainingProbeFiles = @($probeFiles | Where-Object { Test-Path -LiteralPath $_ })
                if ($remainingProbeFiles.Count -eq 0) {
                    break
                }
                Start-Sleep -Milliseconds 25
            }
            while ($cleanupClock.ElapsedMilliseconds -lt $probeTimeoutMs)
        }
        catch {
            if (-not $probeCleanupFailure) {
                $probeCleanupFailure = $_
            }
        }
        $probeCleanupSucceeded = -not $probeCleanupFailure `
            -and $remainingProbeFiles.Count -eq 0
    }
    if (-not $probeCleanupSucceeded) {
        return $null
    }
    return $probePayload
}

function Invoke-ProjectAtlasRuntimeInfo {
    param(
        [string]$FilePath
    )
    $payload = Invoke-ProjectAtlasBoundedJsonCommand `
        $FilePath `
        ([string[]]@("--format", "json", "runtime-info"))
    if (-not (Test-ProjectAtlasJsonObject $payload)) {
        return $null
    }
    if ($null -ne $payload.PSObject.Properties["runtime"]) {
        if (-not (Test-ProjectAtlasJsonObject $payload.runtime)) {
            return $null
        }
        return $payload.runtime
    }
    return $payload
}

function Test-ProjectAtlasRuntime {
    param(
        [string]$FilePath,
        [string]$ExpectedVersion
    )
    $runtime = Invoke-ProjectAtlasRuntimeInfo $FilePath
    if (-not (Test-ProjectAtlasJsonObject $runtime)) {
        return $false
    }
    $majorVersion = 0
    if (-not [int]::TryParse([string]$runtime.major_version, [ref]$majorVersion)) {
        return $false
    }
    $expectedRuntimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    $versionMatches = -not $expectedRuntimeVersion -or $runtime.version -eq $expectedRuntimeVersion
    return $runtime.project -eq "ProjectAtlas" `
        -and $majorVersion -ge 3 `
        -and @($runtime.capabilities) -contains "mcp" `
        -and $runtime.text_format -eq "TOON" `
        -and $versionMatches
}

function Get-ProjectAtlasRuntimeVersion {
    param(
        [string]$FilePath
    )
    $runtime = Invoke-ProjectAtlasRuntimeInfo $FilePath
    return $(if ($runtime) { $runtime.version } else { $null })
}

function Get-ProjectAtlasRuntimeImageSha256 {
    param(
        [string]$FilePath
    )
    try {
        Assert-ProjectAtlasDirectFilePath $FilePath "ProjectAtlas runtime image"
        Initialize-ProjectAtlasRuntimeProbe
        return [ProjectAtlas.Installer.ObsoleteMcpProcess]::ComputeImageSha256($FilePath)
    }
    catch {
        return $null
    }
}

function Test-ProjectAtlasAuthenticodeCodexSignature {
    param(
        [object]$Signature
    )
    if ($null -eq $Signature `
        -or -not ($Signature.Status -is [System.Management.Automation.SignatureStatus]) `
        -or $Signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid `
        -or -not ($Signature.SignatureType -is [System.Management.Automation.SignatureType]) `
        -or $Signature.SignatureType -ne [System.Management.Automation.SignatureType]::Authenticode `
        -or $null -eq $Signature.SignerCertificate) {
        return $false
    }
    $signerSimpleName = $Signature.SignerCertificate.GetNameInfo(
        [System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName,
        $false
    )
    return [string]::Equals(
        $signerSimpleName,
        "OpenAI OpCo, LLC",
        [System.StringComparison]::Ordinal
    )
}

function Test-ProjectAtlasStableCodexImageIdentity {
    param(
        [string]$ImageSha256BeforeSignature,
        [object]$Signature,
        [string]$ImageSha256AfterSignature
    )
    return -not [string]::IsNullOrWhiteSpace($ImageSha256BeforeSignature) `
        -and -not [string]::IsNullOrWhiteSpace($ImageSha256AfterSignature) `
        -and [string]::Equals(
            $ImageSha256BeforeSignature,
            $ImageSha256AfterSignature,
            [System.StringComparison]::OrdinalIgnoreCase
        ) `
        -and (Test-ProjectAtlasAuthenticodeCodexSignature $Signature)
}

function Get-ProjectAtlasCodexImageIdentity {
    param(
        [string]$FilePath
    )
    try {
        if ([string]::IsNullOrWhiteSpace($FilePath) `
            -or -not [System.IO.Path]::IsPathRooted($FilePath) `
            -or -not [string]::Equals(
                [System.IO.Path]::GetFileName($FilePath),
                "codex.exe",
                [System.StringComparison]::OrdinalIgnoreCase) `
            -or -not [System.IO.File]::Exists($FilePath)) {
            return $null
        }
        $imageSha256BeforeSignature = Get-ProjectAtlasRuntimeImageSha256 $FilePath
        if ([string]::IsNullOrWhiteSpace($imageSha256BeforeSignature)) {
            return $null
        }
        $signatureCommands = @(Microsoft.PowerShell.Core\Get-Command `
                'Microsoft.PowerShell.Security\Get-AuthenticodeSignature' `
                -CommandType Cmdlet `
                -ErrorAction Stop)
        if ($signatureCommands.Count -ne 1) {
            return $null
        }
        $signatureCommand = $signatureCommands[0]
        $trustedModuleRoot = [System.IO.Path]::GetFullPath(
            [System.IO.Path]::Combine($PSHOME, "Modules", "Microsoft.PowerShell.Security")
        ).TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
        $modulePath = if ($signatureCommand.Module) {
            [System.IO.Path]::GetFullPath([string]$signatureCommand.Module.Path)
        }
        else {
            $null
        }
        $trustedModulePrefix = $trustedModuleRoot + [System.IO.Path]::DirectorySeparatorChar
        if ($signatureCommand.CommandType -ne [System.Management.Automation.CommandTypes]::Cmdlet `
            -or -not [string]::Equals(
                $signatureCommand.ModuleName,
                "Microsoft.PowerShell.Security",
                [System.StringComparison]::Ordinal) `
            -or -not [string]::Equals(
                $signatureCommand.Source,
                "Microsoft.PowerShell.Security",
                [System.StringComparison]::Ordinal) `
            -or [string]::IsNullOrWhiteSpace($modulePath) `
            -or -not $modulePath.StartsWith(
                $trustedModulePrefix,
                [System.StringComparison]::OrdinalIgnoreCase)) {
            return $null
        }
        $signature = & $signatureCommand -LiteralPath $FilePath -ErrorAction Stop
        $imageSha256AfterSignature = Get-ProjectAtlasRuntimeImageSha256 $FilePath
        if (-not (Test-ProjectAtlasStableCodexImageIdentity `
                $imageSha256BeforeSignature `
                $signature `
                $imageSha256AfterSignature)) {
            return $null
        }
        return $imageSha256AfterSignature
    }
    catch {
        return $null
    }
}

function Find-ProjectAtlasObsoleteStableMcpProcess {
    param(
        [string]$StableMirrorPath,
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath,
        [string]$ExpectedVersion
    )
    $targetVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($targetVersion)) {
        return [pscustomobject]@{ State = "inspection_failed" }
    }
    try {
        Initialize-ProjectAtlasRuntimeProbe
        $processes = @(Get-CimInstance -ClassName Win32_Process -OperationTimeoutSec 5)
    }
    catch {
        Write-Warning "ProjectAtlas obsolete MCP handoff could not inspect Windows processes: $($_.Exception.Message)"
        return [pscustomobject]@{ State = "inspection_failed" }
    }

    $processesById = @{}
    foreach ($process in $processes) {
        $processId = 0
        try {
            $processId = [int]$process.ProcessId
            if ($processId -le 0) {
                continue
            }
            if ($processesById.ContainsKey($processId)) {
                throw "Windows process snapshot contains a duplicate process identity."
            }
            $processesById[$processId] = $process
        }
        catch {
            if ($processId -gt 0) {
                return [pscustomobject]@{ State = "inspection_failed" }
            }
        }
    }

    $stablePath = Get-NormalizedPathEntry $StableMirrorPath
    $exactObsoleteProcesses = @()
    $currentOwnerObserved = $false
    $unsafeOwnerObserved = $false
    foreach ($process in $processes) {
        if (-not [string]::Equals(
                [string]$process.Name,
                "projectatlas.exe",
                [System.StringComparison]::OrdinalIgnoreCase)) {
            continue
        }
        if ([string]::IsNullOrWhiteSpace([string]$process.ExecutablePath)) {
            $unsafeOwnerObserved = $true
            continue
        }
        if ((Get-NormalizedPathEntry ([string]$process.ExecutablePath)) -ne $stablePath) {
            continue
        }
        if ([string]::IsNullOrWhiteSpace([string]$process.CommandLine) `
            -or [string]::IsNullOrWhiteSpace([string]$process.CreationDate)) {
            $unsafeOwnerObserved = $true
            continue
        }
        $arguments = @()
        try {
            $arguments = @([ProjectAtlas.Installer.ObsoleteMcpProcess]::ParseCommandLine(
                    [string]$process.CommandLine
                ))
        }
        catch {
            $unsafeOwnerObserved = $true
            continue
        }
        if ($arguments.Count -lt 6 `
            -or [string]$arguments[1] -ne "--require-version" `
            -or [string]::IsNullOrWhiteSpace([string]$arguments[2]) `
            -or [string]$arguments[$arguments.Count - 1] -ne "mcp" `
            -or -not (Test-ProjectAtlasArgumentsUseAbsolutePaths ([string[]]$arguments))) {
            $unsafeOwnerObserved = $true
            continue
        }
        $observedVersion = Convert-ProjectAtlasVersionTag ([string]$arguments[2])
        $expectedLaunchArguments = Get-ProjectAtlasMcpLaunchArguments `
            $DbPath `
            $ProjectConfigPath `
            $FlatConfigPath `
            $observedVersion
        $expectedArguments = [string[]](@($StableMirrorPath) + $expectedLaunchArguments)
        if (-not (Test-ProjectAtlasExactArguments ([string[]]$arguments) $expectedArguments)) {
            continue
        }
        if ($observedVersion -eq $targetVersion) {
            $currentOwnerObserved = $true
            continue
        }
        try {
            $parentProcessId = [int]$process.ParentProcessId
            if ($parentProcessId -le 0) {
                throw "ProjectAtlas MCP process has no inspectable parent."
            }
            if (-not $processesById.ContainsKey($parentProcessId)) {
                throw "ProjectAtlas MCP parent process is not uniquely inspectable."
            }
            $parent = $processesById[$parentProcessId]
            if ([string]::IsNullOrWhiteSpace([string]$parent.ExecutablePath)) {
                throw "ProjectAtlas MCP parent identity is incomplete."
            }
            $parentPath = [string]$parent.ExecutablePath
            if (-not [System.IO.Path]::IsPathRooted($parentPath)) {
                throw "ProjectAtlas MCP parent executable path is not absolute."
            }
            if (-not [string]::Equals(
                    [System.IO.Path]::GetFileName($parentPath),
                    "codex.exe",
                    [System.StringComparison]::OrdinalIgnoreCase)) {
                continue
            }
            if ([string]::IsNullOrWhiteSpace([string]$parent.CommandLine) `
                -or [string]::IsNullOrWhiteSpace([string]$parent.CreationDate)) {
                throw "ProjectAtlas Codex parent identity is incomplete."
            }
            $creationFileTimeUtc = Convert-ProjectAtlasCimCreationFileTime ([object]$process.CreationDate)
            $parentCreationFileTimeUtc = Convert-ProjectAtlasCimCreationFileTime ([object]$parent.CreationDate)
            if ($parentCreationFileTimeUtc -gt $creationFileTimeUtc) {
                throw "ProjectAtlas Codex parent was created after the MCP child."
            }
            $parentArguments = @([ProjectAtlas.Installer.ObsoleteMcpProcess]::ParseCommandLine(
                    [string]$parent.CommandLine
                ))
            if ($parentArguments.Count -eq 0 `
                -or -not [System.IO.Path]::IsPathRooted([string]$parentArguments[0]) `
                -or -not [string]::Equals(
                    (Get-NormalizedPathEntry ([string]$parentArguments[0])),
                    (Get-NormalizedPathEntry $parentPath),
                    [System.StringComparison]::OrdinalIgnoreCase)) {
                throw "ProjectAtlas MCP parent command is not an absolute executable identity."
            }
            $parentImageSha256 = Get-ProjectAtlasCodexImageIdentity $parentPath
            if ([string]::IsNullOrWhiteSpace($parentImageSha256)) {
                throw "ProjectAtlas Codex parent Authenticode identity is not verified."
            }
            $exactObsoleteProcesses += [pscustomobject]@{
                ProcessId = [int]$process.ProcessId
                CreationFileTimeUtc = $creationFileTimeUtc
                Arguments = [string[]]$arguments
                InvokedVersion = $observedVersion
                ParentProcessId = $parentProcessId
                ParentCreationFileTimeUtc = $parentCreationFileTimeUtc
                ParentPath = $parentPath
                ParentArguments = [string[]]$parentArguments
                ParentImageSha256 = $parentImageSha256
            }
            if ($exactObsoleteProcesses.Count -gt 1) {
                return [pscustomobject]@{ State = "ambiguous" }
            }
        }
        catch {
            $unsafeOwnerObserved = $true
        }
    }
    if ($unsafeOwnerObserved) {
        return [pscustomobject]@{ State = "unsafe_owner" }
    }
    if ($exactObsoleteProcesses.Count -eq 0 -and $currentOwnerObserved) {
        return [pscustomobject]@{ State = "current_owner" }
    }
    if ($exactObsoleteProcesses.Count -eq 0) {
        return [pscustomobject]@{ State = "no_exact_owner" }
    }
    $imageSha256BeforeProbe = Get-ProjectAtlasRuntimeImageSha256 $StableMirrorPath
    $observedVersion = Convert-ProjectAtlasVersionTag (
        Get-ProjectAtlasRuntimeVersion $StableMirrorPath
    )
    $imageSha256AfterProbe = Get-ProjectAtlasRuntimeImageSha256 $StableMirrorPath
    if ([string]::IsNullOrWhiteSpace($observedVersion) `
        -or [string]::IsNullOrWhiteSpace($imageSha256BeforeProbe) `
        -or $imageSha256BeforeProbe -ne $imageSha256AfterProbe) {
        return [pscustomobject]@{ State = "unsafe_owner" }
    }
    if ($observedVersion -eq $targetVersion) {
        return [pscustomobject]@{ State = "current_owner" }
    }
    $selection = $exactObsoleteProcesses[0]
    $selection | Add-Member -NotePropertyName State -NotePropertyValue "exact"
    $selection | Add-Member -NotePropertyName ObservedVersion -NotePropertyValue $observedVersion
    $selection | Add-Member -NotePropertyName ImageSha256 -NotePropertyValue $imageSha256AfterProbe
    return $selection
}

function Invoke-ProjectAtlasObsoleteStableMcpHandoff {
    param(
        [string]$StableMirrorPath,
        [string]$VerifiedRuntimePath,
        [string]$ExpectedVersion,
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath,
        [string]$McpConfigPath,
        [string]$ClaudeMcpConfigPath,
        [string]$OpenCodeConfigPath,
        [string]$ExpectedMcpConfigSha256,
        [string]$ExpectedClaudeMcpConfigSha256,
        [string]$ExpectedOpenCodeConfigSha256
    )
    $targetImageSha256BeforeProbe = Get-ProjectAtlasRuntimeImageSha256 $VerifiedRuntimePath
    $targetRuntimeVerified = Test-ProjectAtlasRuntime $VerifiedRuntimePath $ExpectedVersion
    $targetImageSha256AfterProbe = Get-ProjectAtlasRuntimeImageSha256 $VerifiedRuntimePath
    if (-not $targetRuntimeVerified `
        -or [string]::IsNullOrWhiteSpace($targetImageSha256BeforeProbe) `
        -or $targetImageSha256BeforeProbe -ne $targetImageSha256AfterProbe) {
        Write-Warning "ProjectAtlas obsolete MCP handoff refused because the target versioned runtime is not verified."
        return "target_not_verified"
    }
    $replacementConfigDigests = @{
        $McpConfigPath = $ExpectedMcpConfigSha256
        $ClaudeMcpConfigPath = $ExpectedClaudeMcpConfigSha256
        $OpenCodeConfigPath = $ExpectedOpenCodeConfigSha256
    }
    try {
        foreach ($configPath in @($McpConfigPath, $ClaudeMcpConfigPath, $OpenCodeConfigPath)) {
            Assert-ProjectAtlasDirectFilePath $configPath "ProjectAtlas generated MCP config"
            if ([string]::IsNullOrWhiteSpace([string]$replacementConfigDigests[$configPath]) `
                -or (Get-ProjectAtlasSha256 $configPath) -ne $replacementConfigDigests[$configPath]) {
                throw "ProjectAtlas generated MCP config no longer matches its validated snapshot."
            }
        }
    }
    catch {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: replacement readiness changed before it could be captured. Codex and all ProjectAtlas processes remain running."
        return "replacement_readiness_changed"
    }
    try {
        Assert-ProjectAtlasDirectFilePath $StableMirrorPath "ProjectAtlas stable runtime mirror"
        $selection = Find-ProjectAtlasObsoleteStableMcpProcess `
            $StableMirrorPath `
            $DbPath `
            $ProjectConfigPath `
            $FlatConfigPath `
            $ExpectedVersion
    }
    catch {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial because the stable runtime mirror could not be inspected safely. Repair the invalid mirror path and rerun this installer. Codex and all ProjectAtlas processes remain running."
        return "inspection_failed"
    }
    if ($selection.State -ne "exact") {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: process_owner=$($selection.State). Codex and all ProjectAtlas processes remain running."
        return [string]$selection.State
    }
    if (-not (Test-ProjectAtlasCodexPluginReady $ExpectedVersion)) {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: the target Codex plugin skill was not verified. Codex and all ProjectAtlas processes remain running."
        return "codex_plugin_not_verified"
    }
    if (-not (Test-ProjectAtlasCodexMcpRegistryReady `
            $VerifiedRuntimePath `
            $ExpectedVersion `
            $DbPath `
            $ProjectConfigPath `
            $FlatConfigPath)) {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: the target Codex MCP registry entry was not verified. Codex and all ProjectAtlas processes remain running."
        return "codex_registry_not_verified"
    }
    $finalImageSha256BeforeProbe = Get-ProjectAtlasRuntimeImageSha256 $StableMirrorPath
    $finalObservedVersion = Convert-ProjectAtlasVersionTag (
        Get-ProjectAtlasRuntimeVersion $StableMirrorPath
    )
    $finalImageSha256AfterProbe = Get-ProjectAtlasRuntimeImageSha256 $StableMirrorPath
    if ($finalObservedVersion -ne $selection.ObservedVersion) {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: process_owner=identity_changed_version. Codex and all ProjectAtlas processes remain running."
        return "identity_changed_version"
    }
    if ([string]::IsNullOrWhiteSpace($finalImageSha256BeforeProbe) `
        -or $finalImageSha256BeforeProbe -ne $finalImageSha256AfterProbe `
        -or $finalImageSha256AfterProbe -ne $selection.ImageSha256) {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: process_owner=identity_changed_file. Codex and all ProjectAtlas processes remain running."
        return "identity_changed_file"
    }
    $finalParentImageSha256 = Get-ProjectAtlasCodexImageIdentity $selection.ParentPath
    if ([string]::IsNullOrWhiteSpace($finalParentImageSha256) `
        -or $finalParentImageSha256 -ne $selection.ParentImageSha256) {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: replacement or Codex owner readiness changed before retirement. Codex and all ProjectAtlas processes remain running."
        return "replacement_readiness_changed"
    }
    if (-not (Test-ProjectAtlasCodexPluginReady $ExpectedVersion) `
        -or -not (Test-ProjectAtlasCodexMcpRegistryReady `
            $VerifiedRuntimePath `
            $ExpectedVersion `
            $DbPath `
            $ProjectConfigPath `
            $FlatConfigPath)) {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: replacement readiness changed before retirement. Codex and all ProjectAtlas processes remain running."
        return "replacement_readiness_changed"
    }
    $finalTargetImageSha256BeforeProbe = Get-ProjectAtlasRuntimeImageSha256 $VerifiedRuntimePath
    $finalTargetRuntimeVerified = Test-ProjectAtlasRuntime $VerifiedRuntimePath $ExpectedVersion
    $finalTargetImageSha256AfterProbe = Get-ProjectAtlasRuntimeImageSha256 $VerifiedRuntimePath
    if (-not $finalTargetRuntimeVerified `
        -or [string]::IsNullOrWhiteSpace($finalTargetImageSha256BeforeProbe) `
        -or $finalTargetImageSha256BeforeProbe -ne $finalTargetImageSha256AfterProbe `
        -or $finalTargetImageSha256AfterProbe -ne $targetImageSha256AfterProbe) {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: replacement readiness changed before retirement. Codex and all ProjectAtlas processes remain running."
        return "replacement_readiness_changed"
    }
    try {
        foreach ($configPath in @($McpConfigPath, $ClaudeMcpConfigPath, $OpenCodeConfigPath)) {
            Assert-ProjectAtlasDirectFilePath $configPath "ProjectAtlas generated MCP config"
            if ((Get-ProjectAtlasSha256 $configPath) -ne $replacementConfigDigests[$configPath]) {
                throw "ProjectAtlas generated MCP config changed."
            }
        }
    }
    catch {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: replacement readiness changed before retirement. Codex and all ProjectAtlas processes remain running."
        return "replacement_readiness_changed"
    }
    $result = [ProjectAtlas.Installer.ObsoleteMcpProcess]::Retire(
        $selection.ProcessId,
        $selection.CreationFileTimeUtc,
        $StableMirrorPath,
        [string[]]$selection.Arguments,
        $selection.ImageSha256,
        $selection.ParentProcessId,
        $selection.ParentCreationFileTimeUtc,
        $selection.ParentPath,
        [string[]]$selection.ParentArguments,
        $selection.ParentImageSha256,
        5000
    )
    if ($result.State -eq "retired") {
        Write-Host "Retired exact obsolete Codex-owned ProjectAtlas MCP process $($selection.ProcessId) invoked with version $($selection.InvokedVersion) for database $DbPath."
        return "retired"
    }
    if ($result.ErrorCode -ne 0) {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: process_owner=$($result.State) win32_error=$($result.ErrorCode). Codex and unrelated processes remain running."
    }
    else {
        Write-Warning "ProjectAtlas obsolete MCP handoff remained partial: process_owner=$($result.State). Codex and unrelated processes remain running."
    }
    return [string]$result.State
}

function Get-KnownProjectAtlasShimPaths {
    $paths = @()
    if ($env:USERPROFILE) {
        $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
        $paths += @(
            (Join-Path $cargoBin "projectatlas.exe"),
            (Join-Path $cargoBin "projectatlas.cmd"),
            (Join-Path $cargoBin "projectatlas.ps1")
        )
    }
    if ($env:APPDATA) {
        $npmBin = Join-Path $env:APPDATA "npm"
        $paths += @(
            (Join-Path $npmBin "projectatlas.exe"),
            (Join-Path $npmBin "projectatlas.cmd"),
            (Join-Path $npmBin "projectatlas.ps1"),
            (Join-Path $npmBin "projectatlas")
        )
    }
    return @($paths | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
}

function Test-KnownProjectAtlasShimPath {
    param(
        [string]$FilePath
    )
    if (-not $FilePath) {
        return $false
    }
    $normalized = Get-NormalizedPathEntry $FilePath
    foreach ($knownPath in (Get-KnownProjectAtlasShimPaths)) {
        if ($normalized -eq (Get-NormalizedPathEntry $knownPath)) {
            return $true
        }
    }
    return $false
}

function New-ProjectAtlasShimQuarantinePath {
    param(
        [string]$FilePath,
        [string]$Version
    )
    $safeVersion = if ([string]::IsNullOrWhiteSpace($Version)) { "unknown" } else { $Version -replace '[^A-Za-z0-9_.-]', '_' }
    $basePath = "$FilePath.projectatlas-stale-$safeVersion.bak"
    if (-not (Test-Path -LiteralPath $basePath)) {
        return $basePath
    }
    $timestampPath = "$basePath.$(Get-Date -Format 'yyyyMMddHHmmss')"
    if (-not (Test-Path -LiteralPath $timestampPath)) {
        return $timestampPath
    }
    return "$timestampPath.$([Guid]::NewGuid().ToString('N'))"
}

function Quarantine-ProjectAtlasStaleShims {
    param(
        [string]$VerifiedPath,
        [string]$ExpectedVersion
    )
    $expectedRuntimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if (-not $VerifiedPath -or -not $expectedRuntimeVersion) {
        return
    }
    $verified = Get-NormalizedPathEntry $VerifiedPath
    $candidates = @()
    $candidates += @(where.exe projectatlas 2>$null | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $candidates += Get-KnownProjectAtlasShimPaths
    $seen = @{}
    foreach ($candidate in $candidates) {
        if (-not (Test-Path -LiteralPath $candidate)) {
            continue
        }
        $normalized = Get-NormalizedPathEntry $candidate
        if ($normalized -eq $verified -or $seen.ContainsKey($normalized)) {
            continue
        }
        $seen[$normalized] = $true
        if (-not (Test-KnownProjectAtlasShimPath $candidate)) {
            continue
        }
        if (-not (Test-ProjectAtlasRuntime $candidate $null)) {
            continue
        }
        $version = Get-ProjectAtlasRuntimeVersion $candidate
        if ([string]::IsNullOrWhiteSpace($version) -or $version -eq $expectedRuntimeVersion) {
            continue
        }
        try {
            $quarantinePath = New-ProjectAtlasShimQuarantinePath $candidate $version
            Move-Item -LiteralPath $candidate -Destination $quarantinePath
            Write-Output "Quarantined stale ProjectAtlas shim: $candidate -> $quarantinePath version '$version'"
        }
        catch {
            Write-Warning "Could not quarantine stale ProjectAtlas shim ${candidate} version '$version': $($_.Exception.Message)"
        }
    }
}

function Split-PathList {
    param(
        [string]$Value
    )
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return @()
    }
    return $Value -split ";" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
}

function Get-NormalizedPathEntry {
    param(
        [string]$Value
    )
    try {
        return ([System.IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables($Value))).TrimEnd("\")
    }
    catch {
        return $Value.TrimEnd("\")
    }
}

function Set-ProjectAtlasProcessPathPrecedence {
    param(
        [string]$FilePath
    )
    $runtimeDir = Split-Path -Parent $FilePath
    if (-not $runtimeDir) {
        return
    }

    $normalizedRuntimeDir = Get-NormalizedPathEntry $runtimeDir

    $processEntries = Split-PathList $env:Path
    $processEntries = @($processEntries | Where-Object { (Get-NormalizedPathEntry $_) -ne $normalizedRuntimeDir })
    $env:Path = (@($runtimeDir) + $processEntries) -join ";"
}

function Test-ProjectAtlasBareCommandResolutionOnPath {
    param(
        [string]$PathValue,
        [string]$VerifiedPath
    )
    $installerProcessPath = $env:Path
    try {
        $env:Path = [Environment]::ExpandEnvironmentVariables($PathValue)
        $command = Get-Command projectatlas -ErrorAction SilentlyContinue | Select-Object -First 1
        return $command `
            -and (Get-NormalizedPathEntry $command.Source) -eq (Get-NormalizedPathEntry $VerifiedPath)
    }
    finally {
        $env:Path = $installerProcessPath
    }
}

function Test-ProjectAtlasPersistedBareCommandResolution {
    param(
        [string]$VerifiedPath
    )
    $machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    return (Test-ProjectAtlasBareCommandResolutionOnPath (@($machinePath, $userPath) -join ";") $VerifiedPath)
}

function Set-ProjectAtlasPathPrecedence {
    param(
        [string]$FilePath
    )
    Set-ProjectAtlasProcessPathPrecedence $FilePath
    $runtimeDir = Split-Path -Parent $FilePath
    if (-not $runtimeDir) {
        return $false
    }

    $normalizedRuntimeDir = Get-NormalizedPathEntry $runtimeDir

    if (Test-Truthy $env:PROJECTATLAS_SKIP_USER_PATH_UPDATE) {
        return $false
    }

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $userEntries = Split-PathList $userPath
    $userEntries = @($userEntries | Where-Object { (Get-NormalizedPathEntry $_) -ne $normalizedRuntimeDir })
    $futureUserPath = (@($runtimeDir) + $userEntries) -join ";"
    [Environment]::SetEnvironmentVariable("Path", $futureUserPath, "User")
    return (Test-ProjectAtlasPersistedBareCommandResolution $FilePath)
}

function Confirm-ProjectAtlasBareCommandResolution {
    param(
        [string]$VerifiedPath,
        [string]$ExpectedVersion
    )
    if (-not $VerifiedPath) {
        return
    }
    $verified = Get-NormalizedPathEntry $VerifiedPath
    $projectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue
    if (-not $projectAtlasCommand) {
        Write-Warning "Active process still cannot resolve bare 'projectatlas'. Generated MCP configs use the verified absolute runtime: $VerifiedPath. Restart Codex or the shell before relying on bare projectatlas."
        return
    }
    $commandPath = $projectAtlasCommand.Source
    if ((Get-NormalizedPathEntry $commandPath) -eq $verified -and (Test-ProjectAtlasRuntime $commandPath $ExpectedVersion)) {
        Write-Output "Active process resolves bare projectatlas to verified runtime: $commandPath"
        return
    }
    $commandVersion = Get-ProjectAtlasRuntimeVersion $commandPath
    Write-Warning "Active process still resolves bare 'projectatlas' to $commandPath version '$commandVersion', not the verified runtime $VerifiedPath. Generated MCP configs use the absolute runtime; restart Codex or the shell, put $(Split-Path -Parent $VerifiedPath) first on PATH, or remove the obsolete shim before relying on bare projectatlas."
}

function Sync-ProjectAtlasRuntimeToLocalAppData {
    param(
        [string]$FilePath,
        [string]$ExpectedVersion
    )
    $synchronizationVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if (-not $synchronizationVersion) {
        $synchronizationVersion = Convert-ProjectAtlasVersionTag (Get-ProjectAtlasRuntimeVersion $FilePath)
    }
    if (-not $synchronizationVersion -or -not (Test-ProjectAtlasRuntime $FilePath $synchronizationVersion)) {
        return $false
    }
    $installDir = Join-Path $env:LOCALAPPDATA "ProjectAtlas\bin"
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    $target = Join-Path $installDir "projectatlas.exe"
    if (Test-ProjectAtlasRuntime $target $synchronizationVersion) {
        return $true
    }
    if ((Get-NormalizedPathEntry $FilePath) -ne (Get-NormalizedPathEntry $target)) {
        try {
            Copy-Item -LiteralPath $FilePath -Destination $target -Force
        }
        catch {
            Write-Warning "ProjectAtlas LocalAppData mirror is locked: $($_.Exception.Message) The installer will verify durable absolute MCP configuration before attempting an exact obsolete-child handoff. Codex MCP and generated configs continue to use verified runtime $FilePath."
            return $false
        }
    }
    return (Test-ProjectAtlasRuntime $target $synchronizationVersion)
}

function Find-ProjectAtlas {
    param(
        [string]$ExpectedVersion
    )
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "ProjectAtlas\bin\projectatlas.exe"),
        (Join-Path $env:USERPROFILE ".cargo\bin\projectatlas.exe")
    )
    foreach ($candidate in $candidates) {
        if (Test-ProjectAtlasRuntime $candidate $ExpectedVersion) {
            return $candidate
        }
    }
    $projectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue
    if ($projectAtlasCommand -and (Test-ProjectAtlasRuntime $projectAtlasCommand.Source $ExpectedVersion)) {
        return $projectAtlasCommand.Source
    }
    return $null
}

function Write-ProjectAtlasPathShadowReport {
    param(
        [string]$VerifiedPath,
        [string]$ExpectedVersion
    )
    if (-not $VerifiedPath) {
        return
    }
    $verified = Get-NormalizedPathEntry $VerifiedPath
    $candidates = @(where.exe projectatlas 2>$null | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($candidates.Count -eq 0) {
        Write-Warning "Bare 'projectatlas' is not on PATH. Generated MCP configs use the verified absolute runtime: $VerifiedPath"
        return
    }
    $first = Get-NormalizedPathEntry $candidates[0]
    if ($first -ne $verified) {
        $firstVersion = Get-ProjectAtlasRuntimeVersion $candidates[0]
        Write-Warning "Bare 'projectatlas' resolves to $($candidates[0]) version '$firstVersion', not the verified runtime $VerifiedPath. Start a new shell, put $(Split-Path -Parent $VerifiedPath) first on PATH, or remove the obsolete shim."
    }
    foreach ($candidate in $candidates) {
        $normalized = Get-NormalizedPathEntry $candidate
        if ($normalized -eq $verified) {
            continue
        }
        if (-not (Test-ProjectAtlasRuntime $candidate $ExpectedVersion)) {
            $version = Get-ProjectAtlasRuntimeVersion $candidate
            Write-Warning "Obsolete ProjectAtlas runtime or shim still exists on PATH: $candidate version '$version'. The installer retires only an exact obsolete MCP owner of the stable mirror; start a fresh host or remove an unused shim if this path still shadows the verified runtime $VerifiedPath."
        }
    }
}

function Invoke-Checked {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

function Get-ProjectAtlasMcpLaunchArguments {
    param(
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath,
        [string]$ExpectedVersion
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        return @()
    }
    $launchArgs = @("--require-version", $runtimeVersion, "--db", $DbPath)
    if (Test-Path -LiteralPath $ProjectConfigPath) {
        $launchArgs += @("--config", $ProjectConfigPath)
    }
    elseif (Test-Path -LiteralPath $FlatConfigPath) {
        $launchArgs += @("--config", $FlatConfigPath)
    }
    $launchArgs += "mcp"
    return $launchArgs
}

function Get-ProjectAtlasTokenLaunchArguments {
    param(
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath,
        [string]$ExpectedVersion
    )
    $launchArgs = @(Get-ProjectAtlasMcpLaunchArguments $DbPath $ProjectConfigPath $FlatConfigPath $ExpectedVersion)
    if ($launchArgs.Count -eq 0) {
        return @()
    }
    $launchArgs[$launchArgs.Count - 1] = "token"
    $launchArgs += @("--view", "tui")
    return $launchArgs
}

function Convert-ProjectAtlasCimCreationFileTime {
    param(
        [object]$CreationDate
    )
    $creationTime = if ($CreationDate -is [datetime]) {
        ([datetime]$CreationDate).ToUniversalTime()
    }
    else {
        [System.Management.ManagementDateTimeConverter]::ToDateTime(
            [string]$CreationDate
        ).ToUniversalTime()
    }
    return [long]$creationTime.ToFileTimeUtc()
}

function Test-ProjectAtlasArgumentsUseAbsolutePaths {
    param(
        [string[]]$Arguments
    )
    if (-not $Arguments -or $Arguments.Count -eq 0 `
        -or -not [System.IO.Path]::IsPathRooted([string]$Arguments[0])) {
        return $false
    }
    $pathValueExpected = $false
    foreach ($argument in @($Arguments | Select-Object -Skip 1)) {
        if ($pathValueExpected) {
            if (-not [System.IO.Path]::IsPathRooted([string]$argument)) {
                return $false
            }
            $pathValueExpected = $false
            continue
        }
        $pathValueExpected = [string]$argument -eq "--db" `
            -or [string]$argument -eq "--config"
    }
    return -not $pathValueExpected
}

function Test-ProjectAtlasExactArguments {
    param(
        [string[]]$Actual,
        [string[]]$Expected
    )
    if (-not (Test-ProjectAtlasArgumentsUseAbsolutePaths $Actual) `
        -or -not (Test-ProjectAtlasArgumentsUseAbsolutePaths $Expected) `
        -or $Actual.Count -ne $Expected.Count) {
        return $false
    }
    for ($index = 0; $index -lt $Expected.Count; $index += 1) {
        $pathValue = $index -eq 0 `
            -or ($index -gt 0 `
                -and ([string]$Expected[$index - 1] -eq "--db" `
                    -or [string]$Expected[$index - 1] -eq "--config"))
        if ($pathValue) {
            $actualValue = Get-NormalizedPathEntry ([string]$Actual[$index])
            $expectedValue = Get-NormalizedPathEntry ([string]$Expected[$index])
            if (-not [string]::Equals(
                    $actualValue,
                    $expectedValue,
                    [System.StringComparison]::OrdinalIgnoreCase)) {
                return $false
            }
            continue
        }
        if (-not [string]::Equals(
                [string]$Actual[$index],
                [string]$Expected[$index],
                [System.StringComparison]::Ordinal)) {
            return $false
        }
    }
    return $true
}

function Get-ProjectAtlasComparablePath {
    param(
        [string]$Path
    )
    if ([string]::IsNullOrWhiteSpace($Path)) {
        return ""
    }
    return [System.IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
}

function Assert-ProjectAtlasEquivalentPath {
    param(
        [string]$Actual,
        [string]$Expected,
        [string]$Label
    )
    if ([string]::IsNullOrWhiteSpace($Actual)) {
        throw "${Label} is missing."
    }
    if (-not [System.IO.Path]::IsPathRooted($Actual)) {
        throw "${Label} path is not absolute: $Actual"
    }
    $actualPath = Get-ProjectAtlasComparablePath $Actual
    $expectedPath = Get-ProjectAtlasComparablePath $Expected
    if (-not [string]::Equals($actualPath, $expectedPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "${Label} path mismatch: expected $Expected, found $Actual"
    }
}

function Get-ProjectAtlasSha256FromBytes {
    param(
        [byte[]]$Bytes
    )
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $sha256.ComputeHash($Bytes)
        return ([System.BitConverter]::ToString($hash) -replace '-', '').ToLowerInvariant()
    }
    finally {
        $sha256.Dispose()
    }
}

function Test-ProjectAtlasJsonStringArray {
    param(
        [object]$Value
    )
    if (-not ($Value -is [System.Collections.IList])) {
        return $false
    }
    foreach ($entry in $Value) {
        if ($entry -isnot [string]) {
            return $false
        }
    }
    return $true
}

function Confirm-ProjectAtlasGeneratedMcpConfig {
    param(
        [string]$ConfigPath,
        [string]$Harness,
        [string]$VerifiedPath,
        [string]$ExpectedVersion,
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath,
        [string]$ProjectRoot
    )
    if (-not [System.IO.File]::Exists($ConfigPath)) {
        throw "${Harness} ProjectAtlas generated MCP config was not written: $ConfigPath"
    }
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        $runtimeVersion = Get-ProjectAtlasRuntimeVersion $VerifiedPath
    }
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        throw "${Harness} ProjectAtlas generated MCP config cannot be verified because the runtime version is unknown."
    }
    $configBytes = [System.IO.File]::ReadAllBytes($ConfigPath)
    $utf8 = [System.Text.UTF8Encoding]::new($false, $true)
    $configText = $utf8.GetString($configBytes)
    if (-not $configText.TrimStart().StartsWith("{", [System.StringComparison]::Ordinal)) {
        throw "${Harness} generated MCP config root must be a JSON object."
    }
    $config = ConvertFrom-Json -InputObject $configText
    if (-not (Test-ProjectAtlasJsonObject $config)) {
        throw "${Harness} generated MCP config root must be a JSON object."
    }
    if ($Harness -eq "Codex" -or $Harness -eq "Claude Code") {
        $server = $config.mcpServers.projectatlas
        if (-not (Test-ProjectAtlasJsonObject $config.mcpServers) `
            -or -not (Test-ProjectAtlasJsonObject $server)) {
            throw "${Harness} generated MCP config is missing mcpServers.projectatlas."
        }
        if (-not (Test-ProjectAtlasJsonStringArray $server.args)) {
            throw "${Harness} generated MCP config args must be a JSON array of strings."
        }
        Assert-ProjectAtlasEquivalentPath ([string]$server.command) $VerifiedPath "${Harness} command"
        $arguments = @($server.args)
        if ($Harness -eq "Codex") {
            Assert-ProjectAtlasEquivalentPath ([string]$server.cwd) $ProjectRoot "Codex cwd"
        }
        elseif ($server.PSObject.Properties.Name -contains "cwd") {
            throw "Claude Code generated MCP config must not rely on cwd."
        }
    }
    elseif ($Harness -eq "OpenCode") {
        $server = $config.mcp.projectatlas
        if (-not (Test-ProjectAtlasJsonObject $config.mcp) `
            -or -not (Test-ProjectAtlasJsonObject $server)) {
            throw "OpenCode generated MCP config is missing mcp.projectatlas."
        }
        if (-not ($server.type -is [string]) -or $server.type -ne "local") {
            throw "OpenCode generated MCP config type mismatch: expected local, found $($server.type)"
        }
        if (-not ($server.enabled -is [bool]) -or -not $server.enabled) {
            throw "OpenCode generated MCP config must set enabled=true."
        }
        if (-not (Test-ProjectAtlasJsonStringArray $server.command)) {
            throw "OpenCode generated MCP config command must be a JSON array of strings."
        }
        Assert-ProjectAtlasEquivalentPath ([string]$server.cwd) $ProjectRoot "OpenCode cwd"
        $command = @($server.command)
        if ($command.Count -lt 2) {
            throw "OpenCode generated MCP config command array is incomplete."
        }
        Assert-ProjectAtlasEquivalentPath ([string]$command[0]) $VerifiedPath "OpenCode command"
        $arguments = @($command | Select-Object -Skip 1)
    }
    else {
        throw "Unsupported generated MCP config harness: $Harness"
    }
    $expectedArguments = Get-ProjectAtlasMcpLaunchArguments `
        $DbPath `
        $ProjectConfigPath `
        $FlatConfigPath `
        $runtimeVersion
    if (-not (Test-ProjectAtlasExactArguments `
            ([string[]](@($VerifiedPath) + $arguments)) `
            ([string[]](@($VerifiedPath) + $expectedArguments)))) {
        throw "${Harness} generated MCP config command arguments do not exactly match the verified runtime launch contract."
    }
    Write-Host "${Harness} ProjectAtlas generated MCP config verified for runtime $VerifiedPath and database $DbPath."
    return (Get-ProjectAtlasSha256FromBytes $configBytes)
}

function Test-ProjectAtlasGeneratedMcpConfigReadiness {
    param(
        [string[]]$ConfigPaths,
        [string[]]$ExpectedSha256
    )
    try {
        if ($ConfigPaths.Count -eq 0 `
            -or $ConfigPaths.Count -ne $ExpectedSha256.Count) {
            return $false
        }
        for ($index = 0; $index -lt $ConfigPaths.Count; $index++) {
            Assert-ProjectAtlasDirectFilePath $ConfigPaths[$index] "ProjectAtlas generated MCP config"
            if ([string]::IsNullOrWhiteSpace($ExpectedSha256[$index]) `
                -or (Get-ProjectAtlasSha256 $ConfigPaths[$index]) -ne $ExpectedSha256[$index]) {
                return $false
            }
        }
        return $true
    }
    catch {
        return $false
    }
}

function Resolve-ProjectAtlasCodexCommand {
    param(
        [string]$Operation
    )
    $codexCommandPath = $null
    if (-not [string]::IsNullOrWhiteSpace($env:PROJECTATLAS_CODEX_COMMAND)) {
        $codexCommandPath = (Resolve-Path $env:PROJECTATLAS_CODEX_COMMAND -ErrorAction SilentlyContinue).Path
        if (-not $codexCommandPath) {
            $codexCommand = Get-Command $env:PROJECTATLAS_CODEX_COMMAND -ErrorAction SilentlyContinue
            if ($codexCommand) {
                $codexCommandPath = $codexCommand.Source
            }
        }
        if (-not $codexCommandPath) {
            Write-Warning "${Operation} skipped: PROJECTATLAS_CODEX_COMMAND does not resolve."
            return $null
        }
    }
    else {
        $codexCommand = Get-Command codex -ErrorAction SilentlyContinue
        if ($codexCommand) {
            $codexCommandPath = $codexCommand.Source
        }
    }
    if (-not $codexCommandPath) {
        Write-Host "${Operation} skipped: codex command not found."
        return $null
    }
    return $codexCommandPath
}

function Test-ProjectAtlasOfficialMarketplaceSource {
    param(
        [string]$Source
    )
    if ([string]::IsNullOrWhiteSpace($Source)) {
        return $false
    }
    $normalized = $Source.Trim().TrimEnd("/")
    $allowedSources = @(
        "styler-ai/ProjectAtlas",
        "styler-ai/ProjectAtlas.git",
        "https://github.com/styler-ai/ProjectAtlas",
        "https://github.com/styler-ai/ProjectAtlas.git",
        "git@github.com:styler-ai/ProjectAtlas",
        "git@github.com:styler-ai/ProjectAtlas.git",
        "ssh://git@github.com/styler-ai/ProjectAtlas",
        "ssh://git@github.com/styler-ai/ProjectAtlas.git"
    )
    foreach ($allowed in $allowedSources) {
        if ([string]::Equals($allowed, $normalized, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    return $false
}

function Get-ProjectAtlasCodexConfigPath {
    if (-not [string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
        return Join-Path $env:CODEX_HOME "config.toml"
    }
    if (-not [string]::IsNullOrWhiteSpace($env:USERPROFILE)) {
        return Join-Path $env:USERPROFILE ".codex\config.toml"
    }
    return $null
}

function Get-ProjectAtlasCodexMarketplaceRef {
    $configPath = Get-ProjectAtlasCodexConfigPath
    if (-not $configPath -or -not (Test-Path -LiteralPath $configPath)) {
        return $null
    }
    $inProjectAtlasMarketplace = $false
    foreach ($line in Get-Content -LiteralPath $configPath) {
        if ($line -match '^\s*\[marketplaces\.projectatlas\]\s*$') {
            $inProjectAtlasMarketplace = $true
            continue
        }
        if ($inProjectAtlasMarketplace -and $line -match '^\s*\[') {
            break
        }
        if ($inProjectAtlasMarketplace -and $line -match '^\s*ref\s*=\s*["'']([^"'']+)["'']') {
            return $Matches[1]
        }
    }
    return $null
}

function Test-ProjectAtlasCodexContainedPath {
    param(
        [string]$Path,
        [string]$Root
    )
    if ([string]::IsNullOrWhiteSpace($Path) -or [string]::IsNullOrWhiteSpace($Root)) {
        return $false
    }
    $fullRoot = [System.IO.Path]::GetFullPath($Root).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $prefix = $fullRoot + [System.IO.Path]::DirectorySeparatorChar
    return $fullPath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)
}

function Assert-ProjectAtlasCodexDirectAncestry {
    param(
        [string]$Path,
        [string]$Description,
        [string]$CodexRoot
    )
    $fullRoot = [System.IO.Path]::GetFullPath($CodexRoot).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    if (-not [string]::Equals($fullPath, $fullRoot, [System.StringComparison]::OrdinalIgnoreCase) `
        -and -not (Test-ProjectAtlasCodexContainedPath $fullPath $fullRoot)) {
        throw "$Description '$Path' is outside the Codex state root '$CodexRoot'"
    }

    $currentPath = $fullRoot
    while (-not [string]::IsNullOrWhiteSpace($currentPath)) {
        $item = Get-Item -Force -LiteralPath $currentPath -ErrorAction SilentlyContinue
        if ($item -and (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
            throw "$Description has a symlink, junction, or reparse point in its Codex-root ancestry: $currentPath"
        }
        $parentPath = Split-Path -Parent $currentPath
        if ([string]::IsNullOrWhiteSpace($parentPath) `
            -or [string]::Equals($parentPath, $currentPath, [System.StringComparison]::OrdinalIgnoreCase)) {
            break
        }
        $currentPath = $parentPath
    }
    if ([string]::Equals($fullPath, $fullRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        return
    }
    $relativePath = $fullPath.Substring($fullRoot.Length).TrimStart(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )

    $currentPath = $fullRoot
    $segments = $relativePath.Split(
        [char[]]@([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar),
        [System.StringSplitOptions]::RemoveEmptyEntries
    )
    for ($index = 0; $index -lt $segments.Count; $index++) {
        $currentPath = Join-Path $currentPath $segments[$index]
        $item = Get-Item -Force -LiteralPath $currentPath -ErrorAction SilentlyContinue
        if (-not $item) {
            continue
        }
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "$Description has a symlink, junction, or reparse point in its Codex-root ancestry: $currentPath"
        }
        if ($index -lt ($segments.Count - 1) -and -not ($item -is [System.IO.DirectoryInfo])) {
            throw "$Description has a non-directory ancestor: $currentPath"
        }
    }
}

function Assert-ProjectAtlasCodexRestorableFile {
    param(
        [string]$Path,
        [string]$Description,
        [string]$CodexRoot
    )
    Assert-ProjectAtlasCodexDirectAncestry $Path $Description $CodexRoot
    Assert-ProjectAtlasDirectFilePath $Path $Description
    $item = Get-Item -Force -LiteralPath $Path -ErrorAction Stop
    if ($item.PSObject.Properties.Name -contains "LinkType" `
        -and [string]::Equals($item.LinkType, "HardLink", [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Description is hard linked"
    }
}

function Assert-ProjectAtlasCodexRestorableDirectory {
    param(
        [string]$Path,
        [string]$Description,
        [string]$CodexRoot
    )
    Assert-ProjectAtlasCodexDirectAncestry $Path $Description $CodexRoot
    $rootItem = Get-Item -Force -LiteralPath $Path -ErrorAction Stop
    if (-not ($rootItem -is [System.IO.DirectoryInfo]) `
        -or (($rootItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "$Description is not a direct directory"
    }
    foreach ($item in Get-ChildItem -Force -LiteralPath $Path -Recurse) {
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "$Description contains a reparse point"
        }
        if ($item.PSObject.Properties.Name -contains "LinkType" `
            -and [string]::Equals($item.LinkType, "HardLink", [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "$Description contains a hard-linked file"
        }
    }
}

function Enter-ProjectAtlasCodexPluginUpdateLock {
    param(
        [string]$ConfigPath
    )
    $mutex = $null
    try {
        if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
            throw "Codex config path cannot be resolved"
        }
        $codexRoot = [System.IO.Path]::GetFullPath((Split-Path -Parent $ConfigPath))
        $normalizedRoot = $codexRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar).ToUpperInvariant()
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            $digest = $sha256.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($normalizedRoot))
        }
        finally {
            $sha256.Dispose()
        }
        $digestText = ([System.BitConverter]::ToString($digest)).Replace("-", "")
        $mutex = [System.Threading.Mutex]::new(
            $false,
            "Global\ProjectAtlas-CodexPluginUpdate-$digestText"
        )
        $acquired = $false
        try {
            $acquired = $mutex.WaitOne([System.TimeSpan]::FromSeconds(30))
        }
        catch [System.Threading.AbandonedMutexException] {
            $acquired = $true
        }
        if (-not $acquired) {
            $mutex.Dispose()
            Write-Warning "Codex ProjectAtlas plugin update skipped: another installer still owns the update lock."
            return $null
        }
        return [pscustomobject]@{
            Mutex = $mutex
            Root = $codexRoot
        }
    }
    catch {
        if ($mutex) {
            $mutex.Dispose()
        }
        Write-Warning "Codex ProjectAtlas plugin update skipped: the update lock could not be acquired safely: $($_.Exception.Message)"
        return $null
    }
}

function Exit-ProjectAtlasCodexPluginUpdateLock {
    param(
        [object]$Lock
    )
    if (-not $Lock -or -not $Lock.Mutex) {
        return
    }
    try {
        $Lock.Mutex.ReleaseMutex()
    }
    finally {
        $Lock.Mutex.Dispose()
    }
}

function New-ProjectAtlasCodexStateSnapshot {
    param(
        [object]$ProjectAtlasPlugin,
        [string]$ExpectedVersion
    )
    $configPath = Get-ProjectAtlasCodexConfigPath
    if ([string]::IsNullOrWhiteSpace($configPath)) {
        Write-Warning "Codex ProjectAtlas plugin update skipped: Codex config path cannot be resolved."
        return $null
    }
    $snapshotRoot = $null
    $codexRoot = $null
    try {
        $configExisted = Test-Path -LiteralPath $configPath
        $codexRoot = [System.IO.Path]::GetFullPath((Split-Path -Parent $configPath))
        Assert-ProjectAtlasCodexDirectAncestry $configPath "Codex config" $codexRoot
        if ($configExisted) {
            Assert-ProjectAtlasCodexRestorableFile $configPath "Codex config" $codexRoot
        }

        if ($ExpectedVersion -notmatch '^[0-9A-Za-z][0-9A-Za-z.+-]*$') {
            throw "expected projectatlas plugin version cannot identify its Codex cache safely"
        }
        $expectedMarketplaceRoot = Join-Path $codexRoot ".tmp\marketplaces\projectatlas"
        $expectedPluginSourcePath = Join-Path $expectedMarketplaceRoot "plugins\projectatlas"
        $pluginSourcePath = [System.IO.Path]::GetFullPath($expectedPluginSourcePath)
        Assert-ProjectAtlasCodexDirectAncestry `
            $pluginSourcePath `
            "installed projectatlas plugin source" `
            $codexRoot
        $reportedPluginSourcePath = Get-ProjectAtlasCodexPluginSourcePath $ProjectAtlasPlugin
        if ($ProjectAtlasPlugin -and [string]::IsNullOrWhiteSpace($reportedPluginSourcePath)) {
            throw "installed projectatlas plugin source path is unavailable"
        }
        if ($ProjectAtlasPlugin -and -not [string]::Equals(
                [System.IO.Path]::GetFullPath($reportedPluginSourcePath),
                $pluginSourcePath,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            throw "installed projectatlas plugin source does not use the expected Codex marketplace layout"
        }

        $pluginSourceExisted = Test-Path -LiteralPath $pluginSourcePath
        if ($pluginSourceExisted) {
            Assert-ProjectAtlasCodexRestorableDirectory `
                $pluginSourcePath `
                "installed projectatlas plugin source" `
                $codexRoot
        }
        elseif ($ProjectAtlasPlugin) {
            throw "installed projectatlas plugin source is missing"
        }

        $pluginCachePath = $null
        $pluginCacheExisted = $false
        if ($ProjectAtlasPlugin) {
            if (-not ($ProjectAtlasPlugin.version -is [string]) `
                -or $ProjectAtlasPlugin.version -notmatch '^[0-9A-Za-z][0-9A-Za-z.+-]*$') {
                throw "installed projectatlas plugin version cannot identify its Codex cache safely"
            }
            $pluginCachePath = Join-Path $codexRoot (
                "plugins\cache\projectatlas\projectatlas\" + $ProjectAtlasPlugin.version
            )
            $pluginCacheExisted = Test-Path -LiteralPath $pluginCachePath
            if (-not $pluginCacheExisted) {
                throw "installed projectatlas plugin cache is missing"
            }
            Assert-ProjectAtlasCodexRestorableDirectory `
                $pluginCachePath `
                "installed projectatlas plugin cache" `
                $codexRoot
        }
        $expectedPluginCachePath = Join-Path $codexRoot (
            "plugins\cache\projectatlas\projectatlas\" + $ExpectedVersion
        )
        Assert-ProjectAtlasCodexDirectAncestry `
            $expectedPluginCachePath `
            "expected projectatlas plugin cache" `
            $codexRoot
        $expectedPluginCacheExisted = Test-Path -LiteralPath $expectedPluginCachePath
        if ($expectedPluginCacheExisted `
            -and ($null -eq $pluginCachePath `
                -or -not [string]::Equals(
                    [System.IO.Path]::GetFullPath($expectedPluginCachePath),
                    [System.IO.Path]::GetFullPath($pluginCachePath),
                    [System.StringComparison]::OrdinalIgnoreCase
                ))) {
            Assert-ProjectAtlasCodexRestorableDirectory `
                $expectedPluginCachePath `
                "expected projectatlas plugin cache" `
                $codexRoot
        }

        $marketplaceManifestPath = Join-Path $expectedMarketplaceRoot ".agents\plugins\marketplace.json"
        $marketplaceInstallRecordPath = Join-Path $expectedMarketplaceRoot ".codex-marketplace-install.json"
        Assert-ProjectAtlasCodexDirectAncestry `
            $marketplaceManifestPath `
            "ProjectAtlas marketplace manifest" `
            $codexRoot
        Assert-ProjectAtlasCodexDirectAncestry `
            $marketplaceInstallRecordPath `
            "ProjectAtlas marketplace install record" `
            $codexRoot
        $marketplaceManifestExisted = Test-Path -LiteralPath $marketplaceManifestPath
        $marketplaceInstallRecordExisted = Test-Path -LiteralPath $marketplaceInstallRecordPath
        if ($marketplaceManifestExisted) {
            Assert-ProjectAtlasCodexRestorableFile `
                $marketplaceManifestPath `
                "ProjectAtlas marketplace manifest" `
                $codexRoot
        }
        elseif ($ProjectAtlasPlugin) {
            throw "ProjectAtlas marketplace manifest is missing"
        }
        if ($marketplaceInstallRecordExisted) {
            Assert-ProjectAtlasCodexRestorableFile `
                $marketplaceInstallRecordPath `
                "ProjectAtlas marketplace install record" `
                $codexRoot
        }
        elseif ($ProjectAtlasPlugin) {
            throw "ProjectAtlas marketplace install record is missing"
        }

        $snapshotRoot = Join-Path $codexRoot (".projectatlas-plugin-state-" + [guid]::NewGuid().ToString("N"))
        Assert-ProjectAtlasCodexDirectAncestry $snapshotRoot "Codex state snapshot" $codexRoot
        New-Item -ItemType Directory -Path $snapshotRoot -ErrorAction Stop | Out-Null
        Assert-ProjectAtlasCodexDirectAncestry $snapshotRoot "Codex state snapshot" $codexRoot
        if ($configExisted) {
            [System.IO.File]::Copy($configPath, (Join-Path $snapshotRoot "config.toml"), $false)
        }
        if ($pluginSourceExisted) {
            Copy-Item -LiteralPath $pluginSourcePath -Destination (Join-Path $snapshotRoot "plugin-source") -Recurse -Force
        }
        if ($pluginCacheExisted) {
            Copy-Item -LiteralPath $pluginCachePath -Destination (Join-Path $snapshotRoot "plugin-cache") -Recurse -Force
        }
        $cachePathsMatch = $pluginCachePath -and [string]::Equals(
            [System.IO.Path]::GetFullPath($pluginCachePath),
            [System.IO.Path]::GetFullPath($expectedPluginCachePath),
            [System.StringComparison]::OrdinalIgnoreCase
        )
        if ($expectedPluginCacheExisted -and -not $cachePathsMatch) {
            Copy-Item -LiteralPath $expectedPluginCachePath -Destination (Join-Path $snapshotRoot "expected-plugin-cache") -Recurse -Force
        }
        if ($marketplaceManifestExisted) {
            [System.IO.File]::Copy(
                $marketplaceManifestPath,
                (Join-Path $snapshotRoot "marketplace.json"),
                $false
            )
        }
        if ($marketplaceInstallRecordExisted) {
            [System.IO.File]::Copy(
                $marketplaceInstallRecordPath,
                (Join-Path $snapshotRoot "marketplace-install.json"),
                $false
            )
        }
        return [pscustomobject]@{
            Root                         = $snapshotRoot
            ConfigPath                   = $configPath
            ConfigExisted                = $configExisted
            CodexRoot                    = $codexRoot
            PluginSourcePath             = $pluginSourcePath
            PluginSourceExisted          = $pluginSourceExisted
            PluginCachePath              = $pluginCachePath
            PluginCacheExisted           = $pluginCacheExisted
            ExpectedPluginCachePath      = $expectedPluginCachePath
            ExpectedPluginCacheExisted   = $expectedPluginCacheExisted
            CachePathsMatch              = $cachePathsMatch
            MarketplaceManifestPath      = $marketplaceManifestPath
            MarketplaceManifestExisted   = $marketplaceManifestExisted
            MarketplaceInstallRecordPath = $marketplaceInstallRecordPath
            MarketplaceInstallRecordExisted = $marketplaceInstallRecordExisted
        }
    }
    catch {
        if ($snapshotRoot -and (Test-Path -LiteralPath $snapshotRoot)) {
            Remove-ProjectAtlasCodexStateSnapshot ([pscustomobject]@{
                    Root      = $snapshotRoot
                    CodexRoot = $codexRoot
                }) | Out-Null
        }
        Write-Warning "Codex ProjectAtlas plugin update skipped: existing integration could not be preserved safely: $($_.Exception.Message)"
        return $null
    }
}

function Restore-ProjectAtlasCodexStateSnapshot {
    param(
        [object]$Snapshot
    )
    try {
        if (-not $Snapshot -or -not (Test-Path -LiteralPath $Snapshot.Root)) {
            return $false
        }
        $currentCodexRoot = [System.IO.Path]::GetFullPath((Split-Path -Parent $Snapshot.ConfigPath))
        if (-not [string]::Equals(
                $currentCodexRoot,
                $Snapshot.CodexRoot,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            return $false
        }
        Assert-ProjectAtlasDirectPath $currentCodexRoot "Codex state root"
        Assert-ProjectAtlasCodexDirectAncestry $Snapshot.Root "Codex state snapshot" $currentCodexRoot
        $directoryRestores = @(
                    [pscustomobject]@{
                        Destination = $Snapshot.PluginSourcePath
                        Snapshot    = (Join-Path $Snapshot.Root "plugin-source")
                        Description = "Codex plugin source"
                        Existed     = $Snapshot.PluginSourceExisted
                    },
                    [pscustomobject]@{
                        Destination = $Snapshot.PluginCachePath
                        Snapshot    = (Join-Path $Snapshot.Root "plugin-cache")
                        Description = "Codex plugin cache"
                        Existed     = $Snapshot.PluginCacheExisted
                    }
                )
        if (-not $Snapshot.CachePathsMatch) {
            $directoryRestores += [pscustomobject]@{
                Destination = $Snapshot.ExpectedPluginCachePath
                Snapshot    = (Join-Path $Snapshot.Root "expected-plugin-cache")
                Description = "expected Codex plugin cache"
                Existed     = $Snapshot.ExpectedPluginCacheExisted
            }
        }
        foreach ($directoryRestore in @($directoryRestores | Where-Object {
                    -not [string]::IsNullOrWhiteSpace($_.Destination)
                })) {
                if ($directoryRestore.Existed) {
                Assert-ProjectAtlasCodexRestorableDirectory `
                    $directoryRestore.Snapshot `
                    ($directoryRestore.Description + " snapshot") `
                    $currentCodexRoot
                }
                Assert-ProjectAtlasCodexDirectAncestry `
                    $directoryRestore.Destination `
                    $directoryRestore.Description `
                    $currentCodexRoot
                $destinationParent = Split-Path -Parent $directoryRestore.Destination
                New-Item -ItemType Directory -Path $destinationParent -Force | Out-Null
                Assert-ProjectAtlasCodexDirectAncestry `
                    $directoryRestore.Destination `
                    $directoryRestore.Description `
                    $currentCodexRoot
                if (Test-Path -LiteralPath $directoryRestore.Destination) {
                    Assert-ProjectAtlasCodexRestorableDirectory `
                        $directoryRestore.Destination `
                        $directoryRestore.Description `
                        $currentCodexRoot
                    Remove-Item -LiteralPath $directoryRestore.Destination -Recurse -Force
                }
                Assert-ProjectAtlasCodexDirectAncestry `
                    $directoryRestore.Destination `
                    $directoryRestore.Description `
                    $currentCodexRoot
                if ($directoryRestore.Existed) {
                    Copy-Item `
                        -LiteralPath $directoryRestore.Snapshot `
                        -Destination $directoryRestore.Destination `
                        -Recurse `
                        -Force
                }
            }
            foreach ($fileRestore in @(
                    [pscustomobject]@{
                        Destination = $Snapshot.MarketplaceManifestPath
                        Snapshot    = (Join-Path $Snapshot.Root "marketplace.json")
                        Description = "ProjectAtlas marketplace manifest"
                        Existed     = $Snapshot.MarketplaceManifestExisted
                    },
                    [pscustomobject]@{
                        Destination = $Snapshot.MarketplaceInstallRecordPath
                        Snapshot    = (Join-Path $Snapshot.Root "marketplace-install.json")
                        Description = "ProjectAtlas marketplace install record"
                        Existed     = $Snapshot.MarketplaceInstallRecordExisted
                    }
                )) {
                if ($fileRestore.Existed) {
                Assert-ProjectAtlasCodexRestorableFile `
                    $fileRestore.Snapshot `
                    ($fileRestore.Description + " snapshot") `
                    $currentCodexRoot
                }
                Assert-ProjectAtlasCodexDirectAncestry `
                    $fileRestore.Destination `
                    $fileRestore.Description `
                    $currentCodexRoot
                $destinationParent = Split-Path -Parent $fileRestore.Destination
                New-Item -ItemType Directory -Path $destinationParent -Force | Out-Null
                Assert-ProjectAtlasCodexDirectAncestry `
                    $fileRestore.Destination `
                    $fileRestore.Description `
                    $currentCodexRoot
                if (Test-Path -LiteralPath $fileRestore.Destination) {
                    Assert-ProjectAtlasCodexRestorableFile `
                        $fileRestore.Destination `
                        $fileRestore.Description `
                        $currentCodexRoot
                    if (-not $fileRestore.Existed) {
                        Remove-Item -LiteralPath $fileRestore.Destination -Force
                        if (Test-Path -LiteralPath $fileRestore.Destination) {
                            throw "$($fileRestore.Description) removal did not complete"
                        }
                    }
                }
                if ($fileRestore.Existed) {
                    $temporaryPath = $fileRestore.Destination + ".projectatlas-restore-" + [guid]::NewGuid().ToString("N")
                    Assert-ProjectAtlasCodexDirectAncestry $temporaryPath $fileRestore.Description $currentCodexRoot
                    [System.IO.File]::Copy($fileRestore.Snapshot, $temporaryPath, $false)
                    Assert-ProjectAtlasCodexDirectAncestry $fileRestore.Destination $fileRestore.Description $currentCodexRoot
                    Move-Item -LiteralPath $temporaryPath -Destination $fileRestore.Destination -Force
                }
            }
        if ($Snapshot.ConfigExisted) {
            Assert-ProjectAtlasCodexRestorableFile `
                (Join-Path $Snapshot.Root "config.toml") `
                "Codex config snapshot" `
                $currentCodexRoot
            Assert-ProjectAtlasCodexDirectAncestry $Snapshot.ConfigPath "Codex config" $currentCodexRoot
            if (Test-Path -LiteralPath $Snapshot.ConfigPath) {
                Assert-ProjectAtlasCodexRestorableFile $Snapshot.ConfigPath "Codex config" $currentCodexRoot
            }
            $temporaryConfigPath = $Snapshot.ConfigPath + ".projectatlas-restore-" + [guid]::NewGuid().ToString("N")
            Assert-ProjectAtlasCodexDirectAncestry $temporaryConfigPath "Codex config" $currentCodexRoot
            [System.IO.File]::Copy((Join-Path $Snapshot.Root "config.toml"), $temporaryConfigPath, $false)
            Assert-ProjectAtlasCodexDirectAncestry $Snapshot.ConfigPath "Codex config" $currentCodexRoot
            Move-Item -LiteralPath $temporaryConfigPath -Destination $Snapshot.ConfigPath -Force
        }
        elseif (Test-Path -LiteralPath $Snapshot.ConfigPath) {
            Assert-ProjectAtlasCodexRestorableFile $Snapshot.ConfigPath "Codex config" $currentCodexRoot
            Remove-Item -LiteralPath $Snapshot.ConfigPath -Force
            if (Test-Path -LiteralPath $Snapshot.ConfigPath) {
                throw "Codex config removal did not complete"
            }
        }
        return $true
    }
    catch {
        Write-Warning "Codex ProjectAtlas plugin update failed and its preserved local state could not be restored completely; the recovery snapshot was retained at '$($Snapshot.Root)': $($_.Exception.Message)"
        return $false
    }
}

function Remove-ProjectAtlasCodexStateSnapshot {
    param(
        [object]$Snapshot
    )
    if (-not $Snapshot -or -not (Get-Item -Force -LiteralPath $Snapshot.Root -ErrorAction SilentlyContinue)) {
        return $true
    }
    try {
        Assert-ProjectAtlasCodexRestorableDirectory `
            $Snapshot.Root `
            "Codex state snapshot" `
            $Snapshot.CodexRoot
    }
    catch {
        Write-Warning "Codex ProjectAtlas state snapshot cleanup refused/path changed at '$($Snapshot.Root)': $($_.Exception.Message)"
        return $false
    }
    try {
        Remove-Item -LiteralPath $Snapshot.Root -Recurse -Force -ErrorAction Stop
        if (Test-Path -LiteralPath $Snapshot.Root) {
            throw "snapshot still exists after cleanup"
        }
        return $true
    }
    catch {
        try {
            Assert-ProjectAtlasCodexRestorableDirectory `
                $Snapshot.Root `
                "Codex state snapshot" `
                $Snapshot.CodexRoot
            Write-Warning "Codex ProjectAtlas state snapshot cleanup failed; retained at '$($Snapshot.Root)': $($_.Exception.Message)"
        }
        catch {
            Write-Warning "Codex ProjectAtlas state snapshot cleanup failed and no trusted retained path can be reported at '$($Snapshot.Root)'."
        }
        return $false
    }
}

function Get-ProjectAtlasCodexPluginInventory {
    param(
        [string]$CodexCommandPath
    )
    $unavailable = [pscustomobject]@{
        Complete = $false
        Plugin   = $null
    }
    try {
        $plugins = Invoke-ProjectAtlasBoundedJsonCommand `
            $CodexCommandPath `
            ([string[]]@("plugin", "list", "--marketplace", "projectatlas", "--json"))
        if (-not (Test-ProjectAtlasJsonObject $plugins)) {
            return $unavailable
        }
        if (-not ($plugins.installed -is [System.Collections.IList])) {
            return $unavailable
        }
        $projectAtlasPlugins = @()
        foreach ($pluginEntry in $plugins.installed) {
            if (-not (Test-ProjectAtlasJsonObject $pluginEntry)) {
                return $unavailable
            }
            $projectAtlasIndicator = ($pluginEntry.pluginId -is [string] `
                    -and [string]::Equals($pluginEntry.pluginId, "projectatlas@projectatlas", [System.StringComparison]::Ordinal)) `
                -or (($pluginEntry.name -is [string]) `
                    -and [string]::Equals($pluginEntry.name, "projectatlas", [System.StringComparison]::Ordinal) `
                    -and ($pluginEntry.marketplaceName -is [string]) `
                    -and [string]::Equals($pluginEntry.marketplaceName, "projectatlas", [System.StringComparison]::Ordinal))
            if ($projectAtlasIndicator) {
                $projectAtlasPlugins += $pluginEntry
            }
        }
        if ($projectAtlasPlugins.Count -eq 0) {
            return [pscustomobject]@{
                Complete = $true
                Plugin   = $null
            }
        }
        if ($projectAtlasPlugins.Count -ne 1) {
            return $unavailable
        }
        $plugin = $projectAtlasPlugins[0]
        if (-not ($plugin.pluginId -is [string]) `
            -or -not [string]::Equals($plugin.pluginId, "projectatlas@projectatlas", [System.StringComparison]::Ordinal) `
            -or -not ($plugin.name -is [string]) `
            -or -not [string]::Equals($plugin.name, "projectatlas", [System.StringComparison]::Ordinal) `
            -or -not ($plugin.marketplaceName -is [string]) `
            -or -not [string]::Equals($plugin.marketplaceName, "projectatlas", [System.StringComparison]::Ordinal) `
            -or -not ($plugin.version -is [string]) `
            -or -not ($plugin.installed -is [bool]) `
            -or -not $plugin.installed `
            -or -not ($plugin.enabled -is [bool]) `
            -or -not $plugin.enabled `
            -or [string]::IsNullOrWhiteSpace($plugin.version) `
            -or $plugin.version -notmatch '^[0-9A-Za-z][0-9A-Za-z.+-]*$' `
            -or -not (Test-ProjectAtlasJsonObject $plugin.marketplaceSource) `
            -or -not ($plugin.marketplaceSource.source -is [string]) `
            -or -not (Test-ProjectAtlasOfficialMarketplaceSource $plugin.marketplaceSource.source) `
            -or -not (Test-ProjectAtlasJsonObject $plugin.source) `
            -or -not ($plugin.source.path -is [string]) `
            -or [string]::IsNullOrWhiteSpace($plugin.source.path) `
            -or -not [System.IO.Path]::IsPathRooted($plugin.source.path)) {
            return $unavailable
        }
        return [pscustomobject]@{
            Complete = $true
            Plugin   = $plugin
        }
    }
    catch {
        return $unavailable
    }
    return $unavailable
}

function Get-ProjectAtlasCodexPlugin {
    param(
        [string]$CodexCommandPath
    )
    $inventory = Get-ProjectAtlasCodexPluginInventory $CodexCommandPath
    if ($inventory.Complete) {
        return $inventory.Plugin
    }
    return $null
}

function Get-ProjectAtlasCodexPluginVersion {
    param(
        [string]$CodexCommandPath
    )
    $projectAtlasPlugin = Get-ProjectAtlasCodexPlugin $CodexCommandPath
    if ($projectAtlasPlugin -and $projectAtlasPlugin.version -is [string]) {
        return $projectAtlasPlugin.version
    }
    return $null
}

function Get-ProjectAtlasCodexPluginSourcePath {
    param(
        [object]$ProjectAtlasPlugin
    )
    if (-not $ProjectAtlasPlugin) {
        return $null
    }
    foreach ($candidate in @($ProjectAtlasPlugin.source.path, $ProjectAtlasPlugin.path, $ProjectAtlasPlugin.root, $ProjectAtlasPlugin.location)) {
        if ($candidate -is [string] -and -not [string]::IsNullOrWhiteSpace($candidate)) {
            return $candidate
        }
    }
    return $null
}

function Get-ProjectAtlasCodexPluginSourceManifestVersion {
    param(
        [object]$ProjectAtlasPlugin
    )
    $pluginSourcePath = Get-ProjectAtlasCodexPluginSourcePath $ProjectAtlasPlugin
    if ([string]::IsNullOrWhiteSpace($pluginSourcePath)) {
        return $null
    }
    $manifestPath = Join-Path $pluginSourcePath ".codex-plugin\plugin.json"
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        return ""
    }
    try {
        $manifestText = Get-Content -LiteralPath $manifestPath -Raw
        if (-not $manifestText.TrimStart().StartsWith(
                "{",
                [System.StringComparison]::Ordinal
            )) {
            return ""
        }
        $manifest = ConvertFrom-Json -InputObject $manifestText
        if ((Test-ProjectAtlasJsonObject $manifest) -and $manifest.version -is [string]) {
            return $manifest.version
        }
    }
    catch {
        return ""
    }
    return ""
}

function Test-ProjectAtlasCodexPluginSourceManifest {
    param(
        [object]$ProjectAtlasPlugin,
        [string]$ExpectedVersion
    )
    $pluginSourcePath = Get-ProjectAtlasCodexPluginSourcePath $ProjectAtlasPlugin
    if ([string]::IsNullOrWhiteSpace($pluginSourcePath)) {
        return $false
    }
    return (Get-ProjectAtlasCodexPluginSourceManifestVersion $ProjectAtlasPlugin) -eq $ExpectedVersion
}

function Confirm-ProjectAtlasCodexSkillArtifact {
    param(
        [string]$CodexCommandPath,
        [string]$ExpectedVersion
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        Write-Output "Codex ProjectAtlas plugin skill verification skipped: ProjectAtlas version is unknown."
        return
    }
    $projectAtlasPlugin = Get-ProjectAtlasCodexPlugin $CodexCommandPath
    if (-not $projectAtlasPlugin) {
        Write-Warning "Codex ProjectAtlas plugin skill verification skipped: projectatlas plugin is not installed."
        return
    }
    if ($projectAtlasPlugin.version -ne $runtimeVersion) {
        Write-Warning "Codex ProjectAtlas plugin skill verification failed: installed projectatlas plugin version '$($projectAtlasPlugin.version)' does not match $runtimeVersion."
        return
    }
    $pluginSourcePath = Get-ProjectAtlasCodexPluginSourcePath $projectAtlasPlugin
    if ([string]::IsNullOrWhiteSpace($pluginSourcePath)) {
        Write-Output "Codex ProjectAtlas plugin skill version $runtimeVersion is installed; Codex does not expose the active in-process ProjectAtlas skill path. Restart Codex if this session still advertises an older ProjectAtlas skill."
        return
    }
    $manifestPath = Join-Path $pluginSourcePath ".codex-plugin\plugin.json"
    $skillPath = Join-Path $pluginSourcePath "skills\projectatlas\SKILL.md"
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        Write-Warning "Codex ProjectAtlas plugin skill verification failed: plugin manifest was not found at $manifestPath."
        return
    }
    if (-not (Test-Path -LiteralPath $skillPath)) {
        Write-Warning "Codex ProjectAtlas plugin skill verification failed: ProjectAtlas skill was not found at $skillPath."
        return
    }
    $manifestVersion = Get-ProjectAtlasCodexPluginSourceManifestVersion $projectAtlasPlugin
    if ($manifestVersion -ne $runtimeVersion) {
        Write-Warning "Codex ProjectAtlas plugin skill verification failed: manifest version '$manifestVersion' does not match $runtimeVersion."
        return
    }
    Write-Output "Codex ProjectAtlas plugin skill verified at $skillPath for $runtimeVersion."
    Write-Output "Codex does not expose the active in-process ProjectAtlas skill path; restart Codex if this session still advertises an older ProjectAtlas skill."
}

function Get-ProjectAtlasCodexMarketplace {
    param(
        [object]$MarketplacePayload
    )
    if (-not (Test-ProjectAtlasJsonObject $MarketplacePayload) `
        -or -not ($MarketplacePayload.marketplaces -is [System.Collections.IList])) {
        return $null
    }
    $projectAtlasMarketplaces = @()
    foreach ($marketplaceEntry in $MarketplacePayload.marketplaces) {
        if (-not (Test-ProjectAtlasJsonObject $marketplaceEntry)) {
            return $null
        }
        if ($marketplaceEntry.name -is [string] `
            -and [string]::Equals($marketplaceEntry.name, "projectatlas", [System.StringComparison]::Ordinal)) {
            $projectAtlasMarketplaces += $marketplaceEntry
        }
    }
    if ($projectAtlasMarketplaces.Count -ne 1) {
        return $null
    }
    $marketplace = $projectAtlasMarketplaces[0]
    if (-not (Test-ProjectAtlasJsonObject $marketplace.marketplaceSource) `
        -or -not ($marketplace.marketplaceSource.source -is [string])) {
        return $null
    }
    return $marketplace
}

$script:ProjectAtlasCodexPluginUpdatePreservedPriorState = $false

function Update-ProjectAtlasCodexPlugin {
    param(
        [string]$ExpectedVersion
    )
    if (Test-Truthy $env:PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE) {
        Write-Output "Codex ProjectAtlas plugin update skipped by PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE."
        return
    }
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        Write-Output "Codex ProjectAtlas plugin update skipped: ProjectAtlas version is unknown."
        return
    }
    $codexCommandPath = Resolve-ProjectAtlasCodexCommand "Codex ProjectAtlas plugin update"
    if (-not $codexCommandPath) {
        return
    }
    $updateLock = Enter-ProjectAtlasCodexPluginUpdateLock (Get-ProjectAtlasCodexConfigPath)
    if (-not $updateLock) {
        $script:ProjectAtlasCodexPluginUpdatePreservedPriorState = $true
        return
    }
    try {
        $retainedSnapshot = $null
        if (Test-Path -LiteralPath $updateLock.Root) {
            Assert-ProjectAtlasDirectPath $updateLock.Root "Codex state root"
            $retainedSnapshot = Get-ChildItem `
                -Force `
                -LiteralPath $updateLock.Root `
                -Filter ".projectatlas-plugin-state-*" `
                -ErrorAction Stop `
                | Select-Object -First 1
        }
        if ($retainedSnapshot) {
            $script:ProjectAtlasCodexPluginUpdatePreservedPriorState = $true
            Write-Warning "Codex ProjectAtlas plugin update skipped: retained recovery state requires inspection at '$($retainedSnapshot.FullName)'."
            return
        }
        $marketplacePayload = Invoke-ProjectAtlasBoundedJsonCommand `
            $codexCommandPath `
            ([string[]]@("plugin", "marketplace", "list", "--json"))
        if (-not $marketplacePayload) {
            Write-Output "Codex ProjectAtlas plugin update skipped: could not list Codex plugin marketplaces."
            return
        }
        $projectAtlasMarketplace = Get-ProjectAtlasCodexMarketplace $marketplacePayload
        if (-not $projectAtlasMarketplace) {
            Write-Output "Codex ProjectAtlas plugin update skipped: projectatlas marketplace is not configured."
            return
        }
        $source = if ($projectAtlasMarketplace.marketplaceSource) { $projectAtlasMarketplace.marketplaceSource.source } else { $null }
        if (-not (Test-ProjectAtlasOfficialMarketplaceSource $source)) {
            Write-Output "Codex ProjectAtlas plugin update skipped: projectatlas marketplace is not the official styler-ai/ProjectAtlas source."
            return
        }

        $releaseTag = "v$runtimeVersion"
        $previousRef = Get-ProjectAtlasCodexMarketplaceRef
        $pluginInventory = Get-ProjectAtlasCodexPluginInventory $codexCommandPath
        if (-not $pluginInventory.Complete) {
            $script:ProjectAtlasCodexPluginUpdatePreservedPriorState = $true
            Write-Warning "Codex ProjectAtlas plugin update skipped: installed plugin inventory could not be verified completely; the existing official integration was not changed."
            return
        }
        $projectAtlasPlugin = $pluginInventory.Plugin
        $currentPluginVersion = if ($projectAtlasPlugin -and $projectAtlasPlugin.version -is [string]) { $projectAtlasPlugin.version } else { $null }
        $currentSourceManifestMatches = Test-ProjectAtlasCodexPluginSourceManifest $projectAtlasPlugin $runtimeVersion
        $currentPluginReady = Test-ProjectAtlasCodexPluginReady $ExpectedVersion
        if ($previousRef -eq $releaseTag `
            -and $currentPluginVersion -eq $runtimeVersion `
            -and $currentSourceManifestMatches `
            -and $currentPluginReady) {
            Write-Output "Codex ProjectAtlas plugin marketplace already points to $releaseTag."
            Confirm-ProjectAtlasCodexSkillArtifact $codexCommandPath $ExpectedVersion
            return
        }
        $stateSnapshot = New-ProjectAtlasCodexStateSnapshot $projectAtlasPlugin $runtimeVersion
        if (-not $stateSnapshot) {
            $script:ProjectAtlasCodexPluginUpdatePreservedPriorState = $true
            return
        }
        $updateSucceeded = $false
        $restoreSucceeded = $false
        try {
            if ($previousRef -eq $releaseTag) {
            if ($currentPluginVersion -eq $runtimeVersion -and -not $currentSourceManifestMatches) {
                $sourceManifestVersion = Get-ProjectAtlasCodexPluginSourceManifestVersion $projectAtlasPlugin
                Write-Output "Codex ProjectAtlas plugin source manifest version '$sourceManifestVersion' does not match $runtimeVersion; refreshing official projectatlas plugin cache."
            }
            elseif ($currentPluginVersion -eq $runtimeVersion -and -not $currentPluginReady) {
                Write-Output "Codex ProjectAtlas plugin skill artifact does not match $runtimeVersion; refreshing official projectatlas plugin cache."
            }
            & $codexCommandPath plugin remove projectatlas --marketplace projectatlas --json | Out-Null
            & $codexCommandPath plugin add projectatlas --marketplace projectatlas --json | Out-Null
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "Codex ProjectAtlas plugin update failed: could not install projectatlas plugin at $releaseTag."
                return
            }
            $installedInventory = Get-ProjectAtlasCodexPluginInventory $codexCommandPath
            if (-not $installedInventory.Complete -or -not $installedInventory.Plugin) {
                Write-Warning "Codex ProjectAtlas plugin update failed: installed plugin inventory could not be verified completely after refresh."
                return
            }
            $installedVersion = $installedInventory.Plugin.version
            if ($installedVersion -ne $runtimeVersion) {
                Write-Warning "Codex ProjectAtlas plugin update failed: installed projectatlas plugin version '$installedVersion' does not match $runtimeVersion."
                return
            }
            $installedPlugin = $installedInventory.Plugin
            if (-not (Test-ProjectAtlasCodexPluginSourceManifest $installedPlugin $runtimeVersion)) {
                $sourceManifestVersion = Get-ProjectAtlasCodexPluginSourceManifestVersion $installedPlugin
                Write-Warning "Codex ProjectAtlas plugin update failed: source manifest version '$sourceManifestVersion' does not match $runtimeVersion after refresh."
                return
            }
            $updateSucceeded = $true
            Write-Output "Codex ProjectAtlas plugin marketplace updated to $releaseTag."
            Confirm-ProjectAtlasCodexSkillArtifact $codexCommandPath $ExpectedVersion
            return
        }

        & $codexCommandPath plugin marketplace remove projectatlas --json | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex ProjectAtlas plugin update failed: could not remove stale projectatlas marketplace."
            return
        }
        & $codexCommandPath plugin marketplace add styler-ai/ProjectAtlas --ref $releaseTag --json | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex ProjectAtlas plugin update failed: could not add projectatlas marketplace at $releaseTag."
            return
        }
        & $codexCommandPath plugin remove projectatlas --marketplace projectatlas --json | Out-Null
        & $codexCommandPath plugin add projectatlas --marketplace projectatlas --json | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex ProjectAtlas plugin update failed: could not install projectatlas plugin at $releaseTag."
            return
        }
        $installedInventory = Get-ProjectAtlasCodexPluginInventory $codexCommandPath
        if (-not $installedInventory.Complete -or -not $installedInventory.Plugin) {
            Write-Warning "Codex ProjectAtlas plugin update failed: installed plugin inventory could not be verified completely after refresh."
            return
        }
        $installedVersion = $installedInventory.Plugin.version
        if ($installedVersion -ne $runtimeVersion) {
            Write-Warning "Codex ProjectAtlas plugin update failed: installed projectatlas plugin version '$installedVersion' does not match $runtimeVersion."
            return
        }
        $installedPlugin = $installedInventory.Plugin
        if (-not (Test-ProjectAtlasCodexPluginSourceManifest $installedPlugin $runtimeVersion)) {
            $sourceManifestVersion = Get-ProjectAtlasCodexPluginSourceManifestVersion $installedPlugin
            Write-Warning "Codex ProjectAtlas plugin update failed: source manifest version '$sourceManifestVersion' does not match $runtimeVersion after refresh."
            return
        }
        $updateSucceeded = $true
        Write-Output "Codex ProjectAtlas plugin marketplace updated to $releaseTag."
        Confirm-ProjectAtlasCodexSkillArtifact $codexCommandPath $ExpectedVersion
        }
        finally {
            if (-not $updateSucceeded) {
                $script:ProjectAtlasCodexPluginUpdatePreservedPriorState = $true
                $restoreSucceeded = Restore-ProjectAtlasCodexStateSnapshot $stateSnapshot
            }
            if ($updateSucceeded -or $restoreSucceeded) {
                Remove-ProjectAtlasCodexStateSnapshot $stateSnapshot | Out-Null
            }
        }
    }
    catch {
        $script:ProjectAtlasCodexPluginUpdatePreservedPriorState = $true
        Write-Warning "Codex ProjectAtlas plugin update failed: $($_.Exception.Message)"
    }
    finally {
        Exit-ProjectAtlasCodexPluginUpdateLock $updateLock
    }
}

function Update-ProjectAtlasCodexMcpRegistry {
    param(
        [string]$VerifiedPath,
        [string]$ExpectedVersion,
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath
    )
    if (Test-Truthy $env:PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE) {
        Write-Output "Codex MCP registry update skipped by PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE."
        return
    }
    $codexCommandPath = Resolve-ProjectAtlasCodexCommand "Codex MCP registry update"
    if (-not $codexCommandPath) {
        return
    }
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    $launchArgs = Get-ProjectAtlasMcpLaunchArguments $DbPath $ProjectConfigPath $FlatConfigPath $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion) -or $launchArgs.Count -eq 0) {
        Write-Output "Codex MCP registry update skipped: ProjectAtlas version is unknown."
        return
    }
    try {
        $existing = Get-ProjectAtlasCodexMcpRegistryEntry $codexCommandPath
        if (-not $existing) {
            Write-Output "Codex MCP registry update skipped: no global projectatlas MCP server is configured."
            return
        }
        if (Test-ProjectAtlasCodexMcpRegistryEntry $existing $VerifiedPath $launchArgs) {
            Write-Output "Codex MCP registry already points to ProjectAtlas $runtimeVersion for $DbPath."
            return
        }

        & $codexCommandPath mcp remove projectatlas | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex MCP registry update failed: could not remove stale global projectatlas server."
            return
        }
        $addArgs = @("mcp", "add", "projectatlas", "--", $VerifiedPath) + $launchArgs
        & $codexCommandPath @addArgs | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Codex MCP registry update failed: could not add verified global projectatlas server."
            return
        }
        $updated = Get-ProjectAtlasCodexMcpRegistryEntry $codexCommandPath
        if (-not (Test-ProjectAtlasCodexMcpRegistryEntry $updated $VerifiedPath $launchArgs)) {
            Write-Warning "Codex MCP registry update failed: the replacement entry did not exactly match the verified command and ordered arguments."
            return
        }
        Write-Output "Codex MCP registry updated to ProjectAtlas runtime $VerifiedPath with database $DbPath."
    }
    catch {
        Write-Warning "Codex MCP registry update failed: $($_.Exception.Message)"
    }
}

function Test-ProjectAtlasCodexCommandAvailable {
    if (-not [string]::IsNullOrWhiteSpace($env:PROJECTATLAS_CODEX_COMMAND)) {
        return [bool]((Resolve-Path $env:PROJECTATLAS_CODEX_COMMAND -ErrorAction SilentlyContinue) `
                -or (Get-Command $env:PROJECTATLAS_CODEX_COMMAND -ErrorAction SilentlyContinue))
    }
    return [bool](Get-Command codex -ErrorAction SilentlyContinue)
}

function Get-ProjectAtlasCodexMcpRegistryEntry {
    param(
        [string]$CodexCommandPath
    )
    try {
        return Invoke-ProjectAtlasBoundedJsonCommand `
            $CodexCommandPath `
            ([string[]]@("mcp", "get", "projectatlas", "--json"))
    }
    catch {
        return $null
    }
}

function Test-ProjectAtlasCodexMcpRegistryEntry {
    param(
        [object]$Registration,
        [string]$VerifiedPath,
        [string[]]$ExpectedArguments
    )
    if (-not (Test-ProjectAtlasJsonObject $Registration) `
        -or -not ($Registration.name -is [string]) `
        -or -not [string]::Equals($Registration.name, "projectatlas", [System.StringComparison]::Ordinal) `
        -or -not ($Registration.enabled -is [bool]) `
        -or -not $Registration.enabled `
        -or -not (Test-ProjectAtlasJsonObject $Registration.transport) `
        -or -not ($Registration.transport.type -is [string]) `
        -or -not [string]::Equals($Registration.transport.type, "stdio", [System.StringComparison]::Ordinal) `
        -or -not ($Registration.transport.command -is [string]) `
        -or -not (Test-ProjectAtlasJsonStringArray $Registration.transport.args)) {
        return $false
    }
    $actualCommand = $Registration.transport.command
    if (-not [System.IO.Path]::IsPathRooted($actualCommand) `
        -or -not [System.IO.Path]::IsPathRooted($VerifiedPath)) {
        return $false
    }
    $actualArguments = [string[]](@($actualCommand) + @($Registration.transport.args))
    $expected = [string[]](@($VerifiedPath) + @($ExpectedArguments))
    return Test-ProjectAtlasExactArguments $actualArguments $expected
}

function Test-ProjectAtlasCodexPluginReady {
    param(
        [string]$ExpectedVersion
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    $codexCommandPath = Resolve-ProjectAtlasCodexCommand "Codex ProjectAtlas plugin verification"
    if ([string]::IsNullOrWhiteSpace($runtimeVersion) -or -not $codexCommandPath) {
        return $false
    }
    $plugin = Get-ProjectAtlasCodexPlugin $codexCommandPath
    if (-not $plugin `
        -or -not ($plugin.installed -is [bool]) `
        -or -not $plugin.installed `
        -or -not ($plugin.enabled -is [bool]) `
        -or -not $plugin.enabled `
        -or -not ($plugin.version -is [string]) `
        -or -not [string]::Equals($plugin.version, $runtimeVersion, [System.StringComparison]::Ordinal) `
        -or -not ($plugin.marketplaceSource.source -is [string]) `
        -or -not (Test-ProjectAtlasOfficialMarketplaceSource $plugin.marketplaceSource.source)) {
        return $false
    }
    try {
        if (-not (Test-ProjectAtlasCodexPluginSourceManifest $plugin $runtimeVersion)) {
            return $false
        }
        $pluginSourcePath = Get-ProjectAtlasCodexPluginSourcePath $plugin
        if ([string]::IsNullOrWhiteSpace($pluginSourcePath) `
            -or -not [System.IO.Path]::IsPathRooted($pluginSourcePath)) {
            return $false
        }
        $manifestPath = Join-Path $pluginSourcePath ".codex-plugin\plugin.json"
        $skillPath = Join-Path $pluginSourcePath "skills\projectatlas\SKILL.md"
        $installerPluginRoot = Split-Path -Parent $PSScriptRoot
        $installerSkillPath = Join-Path $installerPluginRoot "skills\projectatlas\SKILL.md"
        if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) `
            -or -not (Test-Path -LiteralPath $skillPath -PathType Leaf) `
            -or -not (Test-Path -LiteralPath $installerSkillPath -PathType Leaf)) {
            return $false
        }
        return (Get-ProjectAtlasSha256 $skillPath) -eq (Get-ProjectAtlasSha256 $installerSkillPath)
    }
    catch {
        return $false
    }
}

function Test-ProjectAtlasCodexMcpRegistryReady {
    param(
        [string]$VerifiedPath,
        [string]$ExpectedVersion,
        [string]$DbPath,
        [string]$ProjectConfigPath,
        [string]$FlatConfigPath
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    $codexCommandPath = Resolve-ProjectAtlasCodexCommand "Codex MCP registry verification"
    if ([string]::IsNullOrWhiteSpace($runtimeVersion) -or -not $codexCommandPath) {
        return $false
    }
    $launchArgs = Get-ProjectAtlasMcpLaunchArguments `
        $DbPath `
        $ProjectConfigPath `
        $FlatConfigPath `
        $ExpectedVersion
    $registered = Get-ProjectAtlasCodexMcpRegistryEntry $codexCommandPath
    return Test-ProjectAtlasCodexMcpRegistryEntry $registered $VerifiedPath $launchArgs
}

function Write-ProjectAtlasWorkflowPinReport {
    param(
        [string]$Root,
        [string]$ExpectedVersion
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $ExpectedVersion
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        return
    }
    $workflowDir = Join-Path $Root ".github\workflows"
    if (-not (Test-Path -LiteralPath $workflowDir)) {
        return
    }
    $releaseTag = "v$runtimeVersion"
    $rootPath = (Resolve-Path -LiteralPath $Root).Path.TrimEnd('\', '/')
    $workflowFiles = Get-ChildItem -LiteralPath $workflowDir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Extension -eq ".yml" -or $_.Extension -eq ".yaml" }
    foreach ($file in $workflowFiles) {
        $lineNumber = 0
        foreach ($line in Get-Content -LiteralPath $file.FullName) {
            $lineNumber += 1
            if ($line -notmatch 'github\.com/styler-ai/ProjectAtlas/releases/download/') {
                continue
            }
            $pinMatches = [System.Text.RegularExpressions.Regex]::Matches($line, 'v[0-9]+\.[0-9]+\.[0-9]+')
            foreach ($match in $pinMatches) {
                $foundTag = $match.Value
                if ($foundTag -and $foundTag -ne $releaseTag) {
                    $relativePath = $file.FullName
                    if ($relativePath.StartsWith($rootPath, [System.StringComparison]::OrdinalIgnoreCase)) {
                        $relativePath = $relativePath.Substring($rootPath.Length).TrimStart('\', '/')
                    }
                    Write-Warning "Stale ProjectAtlas workflow release pin in ${relativePath}:${lineNumber} uses $foundTag; expected $releaseTag."
                }
            }
        }
    }
}

function Get-ReleaseRuntimeInstallPath {
    param(
        [string]$Version
    )
    $runtimeVersion = Convert-ProjectAtlasVersionTag $Version
    if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
        $runtimeVersion = "unknown"
    }
    $safeVersion = $runtimeVersion -replace '[^A-Za-z0-9_.-]', '_'
    $installDir = Join-Path $env:LOCALAPPDATA "ProjectAtlas\runtimes\$safeVersion\x86_64-pc-windows-msvc"
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    return Join-Path $installDir "projectatlas.exe"
}

function Get-ProjectAtlasSha256 {
    param(
        [string]$Archive
    )
    $stream = [System.IO.File]::OpenRead($Archive)
    try {
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            $hash = $sha256.ComputeHash($stream)
            return ([System.BitConverter]::ToString($hash) -replace '-', '').ToLowerInvariant()
        }
        finally {
            $sha256.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

function Confirm-ReleaseArchiveChecksum {
    param(
        [string]$Archive,
        [string]$Asset,
        [string]$Version,
        [string]$BaseUrl,
        [string]$TempDir
    )
    $checksums = Join-Path $TempDir "SHA256SUMS"
    Invoke-WebRequest -Uri "$BaseUrl/$Version/SHA256SUMS" -OutFile $checksums
    $expected = $null
    foreach ($line in Get-Content -LiteralPath $checksums) {
        $parts = $line.Trim() -split '\s+'
        if ($parts.Count -ge 2 -and ($parts[1] -eq $Asset -or $parts[1] -eq "./$Asset")) {
            $expected = $parts[0].ToLowerInvariant()
            break
        }
    }
    if ([string]::IsNullOrWhiteSpace($expected)) {
        throw "SHA256SUMS did not contain an entry for $Asset"
    }
    $actual = Get-ProjectAtlasSha256 $Archive
    if ($actual -ne $expected) {
        throw "Checksum mismatch for ${Asset}: expected $expected, found $actual"
    }
}

function Install-ReleaseBinary {
    param(
        [string]$Version,
        [string]$BaseUrl
    )
    if (-not $Version) {
        return $null
    }
    $asset = "projectatlas-$Version-x86_64-pc-windows-msvc.zip"
    $url = "$BaseUrl/$Version/$asset"
    $target = Get-ReleaseRuntimeInstallPath $Version
    if (Test-ProjectAtlasRuntime $target $Version) {
        return $target
    }
    $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("projectatlas-" + [guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
    $archive = Join-Path $tempDir $asset
    try {
        Invoke-WebRequest -Uri $url -OutFile $archive
        Confirm-ReleaseArchiveChecksum $archive $asset $Version $BaseUrl $tempDir
        Expand-Archive -LiteralPath $archive -DestinationPath $tempDir -Force
        $binary = Get-ChildItem -LiteralPath $tempDir -Filter "projectatlas.exe" -Recurse | Select-Object -First 1
        if (-not $binary) {
            throw "Release archive did not contain projectatlas.exe"
        }
        Copy-Item -LiteralPath $binary.FullName -Destination $target -Force
        if (-not (Test-ProjectAtlasRuntime $target $Version)) {
            throw "Release archive produced an invalid runtime for ProjectAtlas ${Version}: $target"
        }
        return $target
    }
    catch {
        Write-Warning "Release binary install failed from ${url}: $($_.Exception.Message)"
        return $null
    }
    finally {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if (-not $ProjectRoot) {
    $ProjectRoot = Resolve-DefaultProjectRoot
}

if (-not $ProjectAtlasVersion) {
    if ($env:PROJECTATLAS_VERSION) {
        $ProjectAtlasVersion = $env:PROJECTATLAS_VERSION
    }
    else {
        $ProjectAtlasVersion = Resolve-PluginReleaseVersion
    }
}

if (-not $RuntimePath -and $env:PROJECTATLAS_RUNTIME_PATH) {
    $RuntimePath = $env:PROJECTATLAS_RUNTIME_PATH
}

$releaseBinaryOnly = $ReleaseBinaryOnly -or (Test-Truthy $env:PROJECTATLAS_RELEASE_BINARY_ONLY)
$ProjectRoot = (Resolve-Path $ProjectRoot).Path
$atlasDir = Join-Path $ProjectRoot ".projectatlas"
Assert-ProjectAtlasDirectPath $atlasDir "ProjectAtlas project state directory"
$inheritedProcessPath = $env:Path
$inheritedProjectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue | Select-Object -First 1
$inheritedProjectAtlasPath = if ($inheritedProjectAtlasCommand) { $inheritedProjectAtlasCommand.Source } else { $null }
$futureProcessPathReady = $false

if ($RuntimePath) {
    $projectAtlas = (Resolve-Path $RuntimePath).Path
    if (-not (Test-ProjectAtlasRuntime $projectAtlas $ProjectAtlasVersion)) {
        throw "Provided ProjectAtlas runtime does not satisfy the ProjectAtlas runtime/version contract: $projectAtlas"
    }
    $stableMirrorSynchronized = Sync-ProjectAtlasRuntimeToLocalAppData $projectAtlas $ProjectAtlasVersion
    Set-ProjectAtlasProcessPathPrecedence $projectAtlas
}
else {
    $cargo = Find-Cargo
    $installedBinary = $null

    if ($releaseBinaryOnly) {
        $installedBinary = Install-ReleaseBinary $ProjectAtlasVersion $ReleaseBaseUrl
        if (-not $installedBinary) {
            throw "ProjectAtlas release-binary install was required but failed for $ProjectAtlasVersion."
        }
        if (-not (Test-ProjectAtlasRuntime $installedBinary $ProjectAtlasVersion)) {
            throw "ProjectAtlas release-binary install produced an invalid runtime for ${ProjectAtlasVersion}: $installedBinary"
        }
    }
    else {
        $releaseBinary = Install-ReleaseBinary $ProjectAtlasVersion $ReleaseBaseUrl
        if ($releaseBinary) {
            $installedBinary = $releaseBinary
        }
        if (-not $releaseBinary -and $cargo) {
            $installArgs = @("install", "--git", $Repository)
            if ($ProjectAtlasVersion) {
                $installArgs += @("--tag", $ProjectAtlasVersion)
            }
            $installArgs += @("projectatlas-cli", "--locked", "--force")
            Invoke-Checked $cargo $installArgs
        }
    }

    $projectAtlas = if ($installedBinary -and (Test-ProjectAtlasRuntime $installedBinary $ProjectAtlasVersion)) { $installedBinary } else { Find-ProjectAtlas $ProjectAtlasVersion }
    if (-not $projectAtlas) {
        throw "A ProjectAtlas runtime matching $ProjectAtlasVersion was not found. Install Rust/Cargo or provide the matching ProjectAtlas release binary on PATH."
    }
    $stableMirrorSynchronized = Sync-ProjectAtlasRuntimeToLocalAppData $projectAtlas $ProjectAtlasVersion

    Set-ProjectAtlasProcessPathPrecedence $projectAtlas
}
Invoke-Checked $projectAtlas @("--format", "json", "runtime-info") | Out-Null
Confirm-ProjectAtlasBareCommandResolution $projectAtlas $ProjectAtlasVersion
$verifiedRuntimePath = Get-NormalizedPathEntry $projectAtlas
$stableMirrorPath = Get-NormalizedPathEntry (Join-Path $env:LOCALAPPDATA "ProjectAtlas\bin\projectatlas.exe")
Quarantine-ProjectAtlasStaleShims $projectAtlas $ProjectAtlasVersion
if (-not $RuntimePath) {
    $futureProcessPathReady = Set-ProjectAtlasPathPrecedence $projectAtlas
}
else {
    $futureProcessPathReady = Test-ProjectAtlasPersistedBareCommandResolution $projectAtlas
}
$effectiveInheritedProjectAtlasPath = $inheritedProjectAtlasPath
if ([string]::IsNullOrWhiteSpace($effectiveInheritedProjectAtlasPath) -or -not (Test-Path -LiteralPath $effectiveInheritedProjectAtlasPath)) {
    $installerProcessPath = $env:Path
    try {
        $env:Path = $inheritedProcessPath
        $effectiveInheritedProjectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue | Select-Object -First 1
        $effectiveInheritedProjectAtlasPath = if ($effectiveInheritedProjectAtlasCommand) { $effectiveInheritedProjectAtlasCommand.Source } else { $null }
    }
    finally {
        $env:Path = $installerProcessPath
    }
}
Assert-ProjectAtlasDirectPath $atlasDir "ProjectAtlas project state directory"
New-Item -ItemType Directory -Force -Path $atlasDir | Out-Null
Assert-ProjectAtlasDirectPath $atlasDir "ProjectAtlas project state directory"
$dbPath = Join-Path $atlasDir "projectatlas.db"
$projectConfigPath = Join-Path $atlasDir "config.toml"
$flatConfigPath = Join-Path $ProjectRoot "projectatlas.toml"
$mcpConfigPath = Join-Path $atlasDir "projectatlas.mcp.json"
$claudeMcpConfigPath = Join-Path $atlasDir "projectatlas.claude.mcp.json"
$opencodeConfigPath = Join-Path $atlasDir "projectatlas.opencode.json"

function Write-ProjectAtlasMcpConfig {
    param(
        [string]$OutputPath,
        [string]$Harness
    )
    $mcpArgs = @("--format", "json", "--db", $dbPath)
    if (Test-Path -LiteralPath $projectConfigPath) {
        $mcpArgs += @("--config", $projectConfigPath)
    }
    elseif (Test-Path -LiteralPath $flatConfigPath) {
        $mcpArgs += @("--config", $flatConfigPath)
    }
    $mcpArgs += @("mcp-config")
    if ($Harness) {
        $mcpArgs += @("--harness", $Harness)
    }
    $mcpConfig = & $projectAtlas @mcpArgs
    if ($LASTEXITCODE -ne 0) {
        throw "ProjectAtlas MCP config generation failed with exit code $LASTEXITCODE for harness '$Harness'."
    }
    $utf8NoBom = New-Object System.Text.UTF8Encoding -ArgumentList $false
    $mcpConfigText = ($mcpConfig -join [Environment]::NewLine) + [Environment]::NewLine
    Assert-ProjectAtlasDirectFilePath $OutputPath "ProjectAtlas MCP config output"
    $temporaryOutputPath = Join-Path $atlasDir (".projectatlas-mcp-config-" + [guid]::NewGuid().ToString("N") + ".tmp")
    try {
        [System.IO.File]::WriteAllText($temporaryOutputPath, $mcpConfigText, $utf8NoBom)
        Assert-ProjectAtlasDirectFilePath $OutputPath "ProjectAtlas MCP config output"
        Move-Item -LiteralPath $temporaryOutputPath -Destination $OutputPath -Force
    }
    finally {
        if ([System.IO.File]::Exists($temporaryOutputPath)) {
            [System.IO.File]::Delete($temporaryOutputPath)
        }
    }
}

Write-ProjectAtlasMcpConfig $mcpConfigPath $null
Write-ProjectAtlasMcpConfig $claudeMcpConfigPath "claude-code"
Write-ProjectAtlasMcpConfig $opencodeConfigPath "opencode"
$mcpConfigSha256 = Confirm-ProjectAtlasGeneratedMcpConfig $mcpConfigPath "Codex" $projectAtlas $ProjectAtlasVersion $dbPath $projectConfigPath $flatConfigPath $ProjectRoot
$claudeMcpConfigSha256 = Confirm-ProjectAtlasGeneratedMcpConfig $claudeMcpConfigPath "Claude Code" $projectAtlas $ProjectAtlasVersion $dbPath $projectConfigPath $flatConfigPath $ProjectRoot
$opencodeConfigSha256 = Confirm-ProjectAtlasGeneratedMcpConfig $opencodeConfigPath "OpenCode" $projectAtlas $ProjectAtlasVersion $dbPath $projectConfigPath $flatConfigPath $ProjectRoot
Update-ProjectAtlasCodexPlugin $ProjectAtlasVersion
if ($script:ProjectAtlasCodexPluginUpdatePreservedPriorState) {
    Write-Output "Codex MCP registry update skipped because the prior ProjectAtlas plugin integration was preserved after a failed update."
}
else {
    Update-ProjectAtlasCodexMcpRegistry $projectAtlas $ProjectAtlasVersion $dbPath $projectConfigPath $flatConfigPath
}
$codexIntegrationManaged = Test-ProjectAtlasCodexCommandAvailable
$handoffState = "not_required"
if (-not $stableMirrorSynchronized) {
    $handoffState = Invoke-ProjectAtlasObsoleteStableMcpHandoff `
        $stableMirrorPath `
        $projectAtlas `
        $ProjectAtlasVersion `
        $dbPath `
        $projectConfigPath `
        $flatConfigPath `
        $mcpConfigPath `
        $claudeMcpConfigPath `
        $opencodeConfigPath `
        $mcpConfigSha256 `
        $claudeMcpConfigSha256 `
        $opencodeConfigSha256
    if ($handoffState -eq "retired" -or $handoffState -eq "exited") {
        $stableMirrorSynchronized = Sync-ProjectAtlasRuntimeToLocalAppData $projectAtlas $ProjectAtlasVersion
        if ($stableMirrorSynchronized) {
            $handoffState = if ($handoffState -eq "retired") { "completed" } else { "completed_after_exit" }
        }
        else {
            $handoffState = if ($handoffState -eq "retired") { "retry_failed" } else { "exit_retry_failed" }
        }
    }
}
$inheritedCommandMatchesRuntime = -not [string]::IsNullOrWhiteSpace($effectiveInheritedProjectAtlasPath) `
    -and (Get-NormalizedPathEntry $effectiveInheritedProjectAtlasPath) -eq $verifiedRuntimePath
$inheritedCommandMatchesMirror = -not [string]::IsNullOrWhiteSpace($effectiveInheritedProjectAtlasPath) `
    -and (Get-NormalizedPathEntry $effectiveInheritedProjectAtlasPath) -eq $stableMirrorPath
$installerProjectAtlasCommand = Get-Command projectatlas -ErrorAction SilentlyContinue | Select-Object -First 1
$installerProjectAtlasPath = if ($installerProjectAtlasCommand) { $installerProjectAtlasCommand.Source } else { $null }
$installerCommandMatchesRuntime = -not [string]::IsNullOrWhiteSpace($installerProjectAtlasPath) `
    -and (Get-NormalizedPathEntry $installerProjectAtlasPath) -eq $verifiedRuntimePath
$codexPluginReady = Test-ProjectAtlasCodexPluginReady $ProjectAtlasVersion
$codexRegistryReady = Test-ProjectAtlasCodexMcpRegistryReady $projectAtlas $ProjectAtlasVersion $dbPath $projectConfigPath $flatConfigPath
$generatedMcpConfigsReady = Test-ProjectAtlasGeneratedMcpConfigReadiness `
    ([string[]]@($mcpConfigPath, $claudeMcpConfigPath, $opencodeConfigPath)) `
    ([string[]]@($mcpConfigSha256, $claudeMcpConfigSha256, $opencodeConfigSha256))
$verifiedRuntimeReady = Test-ProjectAtlasRuntime $projectAtlas $ProjectAtlasVersion
$stableMirrorReady = $stableMirrorSynchronized `
    -and (Test-ProjectAtlasRuntime $stableMirrorPath $ProjectAtlasVersion)
$inheritedCommandReady = $verifiedRuntimeReady -and $inheritedCommandMatchesRuntime
$inheritedSynchronizedMirrorReady = $stableMirrorReady -and $inheritedCommandMatchesMirror
$installerCliReady = $verifiedRuntimeReady -and $installerCommandMatchesRuntime
$parentCliReady = $inheritedCommandReady -or $inheritedSynchronizedMirrorReady
$hostRestartRequired = $verifiedRuntimeReady -and -not $parentCliReady -and $futureProcessPathReady
$hostRepairRequired = $verifiedRuntimeReady -and -not $parentCliReady -and -not $futureProcessPathReady
$runtimeMcpConfigsReady = $verifiedRuntimeReady -and $generatedMcpConfigsReady
$integrationVerificationRequired = $handoffState -ne "not_required" -or $codexIntegrationManaged
$updateState = if ($runtimeMcpConfigsReady `
        -and $stableMirrorReady `
        -and (-not $integrationVerificationRequired -or ($codexPluginReady -and $codexRegistryReady))) {
    "complete"
}
else {
    "partial"
}
if ($verifiedRuntimeReady) {
    Write-ProjectAtlasPathShadowReport $projectAtlas $ProjectAtlasVersion
}
else {
    Write-Warning "ProjectAtlas PATH shadow report skipped because the requested absolute runtime failed final verification."
}
Write-ProjectAtlasWorkflowPinReport $ProjectRoot $ProjectAtlasVersion

if ($verifiedRuntimeReady) {
    Write-Output "ProjectAtlas runtime installed and verified: $projectAtlas"
}
else {
    Write-Warning "ProjectAtlas runtime failed final verification: path=$verifiedRuntimePath target_version=$(Convert-ProjectAtlasVersionTag $ProjectAtlasVersion). Rerun this installer with a valid matching runtime."
}
Write-Output "ProjectAtlas update preserved project state under $atlasDir; use reset-index --apply for explicit state cleanup."
Write-Output "Project-local MCP config written: $mcpConfigPath"
Write-Output "Project-local Claude Code MCP config written: $claudeMcpConfigPath"
Write-Output "Project-local OpenCode MCP config written: $opencodeConfigPath"
$runtimeMcpConfigGuidance = if ($runtimeMcpConfigsReady) {
    Write-Output "Claude Code ProjectAtlas integration verified through generated MCP config; restart Claude Code if an older session cached previous instructions."
    Write-Output "OpenCode ProjectAtlas integration verified through generated MCP config; restart OpenCode if an older session cached previous instructions."
    "The runtime and generated MCP configs are ready through the verified absolute runtime."
}
else {
    if (-not $verifiedRuntimeReady) {
        Write-Warning "ProjectAtlas runtime readiness changed before final reporting; generated MCP configs are not usable until the runtime is reverified."
    }
    else {
        Write-Warning "ProjectAtlas generated MCP config readiness changed before final reporting; rerun this installer."
    }
    "The installed runtime and generated MCP configs could not be reverified; rerun this installer."
}
$targetRuntimeVersion = Convert-ProjectAtlasVersionTag $ProjectAtlasVersion
if ([string]::IsNullOrWhiteSpace($targetRuntimeVersion)) {
    $targetRuntimeVersion = Convert-ProjectAtlasVersionTag (Get-ProjectAtlasRuntimeVersion $verifiedRuntimePath)
}
$staleBareCommandPath = if (-not $stableMirrorReady `
        -and -not [string]::IsNullOrWhiteSpace($effectiveInheritedProjectAtlasPath)) {
    Get-NormalizedPathEntry $effectiveInheritedProjectAtlasPath
}
else {
    $null
}
$staleBareCommandVersion = if ($staleBareCommandPath) {
    Get-ProjectAtlasRuntimeVersion $staleBareCommandPath
}
else {
    $null
}
$lockedStaleCommandRecoveryRequired = -not $stableMirrorReady `
    -and $staleBareCommandPath `
    -and -not [string]::IsNullOrWhiteSpace($targetRuntimeVersion) `
    -and -not $parentCliReady
if ($lockedStaleCommandRecoveryRequired) {
    $observedVersion = if ($staleBareCommandVersion) { $staleBareCommandVersion } else { "unavailable" }
    $quotedInstaller = "'" + (Get-NormalizedPathEntry $PSCommandPath).Replace("'", "''") + "'"
    $quotedProjectRoot = "'" + $ProjectRoot.Replace("'", "''") + "'"
    $quotedReleaseVersion = "'" + $targetRuntimeVersion.Replace("'", "''") + "'"
    $quotedRuntimeVersion = "'" + $targetRuntimeVersion.Replace("'", "''") + "'"
    if ($verifiedRuntimeReady) {
        $quotedRuntime = "'" + $verifiedRuntimePath.Replace("'", "''") + "'"
        $tokenArguments = @(Get-ProjectAtlasTokenLaunchArguments $dbPath $projectConfigPath $flatConfigPath $targetRuntimeVersion)
        $quotedTokenArguments = ($tokenArguments | ForEach-Object { "'" + ([string]$_).Replace("'", "''") + "'" }) -join " "
        Write-Output "ProjectAtlas stale bare command: path=$staleBareCommandPath observed_version=$observedVersion ready=false; verified_runtime=$verifiedRuntimePath target_version=$targetRuntimeVersion."
        Write-Output "ProjectAtlas verified absolute runtime command: & $quotedRuntime --require-version $quotedRuntimeVersion --format json runtime-info"
        Write-Output "ProjectAtlas verified absolute runtime operation: & $quotedRuntime $quotedTokenArguments"
        Write-Warning "ProjectAtlas locked-mirror recovery: restart_can_repair_command_resolution=$($hostRestartRequired.ToString().ToLowerInvariant()). Wait for the lock owner to exit or release $stableMirrorPath; then rerun & $quotedInstaller -ProjectRoot $quotedProjectRoot -ProjectAtlasVersion $quotedReleaseVersion -RuntimePath $quotedRuntime. From $ProjectRoot, require both bare-command gates before declaring convergence: projectatlas --require-version $quotedRuntimeVersion --format json runtime-info; projectatlas --require-version $quotedRuntimeVersion token --view tui."
    }
    else {
        Write-Output "ProjectAtlas stale bare command: path=$staleBareCommandPath observed_version=$observedVersion ready=false; target_version=$targetRuntimeVersion."
        Write-Warning "ProjectAtlas requested absolute runtime failed final verification: path=$verifiedRuntimePath target_version=$targetRuntimeVersion. Restore or replace that runtime and rerun & $quotedInstaller -ProjectRoot $quotedProjectRoot -ProjectAtlasVersion $quotedReleaseVersion; do not use it through -RuntimePath until verification succeeds."
    }
}
if (-not $verifiedRuntimeReady) {
    Write-Warning "Existing host recovery cannot use or advertise the requested absolute runtime until final runtime verification succeeds. $runtimeMcpConfigGuidance"
}
elseif ($hostRestartRequired) {
    Write-Warning "Existing host restart required: the inherited bare 'projectatlas' command remains stale, but the verified runtime is first on the persisted fresh-process PATH. Restart the environment-owning Windows launcher or terminal session, then start a new Codex or shell; restarting only a child of an unchanged launcher can retain stale PATH. $runtimeMcpConfigGuidance"
}
elseif ($hostRepairRequired) {
    if ($handoffState -in @("not_required", "completed", "completed_after_exit", "retry_failed", "exit_retry_failed")) {
        Write-Warning "Existing host bare CLI is not ready, and restart alone will not repair it because the verified runtime is not the first bare command for a fresh process. Configure $(Split-Path -Parent $projectAtlas) first on PATH, then rerun this installer if convergence remains partial. $runtimeMcpConfigGuidance"
    }
    else {
        Write-Warning "Existing host bare CLI is not ready, and restart alone will not repair it because this installation could not make the verified runtime the first bare command for a fresh process. The installer could not prove an exact retireable obsolete MCP owner; restart the owning host and rerun this installer, or configure $(Split-Path -Parent $projectAtlas) first on PATH. $runtimeMcpConfigGuidance"
    }
}
Write-Output "ProjectAtlas convergence: update_state=$updateState stable_mirror_ready=$($stableMirrorReady.ToString().ToLowerInvariant()) obsolete_mcp_handoff=$handoffState codex_plugin_ready=$($codexPluginReady.ToString().ToLowerInvariant()) codex_registry_ready=$($codexRegistryReady.ToString().ToLowerInvariant())"
Write-Output "ProjectAtlas readiness: runtime_ready=$($verifiedRuntimeReady.ToString().ToLowerInvariant()) generated_mcp_configs_ready=$($generatedMcpConfigsReady.ToString().ToLowerInvariant()) runtime_mcp_configs_ready=$($runtimeMcpConfigsReady.ToString().ToLowerInvariant()) installer_cli_ready=$($installerCliReady.ToString().ToLowerInvariant()) parent_cli_ready=$($parentCliReady.ToString().ToLowerInvariant()) host_restart_required=$($hostRestartRequired.ToString().ToLowerInvariant())"
