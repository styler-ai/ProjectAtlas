[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string] $OutputPath,

    [switch] $RunSelfTest
)

# Builds the Windows runtime containment broker used beside the immutable optional parser pack.
# The artifact-manifest digest deterministically names its AppContainer profile. Production
# profiles and their pack read/execute ACEs intentionally persist so concurrent workers cannot
# race profile deletion; the pack lifecycle must remove stale artifact-scoped profiles. The
# self-test uses a unique manifest and proves its temporary profile can be removed.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$brokerSource = @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;

namespace ProjectAtlas.Release
{
    internal static class ParserPackRuntimeContainment
    {
        private const int FailureExitCode = 125;
        private const int MemoryLimitExitCode = 124;
        private const int MaximumArtifactManifestBytes = 1024 * 1024;
        private const ulong MinimumMemoryBytes = 16UL * 1024UL * 1024UL;
        private const ulong MaximumMemoryBytes = 64UL * 1024UL * 1024UL * 1024UL;
        private const uint CleanupWaitMilliseconds = 5000;
        private const uint ErrorInsufficientBuffer = 122;
        private const uint ErrorAlreadyExists = 183;
        private const uint ErrorFileNotFound = 2;
        private const uint ErrorNotFound = 1168;
        private const uint TokenQuery = 0x0008;
        private const uint ProcessSynchronize = 0x00100000;
        private const int StandardInputHandle = -10;
        private const int StandardOutputHandle = -11;
        private const int StandardErrorHandle = -12;
        private const uint FileTypePipe = 0x0003;
        private const uint HandleFlagInherit = 0x00000001;
        private const uint StartfUseStdHandles = 0x00000100;
        private const uint CreateSuspended = 0x00000004;
        private const uint CreateNoWindow = 0x08000000;
        private const uint CreateUnicodeEnvironment = 0x00000400;
        private const uint ExtendedStartupInfoPresent = 0x00080000;
        private const uint WaitObject0 = 0x00000000;
        private const uint WaitTimeout = 0x00000102;
        private const uint WaitFailed = 0xffffffff;
        private const uint JobObjectExtendedLimitInformation = 9;
        private const uint JobObjectAssociateCompletionPortInformation = 7;
        private const uint JobObjectLimitActiveProcess = 0x00000008;
        private const uint JobObjectLimitProcessMemory = 0x00000100;
        private const uint JobObjectLimitJobMemory = 0x00000200;
        private const uint JobObjectLimitKillOnJobClose = 0x00002000;
        private const uint JobObjectMessageActiveProcessZero = 4;
        private const uint JobObjectMessageProcessMemoryLimit = 9;
        private const uint JobObjectMessageJobMemoryLimit = 10;
        private const uint JobCompletionKey = 0x5041544c;
        private const uint ProcessCreationChildProcessRestricted = 0x00000001;
        private const uint ProcessCreationAllApplicationPackagesOptOut = 0x00000001;
        private const uint ProcessChildProcessPolicy = 13;
        private const uint ProcessMitigationNoChildProcessCreation = 0x00000001;
        private const uint TokenGroups = 2;
        private const uint TokenIsAppContainer = 29;
        private const uint TokenCapabilities = 30;
        private const uint TokenAppContainerSid = 31;
        private const uint MaximumTokenInformationBytes = 64 * 1024;
        private const string AllApplicationPackagesSid = "S-1-15-2-1";
        private const string AllRestrictedApplicationPackagesSid = "S-1-15-2-2";
        private const string ArtifactManifestFileName = "artifact-manifest.json";
        private const string WorkerFileName = "projectatlas-parser-worker.exe";
        private const string WorkerServeArgument = "--serve";
        private const string SelfTestMarkerFileName = "containment-self-test.marker";
        private const string SelfTestMarker = "projectatlas-containment-self-test-v1";
        private const string SelfTestPreAssignmentFaultMarker =
            "projectatlas-containment-self-test-pre-assignment-fault-v1";
        private static readonly IntPtr InvalidHandleValue = new IntPtr(-1);
        private static readonly IntPtr ProcThreadAttributeHandleList = new IntPtr(0x00020002);
        private static readonly IntPtr ProcThreadAttributeSecurityCapabilities = new IntPtr(0x00020009);
        private static readonly IntPtr ProcThreadAttributeChildProcessPolicy = new IntPtr(0x0002000e);
        private static readonly IntPtr ProcThreadAttributeAllApplicationPackagesPolicy = new IntPtr(0x0002000f);
        private static readonly byte[] AdmissionRecord = new byte[]
        {
            0x50, 0x41, 0x54, 0x4c, 0x41, 0x53, 0x2d, 0x41,
            0x44, 0x4d, 0x49, 0x54, 0x00, 0x00, 0x00, 0x01
        };
        private static readonly byte[] SelfTestAdmissionRecord = Encoding.ASCII.GetBytes(
            "projectatlas-containment-admission-only-v1\n");
        private static readonly object StandardErrorLock = new object();

        [StructLayout(LayoutKind.Sequential)]
        private struct SecurityAttributes
        {
            public int Length;
            public IntPtr SecurityDescriptor;
            [MarshalAs(UnmanagedType.Bool)]
            public bool InheritHandle;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct SecurityCapabilities
        {
            public IntPtr AppContainerSid;
            public IntPtr Capabilities;
            public uint CapabilityCount;
            public uint Reserved;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct StartupInfo
        {
            public uint cb;
            public string lpReserved;
            public string lpDesktop;
            public string lpTitle;
            public uint dwX;
            public uint dwY;
            public uint dwXSize;
            public uint dwYSize;
            public uint dwXCountChars;
            public uint dwYCountChars;
            public uint dwFillAttribute;
            public uint dwFlags;
            public ushort wShowWindow;
            public ushort cbReserved2;
            public IntPtr lpReserved2;
            public IntPtr hStdInput;
            public IntPtr hStdOutput;
            public IntPtr hStdError;
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct StartupInfoEx
        {
            public StartupInfo StartupInfo;
            public IntPtr AttributeList;
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
        private struct JobObjectBasicLimitInformation
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
        private struct JobObjectExtendedLimitInformationValue
        {
            public JobObjectBasicLimitInformation BasicLimitInformation;
            public IoCounters IoInfo;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct JobObjectAssociateCompletionPortValue
        {
            public IntPtr CompletionKey;
            public IntPtr CompletionPort;
        }

        private sealed class ContainmentFailure : Exception
        {
            internal ContainmentFailure(string stage)
                : base(stage)
            {
                Stage = stage;
            }

            internal ContainmentFailure(string stage, string code)
                : base(stage)
            {
                Stage = stage;
                Code = code;
            }

            internal string Stage { get; private set; }
            internal string Code { get; private set; }
        }

        private sealed class ServeConfiguration
        {
            internal uint ParentProcessId { get; set; }
            internal ulong ProcessMemoryBytes { get; set; }
            internal ulong JobMemoryBytes { get; set; }
        }

        private sealed class ProfileIdentity
        {
            internal ProfileIdentity(string name, SecurityIdentifier sid)
            {
                Name = name;
                Sid = sid;
            }

            internal string Name { get; private set; }
            internal SecurityIdentifier Sid { get; private set; }
        }

        private sealed class AttributeResources : IDisposable
        {
            private readonly List<IntPtr> allocations = new List<IntPtr>();
            private bool initialized;

            internal IntPtr List { get; private set; }

            internal void Build(SecurityIdentifier sid, IntPtr[] handles)
            {
                IntPtr size = IntPtr.Zero;
                if (InitializeProcThreadAttributeList(IntPtr.Zero, 4, 0, ref size)
                    || Marshal.GetLastWin32Error() != ErrorInsufficientBuffer)
                {
                    throw Win32Failure("measure-attribute-list");
                }
                List = Marshal.AllocHGlobal(size);
                allocations.Add(List);
                if (!InitializeProcThreadAttributeList(List, 4, 0, ref size))
                {
                    throw Win32Failure("initialize-attribute-list");
                }
                initialized = true;

                IntPtr sidPointer = AllocateSid(sid);
                SecurityCapabilities capabilities = new SecurityCapabilities();
                capabilities.AppContainerSid = sidPointer;
                IntPtr capabilitiesPointer = AllocateStructure(capabilities);
                UpdateAttribute(
                    ProcThreadAttributeSecurityCapabilities,
                    capabilitiesPointer,
                    new UIntPtr((uint)Marshal.SizeOf(typeof(SecurityCapabilities))),
                    "set-security-capabilities");

                IntPtr allPackagesPolicy = AllocateUInt32(ProcessCreationAllApplicationPackagesOptOut);
                UpdateAttribute(
                    ProcThreadAttributeAllApplicationPackagesPolicy,
                    allPackagesPolicy,
                    new UIntPtr(sizeof(uint)),
                    "set-lpac-policy");

                IntPtr handleList = Marshal.AllocHGlobal(checked(handles.Length * IntPtr.Size));
                allocations.Add(handleList);
                for (int index = 0; index < handles.Length; index += 1)
                {
                    Marshal.WriteIntPtr(handleList, index * IntPtr.Size, handles[index]);
                }
                UpdateAttribute(
                    ProcThreadAttributeHandleList,
                    handleList,
                    new UIntPtr((uint)checked(handles.Length * IntPtr.Size)),
                    "set-handle-list");

                IntPtr childPolicy = AllocateUInt32(ProcessCreationChildProcessRestricted);
                UpdateAttribute(
                    ProcThreadAttributeChildProcessPolicy,
                    childPolicy,
                    new UIntPtr(sizeof(uint)),
                    "set-child-process-policy");

            }

            public void Dispose()
            {
                if (initialized)
                {
                    DeleteProcThreadAttributeList(List);
                }
                for (int index = allocations.Count - 1; index >= 0; index -= 1)
                {
                    Marshal.FreeHGlobal(allocations[index]);
                }
                allocations.Clear();
                List = IntPtr.Zero;
                initialized = false;
            }

            private IntPtr AllocateSid(SecurityIdentifier sid)
            {
                byte[] bytes = new byte[sid.BinaryLength];
                sid.GetBinaryForm(bytes, 0);
                IntPtr value = Marshal.AllocHGlobal(bytes.Length);
                allocations.Add(value);
                Marshal.Copy(bytes, 0, value, bytes.Length);
                return value;
            }

            private IntPtr AllocateUInt32(uint value)
            {
                IntPtr pointer = Marshal.AllocHGlobal(sizeof(uint));
                allocations.Add(pointer);
                Marshal.WriteInt32(pointer, unchecked((int)value));
                return pointer;
            }

            private IntPtr AllocateStructure<T>(T value) where T : struct
            {
                IntPtr pointer = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(T)));
                allocations.Add(pointer);
                Marshal.StructureToPtr(value, pointer, false);
                return pointer;
            }

            private void UpdateAttribute(IntPtr attribute, IntPtr value, UIntPtr size, string stage)
            {
                if (!UpdateProcThreadAttribute(
                    List,
                    0,
                    attribute,
                    value,
                    size,
                    IntPtr.Zero,
                    IntPtr.Zero))
                {
                    throw Win32Failure(stage);
                }
            }
        }

        [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
        private static extern int CreateAppContainerProfile(
            string appContainerName,
            string displayName,
            string description,
            IntPtr capabilities,
            uint capabilityCount,
            out IntPtr appContainerSid);

        [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
        private static extern int DeriveAppContainerSidFromAppContainerName(
            string appContainerName,
            out IntPtr appContainerSid);

        [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
        private static extern int DeleteAppContainerProfile(string appContainerName);

        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool OpenProcessToken(
            IntPtr process,
            uint desiredAccess,
            out IntPtr token);

        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetTokenInformation(
            IntPtr token,
            uint tokenInformationClass,
            IntPtr tokenInformation,
            uint tokenInformationLength,
            out uint returnLength);

        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool EqualSid(IntPtr firstSid, IntPtr secondSid);

        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern IntPtr FreeSid(IntPtr sid);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool InitializeProcThreadAttributeList(
            IntPtr attributeList,
            int attributeCount,
            uint flags,
            ref IntPtr size);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool UpdateProcThreadAttribute(
            IntPtr attributeList,
            uint flags,
            IntPtr attribute,
            IntPtr value,
            UIntPtr size,
            IntPtr previousValue,
            IntPtr returnSize);

        [DllImport("kernel32.dll")]
        private static extern void DeleteProcThreadAttributeList(IntPtr attributeList);

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

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr CreateJobObject(IntPtr jobAttributes, string name);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetInformationJobObject(
            IntPtr job,
            uint informationClass,
            IntPtr information,
            uint informationLength);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool QueryInformationJobObject(
            IntPtr job,
            uint informationClass,
            out JobObjectExtendedLimitInformationValue information,
            uint informationLength,
            out uint returnLength);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr CreateIoCompletionPort(
            IntPtr fileHandle,
            IntPtr existingCompletionPort,
            UIntPtr completionKey,
            uint concurrentThreads);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetQueuedCompletionStatus(
            IntPtr completionPort,
            out uint message,
            out UIntPtr completionKey,
            out IntPtr messageValue,
            uint milliseconds);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool IsProcessInJob(
            IntPtr process,
            IntPtr job,
            [MarshalAs(UnmanagedType.Bool)] out bool result);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetProcessMitigationPolicy(
            IntPtr process,
            uint mitigationPolicy,
            out uint policy,
            UIntPtr length);

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
        private static extern uint WaitForMultipleObjects(
            uint count,
            IntPtr[] handles,
            [MarshalAs(UnmanagedType.Bool)] bool waitAll,
            uint milliseconds);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr OpenProcess(
            uint desiredAccess,
            [MarshalAs(UnmanagedType.Bool)] bool inheritHandle,
            uint processId);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr GetStdHandle(int standardHandle);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern uint GetWindowsDirectory(StringBuilder buffer, uint size);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint GetFileType(IntPtr handle);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetHandleInformation(IntPtr handle, out uint flags);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CreatePipe(
            out IntPtr readPipe,
            out IntPtr writePipe,
            ref SecurityAttributes pipeAttributes,
            uint size);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CloseHandle(IntPtr handle);

        public static int Main(string[] arguments)
        {
            try
            {
                RequireWindowsX64();
                if (arguments.Length == 1
                    && String.Equals(arguments[0], "--version", StringComparison.Ordinal))
                {
                    Console.Out.WriteLine("projectatlas-parser-containment 1");
                    return 0;
                }
                if (arguments.Length == 1
                    && String.Equals(arguments[0], "--build-contract", StringComparison.Ordinal))
                {
                    Console.Out.WriteLine(BuildContract());
                    return 0;
                }
                if (arguments.Length == 1
                    && String.Equals(arguments[0], "self-test", StringComparison.Ordinal))
                {
                    RunSelfTest();
                    Console.Out.WriteLine("[parser-containment] self-test passed");
                    return 0;
                }
                if (arguments.Length > 0
                    && String.Equals(arguments[0], "serve-worker", StringComparison.Ordinal))
                {
                    return ServeWorker(ParseServeConfiguration(arguments));
                }
                if (arguments.Length == 1
                    && String.Equals(arguments[0], "cleanup-artifact-profile", StringComparison.Ordinal))
                {
                    CleanupArtifactProfile();
                    Console.Out.WriteLine("[parser-containment] artifact profile cleanup passed");
                    return 0;
                }
                throw new ContainmentFailure("parse-command");
            }
            catch (ContainmentFailure failure)
            {
                WriteFailure(failure.Stage, failure.Code);
                return FailureExitCode;
            }
            catch
            {
                WriteFailure("unhandled-containment-error", null);
                return FailureExitCode;
            }
        }

        private static void RequireWindowsX64()
        {
            if (!Environment.Is64BitProcess || Environment.OSVersion.Platform != PlatformID.Win32NT)
            {
                throw new ContainmentFailure("require-windows-x64");
            }
        }

        private static string BuildContract()
        {
            List<string> identities = new List<string>();
            HashSet<string> uniqueIdentities = new HashSet<string>(StringComparer.Ordinal);
            SortedSet<string> modules = new SortedSet<string>(StringComparer.Ordinal);
            foreach (Type type in Assembly.GetExecutingAssembly().GetTypes())
            {
                MethodInfo[] methods = type.GetMethods(
                    BindingFlags.Public
                    | BindingFlags.NonPublic
                    | BindingFlags.Static
                    | BindingFlags.Instance
                    | BindingFlags.DeclaredOnly);
                foreach (MethodInfo method in methods)
                {
                    if ((method.Attributes & MethodAttributes.PinvokeImpl) == 0)
                    {
                        continue;
                    }
                    DllImportAttribute import = (DllImportAttribute)Attribute.GetCustomAttribute(
                        method,
                        typeof(DllImportAttribute));
                    if (import == null || String.IsNullOrEmpty(import.Value))
                    {
                        throw new ContainmentFailure("build-contract-import-metadata");
                    }
                    string module = import.Value.ToLowerInvariant();
                    string entryPoint = String.IsNullOrEmpty(import.EntryPoint)
                        ? method.Name
                        : import.EntryPoint;
                    if (String.IsNullOrEmpty(module) || String.IsNullOrEmpty(entryPoint))
                    {
                        throw new ContainmentFailure("build-contract-empty-import-identity");
                    }
                    string identity = module + "!" + entryPoint.ToLowerInvariant();
                    if (!uniqueIdentities.Add(identity))
                    {
                        throw new ContainmentFailure("build-contract-duplicate-import-identity");
                    }
                    modules.Add(module);
                    identities.Add(identity);
                }
            }
            if (identities.Count == 0)
            {
                throw new ContainmentFailure("build-contract-empty-imports");
            }
            identities.Sort(StringComparer.Ordinal);
            string identityDigest;
            using (SHA256 sha256 = SHA256.Create())
            {
                byte[] canonical = Encoding.UTF8.GetBytes(String.Join("\n", identities.ToArray()));
                byte[] digest = sha256.ComputeHash(canonical);
                StringBuilder hex = new StringBuilder(64);
                foreach (byte value in digest)
                {
                    hex.Append(value.ToString("x2", CultureInfo.InvariantCulture));
                }
                identityDigest = hex.ToString();
            }
            string line = "projectatlas-parser-containment-build-contract-v1"
                + "|runtime=windows-net-framework-clr-v4"
                + "|architecture=x86_64"
                + "|modules=" + String.Join(",", new List<string>(modules).ToArray())
                + "|methods=" + identities.Count.ToString(CultureInfo.InvariantCulture)
                + "|imports_sha256=" + identityDigest;
            if (line.Length > 512 || Encoding.ASCII.GetString(Encoding.ASCII.GetBytes(line)) != line)
            {
                throw new ContainmentFailure("build-contract-output-bound");
            }
            return line;
        }

        private static ServeConfiguration ParseServeConfiguration(string[] arguments)
        {
            if (arguments.Length != 7)
            {
                throw new ContainmentFailure("parse-serve-worker");
            }
            ServeConfiguration configuration = new ServeConfiguration();
            bool hasParent = false;
            bool hasProcessMemory = false;
            bool hasJobMemory = false;
            for (int index = 1; index < arguments.Length; index += 2)
            {
                string option = arguments[index];
                string value = arguments[index + 1];
                if (String.Equals(option, "--parent-pid", StringComparison.Ordinal))
                {
                    uint parentProcessId;
                    if (hasParent
                        || !UInt32.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out parentProcessId)
                        || parentProcessId == 0
                        || parentProcessId == unchecked((uint)Process.GetCurrentProcess().Id))
                    {
                        throw new ContainmentFailure("invalid-parent-pid");
                    }
                    configuration.ParentProcessId = parentProcessId;
                    hasParent = true;
                }
                else if (String.Equals(option, "--process-memory-bytes", StringComparison.Ordinal))
                {
                    ulong processMemoryBytes;
                    if (hasProcessMemory
                        || !UInt64.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out processMemoryBytes))
                    {
                        throw new ContainmentFailure("invalid-process-memory");
                    }
                    configuration.ProcessMemoryBytes = processMemoryBytes;
                    hasProcessMemory = true;
                }
                else if (String.Equals(option, "--job-memory-bytes", StringComparison.Ordinal))
                {
                    ulong jobMemoryBytes;
                    if (hasJobMemory
                        || !UInt64.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out jobMemoryBytes))
                    {
                        throw new ContainmentFailure("invalid-job-memory");
                    }
                    configuration.JobMemoryBytes = jobMemoryBytes;
                    hasJobMemory = true;
                }
                else
                {
                    throw new ContainmentFailure("unknown-serve-worker-option");
                }
            }
            if (!hasParent || !hasProcessMemory || !hasJobMemory)
            {
                throw new ContainmentFailure("missing-serve-worker-option");
            }
            if (configuration.ProcessMemoryBytes < MinimumMemoryBytes
                || configuration.ProcessMemoryBytes > MaximumMemoryBytes)
            {
                throw new ContainmentFailure("process-memory-out-of-range");
            }
            if (configuration.JobMemoryBytes < configuration.ProcessMemoryBytes
                || configuration.JobMemoryBytes > MaximumMemoryBytes)
            {
                throw new ContainmentFailure("job-memory-out-of-range");
            }
            return configuration;
        }

        private static int ServeWorker(ServeConfiguration configuration)
        {
            string broker = CanonicalFile(Process.GetCurrentProcess().MainModule.FileName, "broker");
            string packRoot = Path.GetDirectoryName(broker);
            if (String.IsNullOrEmpty(packRoot))
            {
                throw new ContainmentFailure("resolve-pack-root");
            }
            packRoot = CanonicalDirectory(packRoot, "pack-root");
            string worker = CanonicalFile(Path.Combine(packRoot, WorkerFileName), "worker");
            byte[] artifactManifest = ReadArtifactManifest(Path.Combine(packRoot, ArtifactManifestFileName));
            ProfileIdentity profile = EnsureArtifactProfile(artifactManifest);
            GrantPackReadExecute(packRoot, profile.Sid);

            IntPtr parent = IntPtr.Zero;
            IntPtr job = IntPtr.Zero;
            IntPtr jobCompletionPort = IntPtr.Zero;
            IntPtr workerStderrRead = IntPtr.Zero;
            IntPtr workerStderrWrite = IntPtr.Zero;
            ProcessInformation process = new ProcessInformation();
            bool workerCreated = false;
            uint standardInputFlags = 0;
            uint standardOutputFlags = 0;
            bool restoreStandardInput = false;
            bool restoreStandardOutput = false;
            Thread workerStderrForwarder = null;
            try
            {
                parent = OpenProcess(ProcessSynchronize, false, configuration.ParentProcessId);
                if (IsInvalidHandle(parent))
                {
                    throw Win32Failure("open-parent");
                }
                uint parentState = WaitForSingleObject(parent, 0);
                if (parentState == WaitObject0)
                {
                    throw new ContainmentFailure("parent-already-exited");
                }
                if (parentState == WaitFailed)
                {
                    throw Win32Failure("check-parent");
                }

                IntPtr standardInput = RequireProtocolPipe(StandardInputHandle, "stdin");
                IntPtr standardOutput = RequireProtocolPipe(StandardOutputHandle, "stdout");
                RequireProtocolPipe(StandardErrorHandle, "stderr");
                if (!GetHandleInformation(standardInput, out standardInputFlags))
                {
                    throw Win32Failure("inspect-stdin-handle");
                }
                if (!GetHandleInformation(standardOutput, out standardOutputFlags))
                {
                    throw Win32Failure("inspect-stdout-handle");
                }
                if (!SetHandleInformation(standardInput, HandleFlagInherit, HandleFlagInherit))
                {
                    throw Win32Failure("enable-stdin-inheritance");
                }
                restoreStandardInput = true;
                if (!SetHandleInformation(standardOutput, HandleFlagInherit, HandleFlagInherit))
                {
                    throw Win32Failure("enable-stdout-inheritance");
                }
                restoreStandardOutput = true;

                SecurityAttributes pipeAttributes = new SecurityAttributes();
                pipeAttributes.Length = Marshal.SizeOf(typeof(SecurityAttributes));
                pipeAttributes.InheritHandle = true;
                if (!CreatePipe(out workerStderrRead, out workerStderrWrite, ref pipeAttributes, 0))
                {
                    throw Win32Failure("create-worker-stderr-pipe");
                }
                if (!SetHandleInformation(workerStderrRead, HandleFlagInherit, 0))
                {
                    throw Win32Failure("protect-worker-stderr-reader");
                }

                job = CreateConfiguredJob(configuration.ProcessMemoryBytes, configuration.JobMemoryBytes);
                jobCompletionPort = CreateJobCompletionPort(job);
                using (AttributeResources attributes = new AttributeResources())
                {
                    attributes.Build(
                        profile.Sid,
                        new IntPtr[] { standardInput, standardOutput, workerStderrWrite });
                    StartupInfoEx startup = new StartupInfoEx();
                    startup.StartupInfo.cb = (uint)Marshal.SizeOf(typeof(StartupInfoEx));
                    startup.StartupInfo.dwFlags = StartfUseStdHandles;
                    startup.StartupInfo.hStdInput = standardInput;
                    startup.StartupInfo.hStdOutput = standardOutput;
                    startup.StartupInfo.hStdError = workerStderrWrite;
                    startup.AttributeList = attributes.List;
                    IntPtr environment = AllocateWorkerEnvironment(packRoot);
                    try
                    {
                        StringBuilder commandLine = new StringBuilder(QuoteArgument(worker) + " " + WorkerServeArgument);
                        uint creationFlags = CreateSuspended
                            | CreateNoWindow
                            | CreateUnicodeEnvironment
                            | ExtendedStartupInfoPresent;
                        if (!CreateProcessW(
                            worker,
                            commandLine,
                            IntPtr.Zero,
                            IntPtr.Zero,
                            true,
                            creationFlags,
                            environment,
                            packRoot,
                            ref startup,
                            out process))
                        {
                            throw Win32Failure("create-suspended-worker");
                        }
                        workerCreated = true;
                    }
                    finally
                    {
                        if (environment != IntPtr.Zero)
                        {
                            Marshal.FreeHGlobal(environment);
                        }
                    }
                }
                if (SelfTestMarkerMatches(packRoot, SelfTestPreAssignmentFaultMarker))
                {
                    throw new ContainmentFailure("self-test-before-job-assignment");
                }
                RestoreInheritance(
                    RequireProtocolPipe(StandardInputHandle, "stdin"),
                    standardInputFlags,
                    "restore-stdin-inheritance");
                restoreStandardInput = false;
                RestoreInheritance(
                    RequireProtocolPipe(StandardOutputHandle, "stdout"),
                    standardOutputFlags,
                    "restore-stdout-inheritance");
                restoreStandardOutput = false;
                CloseRequired(ref workerStderrWrite, "close-broker-worker-stderr-writer");

                if (!AssignProcessToJobObject(job, process.Process))
                {
                    throw Win32Failure("assign-worker-job");
                }
                VerifyJob(job, process.Process, configuration.ProcessMemoryBytes, configuration.JobMemoryBytes);
                VerifyWorkerIdentity(process.Process, profile.Sid);
                if (SelfTestMarkerMatches(packRoot, SelfTestMarker))
                {
                    // The builder is managed code and cannot stand in for the native Rust
                    // worker after LPAC resume: CLR initialization needs system files that
                    // the zero-capability LPAC intentionally cannot read. Prove suspended
                    // admission plus Job-owned termination here; the packaged-worker lane
                    // separately proves the production resume/protocol path.
                    if (!StopAndWait(job, process.Process))
                    {
                        throw new ContainmentFailure("self-test-reap-suspended-worker");
                    }
                    workerCreated = false;
                    WriteStandardError(
                        SelfTestAdmissionRecord,
                        0,
                        SelfTestAdmissionRecord.Length);
                    return 0;
                }
                if (ResumeThread(process.Thread) == UInt32.MaxValue)
                {
                    throw Win32Failure("resume-worker");
                }

                WriteStandardError(AdmissionRecord, 0, AdmissionRecord.Length);
                workerStderrForwarder = StartWorkerStderrForwarder(workerStderrRead);
                workerStderrRead = IntPtr.Zero;

                IntPtr[] waitHandles = new IntPtr[] { process.Process, parent };
                uint wait = WaitForMultipleObjects(2, waitHandles, false, UInt32.MaxValue);
                if (wait == WaitObject0)
                {
                    uint exitCode;
                    if (!GetExitCodeProcess(process.Process, out exitCode))
                    {
                        throw Win32Failure("read-worker-exit");
                    }
                    JoinWorkerStderrForwarder(workerStderrForwarder);
                    bool memoryLimitReached = ObserveJobMemoryLimit(
                        jobCompletionPort,
                        process.ProcessId);
                    if (memoryLimitReached)
                    {
                        return MemoryLimitExitCode;
                    }
                    if (exitCode == unchecked((uint)MemoryLimitExitCode))
                    {
                        throw new ContainmentFailure("reserved-worker-exit-code");
                    }
                    return unchecked((int)exitCode);
                }
                if (wait == WaitObject0 + 1)
                {
                    if (!StopAndWait(job, process.Process))
                    {
                        throw new ContainmentFailure("reap-worker-after-parent-exit");
                    }
                    JoinWorkerStderrForwarder(workerStderrForwarder);
                    throw new ContainmentFailure("parent-exited");
                }
                throw wait == WaitFailed
                    ? Win32Failure("wait-worker-parent")
                    : new ContainmentFailure("unexpected-wait-result");
            }
            catch
            {
                bool stopped = !workerCreated || StopAndWait(job, process.Process);
                if (!stopped)
                {
                    throw new ContainmentFailure("reap-worker-after-failure");
                }
                JoinWorkerStderrForwarder(workerStderrForwarder);
                throw;
            }
            finally
            {
                if (restoreStandardInput)
                {
                    TryRestoreStandardHandle(StandardInputHandle, standardInputFlags);
                }
                if (restoreStandardOutput)
                {
                    TryRestoreStandardHandle(StandardOutputHandle, standardOutputFlags);
                }
                CloseIfPresent(ref workerStderrWrite);
                CloseIfPresent(ref workerStderrRead);
                CloseIfPresent(ref process.Thread);
                CloseIfPresent(ref process.Process);
                CloseIfPresent(ref jobCompletionPort);
                CloseIfPresent(ref job);
                CloseIfPresent(ref parent);
            }
        }

        private static IntPtr CreateConfiguredJob(ulong processMemoryBytes, ulong jobMemoryBytes)
        {
            IntPtr job = CreateJobObject(IntPtr.Zero, null);
            if (IsInvalidHandle(job))
            {
                throw Win32Failure("create-worker-job");
            }
            JobObjectExtendedLimitInformationValue information = new JobObjectExtendedLimitInformationValue();
            information.BasicLimitInformation.LimitFlags = JobObjectLimitActiveProcess
                | JobObjectLimitProcessMemory
                | JobObjectLimitJobMemory
                | JobObjectLimitKillOnJobClose;
            information.BasicLimitInformation.ActiveProcessLimit = 1;
            information.ProcessMemoryLimit = new UIntPtr(processMemoryBytes);
            information.JobMemoryLimit = new UIntPtr(jobMemoryBytes);
            try
            {
                SetJobInformation(
                    job,
                    JobObjectExtendedLimitInformation,
                    information,
                    "set-worker-job-limits");
            }
            catch
            {
                CloseHandle(job);
                throw;
            }
            return job;
        }

        private static IntPtr CreateJobCompletionPort(IntPtr job)
        {
            IntPtr completionPort = CreateIoCompletionPort(
                InvalidHandleValue,
                IntPtr.Zero,
                UIntPtr.Zero,
                1);
            if (IsInvalidHandle(completionPort))
            {
                throw Win32Failure("create-worker-job-completion-port");
            }
            JobObjectAssociateCompletionPortValue association =
                new JobObjectAssociateCompletionPortValue();
            association.CompletionKey = new IntPtr(unchecked((long)JobCompletionKey));
            association.CompletionPort = completionPort;
            try
            {
                SetJobInformation(
                    job,
                    JobObjectAssociateCompletionPortInformation,
                    association,
                    "associate-worker-job-completion-port");
            }
            catch
            {
                CloseHandle(completionPort);
                throw;
            }
            return completionPort;
        }

        private static void SetJobInformation<T>(
            IntPtr job,
            uint informationClass,
            T information,
            string stage)
            where T : struct
        {
            int size = Marshal.SizeOf(typeof(T));
            IntPtr buffer = Marshal.AllocHGlobal(size);
            try
            {
                Marshal.StructureToPtr(information, buffer, false);
                if (!SetInformationJobObject(
                    job,
                    informationClass,
                    buffer,
                    unchecked((uint)size)))
                {
                    throw Win32Failure(stage);
                }
            }
            finally
            {
                Marshal.DestroyStructure(buffer, typeof(T));
                Marshal.FreeHGlobal(buffer);
            }
        }

        private static bool ObserveJobMemoryLimit(IntPtr completionPort, uint expectedProcessId)
        {
            Stopwatch timeout = Stopwatch.StartNew();
            bool memoryLimitReached = false;
            while (timeout.ElapsedMilliseconds < CleanupWaitMilliseconds)
            {
                uint remaining = unchecked((uint)Math.Max(
                    1,
                    CleanupWaitMilliseconds - timeout.ElapsedMilliseconds));
                uint message;
                UIntPtr completionKey;
                IntPtr messageValue;
                if (!GetQueuedCompletionStatus(
                    completionPort,
                    out message,
                    out completionKey,
                    out messageValue,
                    remaining))
                {
                    int code = Marshal.GetLastWin32Error();
                    if (code == unchecked((int)WaitTimeout))
                    {
                        break;
                    }
                    throw Win32Failure("observe-worker-job-completion");
                }
                if (completionKey.ToUInt64() != JobCompletionKey)
                {
                    throw new ContainmentFailure("worker-job-completion-key-mismatch");
                }
                if (message == JobObjectMessageProcessMemoryLimit
                    || message == JobObjectMessageJobMemoryLimit)
                {
                    ulong observedProcessId = unchecked((ulong)messageValue.ToInt64());
                    if (observedProcessId != expectedProcessId)
                    {
                        throw new ContainmentFailure("worker-job-memory-process-mismatch");
                    }
                    memoryLimitReached = true;
                }
                if (message == JobObjectMessageActiveProcessZero)
                {
                    return memoryLimitReached;
                }
            }
            throw new ContainmentFailure("worker-job-completion-timeout");
        }

        private static void VerifyJob(
            IntPtr job,
            IntPtr process,
            ulong processMemoryBytes,
            ulong jobMemoryBytes)
        {
            bool inJob;
            if (!IsProcessInJob(process, job, out inJob))
            {
                throw Win32Failure("verify-worker-job-membership");
            }
            if (!inJob)
            {
                throw new ContainmentFailure("worker-outside-job");
            }
            JobObjectExtendedLimitInformationValue actual;
            uint returned;
            if (!QueryInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                out actual,
                (uint)Marshal.SizeOf(typeof(JobObjectExtendedLimitInformationValue)),
                out returned))
            {
                throw Win32Failure("query-worker-job-limits");
            }
            uint requiredFlags = JobObjectLimitActiveProcess
                | JobObjectLimitProcessMemory
                | JobObjectLimitJobMemory
                | JobObjectLimitKillOnJobClose;
            if (returned != Marshal.SizeOf(typeof(JobObjectExtendedLimitInformationValue))
                || (actual.BasicLimitInformation.LimitFlags & requiredFlags) != requiredFlags
                || actual.BasicLimitInformation.ActiveProcessLimit != 1
                || actual.ProcessMemoryLimit.ToUInt64() != processMemoryBytes
                || actual.JobMemoryLimit.ToUInt64() != jobMemoryBytes)
            {
                throw new ContainmentFailure("worker-job-limit-mismatch");
            }
        }

        private static void VerifyWorkerIdentity(IntPtr process, SecurityIdentifier expectedSid)
        {
            IntPtr token = IntPtr.Zero;
            IntPtr expectedSidPointer = IntPtr.Zero;
            IntPtr tokenBuffer = IntPtr.Zero;
            try
            {
                if (!OpenProcessToken(process, TokenQuery, out token))
                {
                    throw Win32Failure("open-worker-token");
                }
                if (ReadTokenUInt32(token, TokenIsAppContainer, "read-appcontainer-token") != 1)
                {
                    throw new ContainmentFailure("worker-not-appcontainer");
                }
                VerifyLessPrivilegedAppContainerGroups(token);
                VerifyChildProcessPolicy(process);

                uint capabilitiesLength;
                tokenBuffer = ReadTokenBuffer(token, TokenCapabilities, "read-capabilities-token", out capabilitiesLength);
                if (capabilitiesLength < sizeof(uint) || unchecked((uint)Marshal.ReadInt32(tokenBuffer)) != 0)
                {
                    throw new ContainmentFailure("worker-capabilities-not-empty");
                }
                Marshal.FreeHGlobal(tokenBuffer);
                tokenBuffer = IntPtr.Zero;

                uint sidLength;
                tokenBuffer = ReadTokenBuffer(token, TokenAppContainerSid, "read-appcontainer-sid", out sidLength);
                if (sidLength < IntPtr.Size)
                {
                    throw new ContainmentFailure("worker-appcontainer-sid-missing");
                }
                IntPtr actualSid = Marshal.ReadIntPtr(tokenBuffer);
                byte[] sidBytes = new byte[expectedSid.BinaryLength];
                expectedSid.GetBinaryForm(sidBytes, 0);
                expectedSidPointer = Marshal.AllocHGlobal(sidBytes.Length);
                Marshal.Copy(sidBytes, 0, expectedSidPointer, sidBytes.Length);
                if (actualSid == IntPtr.Zero || !EqualSid(actualSid, expectedSidPointer))
                {
                    throw new ContainmentFailure("worker-appcontainer-sid-mismatch");
                }
            }
            finally
            {
                if (tokenBuffer != IntPtr.Zero)
                {
                    Marshal.FreeHGlobal(tokenBuffer);
                }
                if (expectedSidPointer != IntPtr.Zero)
                {
                    Marshal.FreeHGlobal(expectedSidPointer);
                }
                CloseIfPresent(ref token);
            }
        }

        private static void VerifyLessPrivilegedAppContainerGroups(IntPtr token)
        {
            uint length;
            IntPtr buffer = ReadTokenBuffer(token, TokenGroups, "read-token-groups", out length);
            try
            {
                if (length < sizeof(uint))
                {
                    throw new ContainmentFailure("worker-token-groups-missing");
                }
                uint count = unchecked((uint)Marshal.ReadInt32(buffer));
                int offset = IntPtr.Size;
                int entrySize = checked(IntPtr.Size * 2);
                ulong required = checked((ulong)offset + ((ulong)count * (ulong)entrySize));
                if (required > length)
                {
                    throw new ContainmentFailure("worker-token-groups-size");
                }
                for (uint index = 0; index < count; index += 1)
                {
                    int entryOffset = checked(offset + checked((int)index * entrySize));
                    IntPtr sidPointer = Marshal.ReadIntPtr(buffer, entryOffset);
                    if (sidPointer == IntPtr.Zero)
                    {
                        throw new ContainmentFailure("worker-token-group-sid-missing");
                    }
                    string sid = new SecurityIdentifier(sidPointer).Value;
                    if (String.Equals(sid, AllApplicationPackagesSid, StringComparison.Ordinal)
                        || String.Equals(
                            sid,
                            AllRestrictedApplicationPackagesSid,
                            StringComparison.Ordinal))
                    {
                        throw new ContainmentFailure("worker-not-lpac");
                    }
                }
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }

        private static void VerifyChildProcessPolicy(IntPtr process)
        {
            uint policy;
            if (!GetProcessMitigationPolicy(
                process,
                ProcessChildProcessPolicy,
                out policy,
                new UIntPtr(sizeof(uint))))
            {
                throw Win32Failure("read-child-process-policy");
            }
            if (policy != ProcessMitigationNoChildProcessCreation)
            {
                throw new ContainmentFailure("worker-child-policy-mismatch");
            }
        }

        private static uint ReadTokenUInt32(IntPtr token, uint informationClass, string stage)
        {
            IntPtr buffer = Marshal.AllocHGlobal(sizeof(uint));
            try
            {
                uint returned;
                if (!GetTokenInformation(token, informationClass, buffer, sizeof(uint), out returned))
                {
                    throw Win32Failure(stage);
                }
                if (returned != sizeof(uint))
                {
                    throw new ContainmentFailure(stage + "-size");
                }
                return unchecked((uint)Marshal.ReadInt32(buffer));
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }

        private static IntPtr ReadTokenBuffer(
            IntPtr token,
            uint informationClass,
            string stage,
            out uint length)
        {
            length = 0;
            if (GetTokenInformation(token, informationClass, IntPtr.Zero, 0, out length)
                || Marshal.GetLastWin32Error() != ErrorInsufficientBuffer
                || length == 0
                || length > MaximumTokenInformationBytes)
            {
                throw Win32Failure(stage + "-measure");
            }
            IntPtr buffer = Marshal.AllocHGlobal(checked((int)length));
            if (!GetTokenInformation(token, informationClass, buffer, length, out length))
            {
                Marshal.FreeHGlobal(buffer);
                throw Win32Failure(stage);
            }
            return buffer;
        }

        private static ProfileIdentity EnsureArtifactProfile(byte[] artifactManifest)
        {
            string name = ProfileName(artifactManifest);
            IntPtr sid = IntPtr.Zero;
            try
            {
                int result = CreateAppContainerProfile(
                    name,
                    "ProjectAtlas parser containment",
                    "Artifact-scoped ProjectAtlas optional parser containment",
                    IntPtr.Zero,
                    0,
                    out sid);
                if (result < 0 && unchecked((uint)result) != 0x80070000U + ErrorAlreadyExists)
                {
                    throw HResultFailure("create-artifact-profile", result);
                }
                if (result < 0)
                {
                    int deriveResult = DeriveAppContainerSidFromAppContainerName(name, out sid);
                    if (deriveResult < 0)
                    {
                        throw HResultFailure("derive-artifact-profile", deriveResult);
                    }
                }
                if (sid == IntPtr.Zero)
                {
                    throw new ContainmentFailure("artifact-profile-sid-missing");
                }
                return new ProfileIdentity(name, new SecurityIdentifier(sid));
            }
            finally
            {
                if (sid != IntPtr.Zero)
                {
                    FreeSid(sid);
                }
            }
        }

        private static string ProfileName(byte[] artifactManifest)
        {
            using (SHA256 sha256 = SHA256.Create())
            {
                byte[] digest = sha256.ComputeHash(artifactManifest);
                StringBuilder name = new StringBuilder("projectatlas.parser.");
                for (int index = 0; index < 20; index += 1)
                {
                    name.Append(digest[index].ToString("x2", CultureInfo.InvariantCulture));
                }
                return name.ToString();
            }
        }

        private static void GrantPackReadExecute(string packRoot, SecurityIdentifier sid)
        {
            DirectoryInfo directory = new DirectoryInfo(packRoot);
            DirectorySecurity security = directory.GetAccessControl(AccessControlSections.Access);
            FileSystemAccessRule rule = new FileSystemAccessRule(
                sid,
                FileSystemRights.ReadAndExecute | FileSystemRights.Synchronize,
                InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                PropagationFlags.None,
                AccessControlType.Allow);
            security.AddAccessRule(rule);
            directory.SetAccessControl(security);
        }

        private static void RemovePackReadExecute(string packRoot, string profileName)
        {
            IntPtr sidPointer = IntPtr.Zero;
            try
            {
                int result = DeriveAppContainerSidFromAppContainerName(profileName, out sidPointer);
                if (result < 0 || sidPointer == IntPtr.Zero)
                {
                    throw HResultFailure("self-test-derive-profile", result);
                }
                SecurityIdentifier sid = new SecurityIdentifier(sidPointer);
                SecurityIdentifier currentUser = WindowsIdentity.GetCurrent().User;
                foreach (string file in Directory.GetFiles(packRoot, "*", SearchOption.AllDirectories))
                {
                    FileInfo information = new FileInfo(file);
                    FileSecurity security = information.GetAccessControl(AccessControlSections.Access);
                    security.SetAccessRuleProtection(true, true);
                    // Persist and reload protection first. Windows will not remove an inherited
                    // ACE from an in-memory descriptor merely because that descriptor is about
                    // to become protected; after this round trip the copied ACE is explicit and
                    // can be removed deterministically.
                    information.SetAccessControl(security);
                    security = information.GetAccessControl(AccessControlSections.Access);
                    RemoveIdentityRules(security, sid);
                    security.AddAccessRule(new FileSystemAccessRule(
                        currentUser,
                        FileSystemRights.FullControl,
                        AccessControlType.Allow));
                    information.SetAccessControl(security);
                }
                List<string> directories = new List<string>(
                    Directory.GetDirectories(packRoot, "*", SearchOption.AllDirectories));
                directories.Add(packRoot);
                foreach (string path in directories)
                {
                    DirectoryInfo directory = new DirectoryInfo(path);
                    DirectorySecurity security = directory.GetAccessControl(AccessControlSections.Access);
                    security.SetAccessRuleProtection(true, true);
                    // See the file case above: commit protection before removing the copied
                    // profile ACE so inherited state cannot survive as an explicit rule.
                    directory.SetAccessControl(security);
                    security = directory.GetAccessControl(AccessControlSections.Access);
                    RemoveIdentityRules(security, sid);
                    security.AddAccessRule(new FileSystemAccessRule(
                        currentUser,
                        FileSystemRights.FullControl,
                        InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                        PropagationFlags.None,
                        AccessControlType.Allow));
                    directory.SetAccessControl(security);
                }
            }
            finally
            {
                if (sidPointer != IntPtr.Zero)
                {
                    FreeSid(sidPointer);
                }
            }
        }

        private static void CleanupArtifactProfile()
        {
            string executable = Process.GetCurrentProcess().MainModule.FileName;
            string packRoot = Path.GetDirectoryName(executable);
            if (String.IsNullOrEmpty(packRoot))
            {
                throw new ContainmentFailure("cleanup-pack-root");
            }
            packRoot = CanonicalDirectory(packRoot, "cleanup-pack-root");
            byte[] artifactManifest = ReadArtifactManifest(
                Path.Combine(packRoot, ArtifactManifestFileName));
            string profileName = ProfileName(artifactManifest);
            RemovePackReadExecute(packRoot, profileName);
            DeleteArtifactProfileIfPresent(profileName);
        }

        private static void DeleteArtifactProfileIfPresent(string profileName)
        {
            int result = DeleteAppContainerProfile(profileName);
            uint code = unchecked((uint)result);
            if (result < 0
                && code != 0x80070000U + ErrorFileNotFound
                && code != 0x80070000U + ErrorNotFound)
            {
                throw HResultFailure("cleanup-delete-profile", result);
            }
        }

        private static void RemoveIdentityRules(
            FileSystemSecurity security,
            SecurityIdentifier identity)
        {
            AuthorizationRuleCollection rules = security.GetAccessRules(
                true,
                true,
                typeof(SecurityIdentifier));
            foreach (AuthorizationRule authorization in rules)
            {
                FileSystemAccessRule rule = authorization as FileSystemAccessRule;
                if (rule != null && identity.Equals(rule.IdentityReference))
                {
                    security.RemoveAccessRuleSpecific(rule);
                }
            }
        }

        private static byte[] ReadArtifactManifest(string path)
        {
            FileInfo manifest = new FileInfo(path);
            if (!manifest.Exists || manifest.Length < 1 || manifest.Length > MaximumArtifactManifestBytes)
            {
                throw new ContainmentFailure("artifact-manifest-size");
            }
            byte[] bytes = File.ReadAllBytes(path);
            if (bytes.Length < 1 || bytes.Length > MaximumArtifactManifestBytes)
            {
                throw new ContainmentFailure("artifact-manifest-read-size");
            }
            return bytes;
        }

        private static string CanonicalFile(string path, string stage)
        {
            string full = Path.GetFullPath(path);
            if (!Path.IsPathRooted(full) || !File.Exists(full))
            {
                throw new ContainmentFailure(stage + "-missing");
            }
            return full;
        }

        private static string CanonicalDirectory(string path, string stage)
        {
            string full = Path.GetFullPath(path).TrimEnd(
                Path.DirectorySeparatorChar,
                Path.AltDirectorySeparatorChar);
            if (!Path.IsPathRooted(full) || !Directory.Exists(full))
            {
                throw new ContainmentFailure(stage + "-missing");
            }
            return full;
        }

        private static IntPtr RequireProtocolPipe(int standardHandle, string stage)
        {
            IntPtr handle = GetStdHandle(standardHandle);
            if (IsInvalidHandle(handle) || GetFileType(handle) != FileTypePipe)
            {
                throw new ContainmentFailure(stage + "-not-pipe");
            }
            return handle;
        }

        private static IntPtr AllocateWorkerEnvironment(string packRoot)
        {
            SortedDictionary<string, string> environment = new SortedDictionary<string, string>(
                StringComparer.OrdinalIgnoreCase);
            string windowsDirectory = ResolveWindowsDirectory();
            environment.Add("LOCALAPPDATA", packRoot);
            environment.Add("SystemRoot", windowsDirectory);
            environment.Add("WINDIR", windowsDirectory);
            StringBuilder block = new StringBuilder();
            foreach (KeyValuePair<string, string> entry in environment)
            {
                block.Append(entry.Key).Append('=').Append(entry.Value).Append('\0');
            }
            block.Append('\0');
            return Marshal.StringToHGlobalUni(block.ToString());
        }

        private static string ResolveWindowsDirectory()
        {
            StringBuilder buffer = new StringBuilder(261);
            uint length = GetWindowsDirectory(buffer, (uint)buffer.Capacity);
            if (length == 0 || length >= buffer.Capacity)
            {
                throw Win32Failure("resolve-windows-directory");
            }
            return buffer.ToString();
        }

        private static void RestoreInheritance(IntPtr handle, uint originalFlags, string stage)
        {
            if (!SetHandleInformation(
                handle,
                HandleFlagInherit,
                originalFlags & HandleFlagInherit))
            {
                throw Win32Failure(stage);
            }
        }

        private static void TryRestoreStandardHandle(int standardHandle, uint originalFlags)
        {
            IntPtr handle = GetStdHandle(standardHandle);
            if (!IsInvalidHandle(handle))
            {
                SetHandleInformation(handle, HandleFlagInherit, originalFlags & HandleFlagInherit);
            }
        }

        private static bool StopAndWait(IntPtr job, IntPtr process)
        {
            if (IsInvalidHandle(process))
            {
                return true;
            }
            uint state = WaitForSingleObject(process, 0);
            if (state == WaitObject0)
            {
                return true;
            }
            if (!IsInvalidHandle(job))
            {
                TerminateJobObject(job, FailureExitCode);
            }
            if (WaitForSingleObject(process, 0) != WaitObject0)
            {
                // Assignment can fail after suspended process creation. Terminating an
                // empty Job succeeds without terminating that still-unassigned process,
                // so the direct process handle remains the final cleanup authority.
                TerminateProcess(process, FailureExitCode);
            }
            return WaitForSingleObject(process, CleanupWaitMilliseconds) == WaitObject0;
        }

        private static bool SelfTestMarkerMatches(string packRoot, string expected)
        {
            string path = Path.Combine(packRoot, SelfTestMarkerFileName);
            return File.Exists(path)
                && String.Equals(File.ReadAllText(path, Encoding.ASCII), expected, StringComparison.Ordinal);
        }

        private static Thread StartWorkerStderrForwarder(IntPtr readHandle)
        {
            Thread forwarder = new Thread(delegate()
            {
                try
                {
                    using (FileStream input = new FileStream(
                        new SafeFileHandle(readHandle, true),
                        FileAccess.Read,
                        4096,
                        false))
                    {
                        byte[] buffer = new byte[4096];
                        int count;
                        while ((count = input.Read(buffer, 0, buffer.Length)) > 0)
                        {
                            WriteStandardError(buffer, 0, count);
                        }
                    }
                }
                catch (IOException)
                {
                    // The supervisor owns the receiving pipe and its closure ends forwarding.
                }
            });
            forwarder.IsBackground = true;
            forwarder.Name = "parser-worker-stderr";
            forwarder.Start();
            return forwarder;
        }

        private static void JoinWorkerStderrForwarder(Thread forwarder)
        {
            if (forwarder != null && !forwarder.Join(checked((int)CleanupWaitMilliseconds)))
            {
                throw new ContainmentFailure("worker-stderr-drain-timeout");
            }
        }

        private static void WriteStandardError(byte[] bytes, int offset, int count)
        {
            lock (StandardErrorLock)
            {
                Stream error = Console.OpenStandardError();
                error.Write(bytes, offset, count);
                error.Flush();
            }
        }

        private static void WriteFailure(string stage, string code)
        {
            string message = String.IsNullOrEmpty(code)
                ? "[parser-containment] failed at " + stage + "\n"
                : "[parser-containment] failed at " + stage + " (" + code + ")\n";
            byte[] bytes = Encoding.ASCII.GetBytes(message);
            try
            {
                WriteStandardError(bytes, 0, bytes.Length);
            }
            catch
            {
                // A closed supervisor stderr pipe is already a terminal failure boundary.
            }
        }

        private static void RunSelfTest()
        {
            string currentExecutable = Process.GetCurrentProcess().MainModule.FileName;
            string executableDirectory = Path.GetDirectoryName(currentExecutable);
            if (String.IsNullOrEmpty(executableDirectory))
            {
                throw new ContainmentFailure("self-test-executable-directory");
            }
            string temporaryBase = Path.GetFullPath(executableDirectory).TrimEnd(
                Path.DirectorySeparatorChar,
                Path.AltDirectorySeparatorChar);
            string root = Path.Combine(
                temporaryBase,
                "projectatlas-parser-containment-" + Guid.NewGuid().ToString("N"));
            string packRoot = Path.Combine(root, "pack");
            string profileName = null;
            Directory.CreateDirectory(packRoot);
            try
            {
                ProveCleanupAttemptOrder();
                File.Copy(currentExecutable, Path.Combine(packRoot, "projectatlas-parser-containment.exe"), false);
                File.Copy(currentExecutable, Path.Combine(packRoot, WorkerFileName), false);
                File.WriteAllText(
                    Path.Combine(packRoot, SelfTestMarkerFileName),
                    SelfTestMarker,
                    Encoding.ASCII);
                byte[] manifest = Encoding.UTF8.GetBytes(
                    "{\"self_test\":\"" + Guid.NewGuid().ToString("N") + "\"}\n");
                File.WriteAllBytes(Path.Combine(packRoot, ArtifactManifestFileName), manifest);
                profileName = ProfileName(manifest);

                RunBrokerSelfTest(packRoot, 0, SelfTestAdmissionRecord);
                File.WriteAllText(
                    Path.Combine(packRoot, SelfTestMarkerFileName),
                    SelfTestPreAssignmentFaultMarker,
                    Encoding.ASCII);
                RunBrokerSelfTest(
                    packRoot,
                    FailureExitCode,
                    Encoding.ASCII.GetBytes(
                        "[parser-containment] failed at self-test-before-job-assignment\n"));
                RequireNoWorkerProcess(Path.Combine(packRoot, WorkerFileName));
                RunCleanupProfileSelfTest(packRoot);
            }
            finally
            {
                string expectedPrefix = temporaryBase
                    + Path.DirectorySeparatorChar
                    + "projectatlas-parser-containment-";
                List<string> cleanupFailures = AttemptSelfTestCleanup(
                    delegate()
                    {
                        if (!String.IsNullOrEmpty(profileName) && Directory.Exists(packRoot))
                        {
                            RemovePackReadExecute(packRoot, profileName);
                        }
                    },
                    delegate()
                    {
                        if (!String.IsNullOrEmpty(profileName))
                        {
                            DeleteArtifactProfileIfPresent(profileName);
                        }
                        return 0;
                    },
                    delegate()
                    {
                        if (root.StartsWith(expectedPrefix, StringComparison.OrdinalIgnoreCase)
                            && Directory.Exists(root))
                        {
                            DeleteSelfTestRoot(root);
                        }
                    });
                if (cleanupFailures.Count > 0)
                {
                    throw new ContainmentFailure(
                        "self-test-cleanup",
                        String.Join(",", cleanupFailures.ToArray()));
                }
            }
        }

        private static List<string> AttemptSelfTestCleanup(
            Action removeAcl,
            Func<int> deleteProfile,
            Action deleteRoot)
        {
            List<string> failures = new List<string>();
            try
            {
                removeAcl();
            }
            catch
            {
                failures.Add("acl");
            }
            try
            {
                if (deleteProfile() < 0)
                {
                    failures.Add("profile");
                }
            }
            catch
            {
                failures.Add("profile");
            }
            try
            {
                deleteRoot();
            }
            catch
            {
                failures.Add("root");
            }
            return failures;
        }

        private static void ProveCleanupAttemptOrder()
        {
            for (int fault = 0; fault < 3; fault += 1)
            {
                int attempted = 0;
                List<string> failures = AttemptSelfTestCleanup(
                    delegate()
                    {
                        attempted |= 1;
                        if (fault == 0)
                        {
                            throw new ContainmentFailure("self-test-injected-acl-cleanup");
                        }
                    },
                    delegate()
                    {
                        attempted |= 2;
                        if (fault == 1)
                        {
                            throw new ContainmentFailure("self-test-injected-profile-cleanup");
                        }
                        return 0;
                    },
                    delegate()
                    {
                        attempted |= 4;
                        if (fault == 2)
                        {
                            throw new ContainmentFailure("self-test-injected-root-cleanup");
                        }
                    });
                if (attempted != 7 || failures.Count != 1)
                {
                    throw new ContainmentFailure("self-test-cleanup-attempt-order");
                }
            }
        }

        private static void RunBrokerSelfTest(
            string packRoot,
            int expectedExitCode,
            byte[] expectedStderr)
        {
            ProcessStartInfo start = new ProcessStartInfo();
            start.FileName = Path.Combine(packRoot, "projectatlas-parser-containment.exe");
            start.Arguments = BuildArguments(new string[]
            {
                "serve-worker",
                "--parent-pid",
                Process.GetCurrentProcess().Id.ToString(CultureInfo.InvariantCulture),
                "--process-memory-bytes",
                (128UL * 1024UL * 1024UL).ToString(CultureInfo.InvariantCulture),
                "--job-memory-bytes",
                (192UL * 1024UL * 1024UL).ToString(CultureInfo.InvariantCulture)
            });
            start.UseShellExecute = false;
            start.CreateNoWindow = true;
            start.RedirectStandardInput = true;
            start.RedirectStandardOutput = true;
            start.RedirectStandardError = true;
            using (Process broker = Process.Start(start))
            {
                if (broker == null)
                {
                    throw new ContainmentFailure("self-test-start-broker");
                }
                broker.StandardInput.Close();
                byte[] stdout = null;
                byte[] stderr = null;
                Thread stdoutReader = new Thread(delegate()
                {
                    stdout = ReadToEnd(broker.StandardOutput.BaseStream, 64);
                });
                Thread stderrReader = new Thread(delegate()
                {
                    stderr = ReadToEnd(broker.StandardError.BaseStream, 256);
                });
                stdoutReader.IsBackground = true;
                stderrReader.IsBackground = true;
                stdoutReader.Start();
                stderrReader.Start();
                if (!broker.WaitForExit(30000))
                {
                    broker.Kill();
                    if (!broker.WaitForExit(5000))
                    {
                        throw new ContainmentFailure("self-test-broker-reap-timeout");
                    }
                    throw new ContainmentFailure("self-test-broker-timeout");
                }
                if (!stdoutReader.Join(5000)
                    || !stderrReader.Join(5000)
                    || stdout == null
                    || stderr == null)
                {
                    throw new ContainmentFailure("self-test-stream-drain");
                }
                if (broker.ExitCode != expectedExitCode
                    || stdout.Length != 0
                    || !BytesEqual(stderr, expectedStderr))
                {
                    throw new ContainmentFailure(
                        "self-test-broker-contract",
                        "exit=" + broker.ExitCode.ToString(CultureInfo.InvariantCulture));
                }
            }
        }

        private static void RunCleanupProfileSelfTest(string packRoot)
        {
            byte[] expected = Encoding.ASCII.GetBytes(
                "[parser-containment] artifact profile cleanup passed\r\n");
            for (int attempt = 0; attempt < 2; attempt += 1)
            {
                ProcessStartInfo start = new ProcessStartInfo();
                start.FileName = Path.Combine(packRoot, "projectatlas-parser-containment.exe");
                start.Arguments = "cleanup-artifact-profile";
                start.WorkingDirectory = packRoot;
                start.UseShellExecute = false;
                start.CreateNoWindow = true;
                start.RedirectStandardInput = true;
                start.RedirectStandardOutput = true;
                start.RedirectStandardError = true;
                string windowsDirectory = ResolveWindowsDirectory();
                start.EnvironmentVariables.Clear();
                start.EnvironmentVariables.Add("SystemRoot", windowsDirectory);
                start.EnvironmentVariables.Add("WINDIR", windowsDirectory);
                using (Process broker = Process.Start(start))
                {
                    if (broker == null)
                    {
                        throw new ContainmentFailure("self-test-start-cleanup");
                    }
                    broker.StandardInput.Close();
                    byte[] stdout = null;
                    byte[] stderr = null;
                    Thread stdoutReader = new Thread(delegate()
                    {
                        stdout = ReadToEnd(broker.StandardOutput.BaseStream, 128);
                    });
                    Thread stderrReader = new Thread(delegate()
                    {
                        stderr = ReadToEnd(broker.StandardError.BaseStream, 256);
                    });
                    stdoutReader.IsBackground = true;
                    stderrReader.IsBackground = true;
                    stdoutReader.Start();
                    stderrReader.Start();
                    if (!broker.WaitForExit(30000))
                    {
                        broker.Kill();
                        if (!broker.WaitForExit(5000))
                        {
                            throw new ContainmentFailure("self-test-cleanup-reap-timeout");
                        }
                        throw new ContainmentFailure("self-test-cleanup-timeout");
                    }
                    if (!stdoutReader.Join(5000)
                        || !stderrReader.Join(5000)
                        || stdout == null
                        || stderr == null)
                    {
                        throw new ContainmentFailure("self-test-cleanup-stream-drain");
                    }
                    if (broker.ExitCode != 0
                        || !BytesEqual(stdout, expected)
                        || stderr.Length != 0)
                    {
                        throw new ContainmentFailure(
                            "self-test-cleanup-command",
                            "exit=" + broker.ExitCode.ToString(CultureInfo.InvariantCulture));
                    }
                }
            }
        }

        private static void RequireNoWorkerProcess(string workerPath)
        {
            string expected = Path.GetFullPath(workerPath);
            for (int attempt = 0; attempt < 100; attempt += 1)
            {
                bool found = false;
                Process[] processes = Process.GetProcesses();
                foreach (Process process in processes)
                {
                    using (process)
                    {
                        try
                        {
                            string image = process.MainModule.FileName;
                            if (String.Equals(
                                Path.GetFullPath(image),
                                expected,
                                StringComparison.OrdinalIgnoreCase))
                            {
                                found = true;
                            }
                        }
                        catch
                        {
                            // Processes that cannot be inspected cannot be this same-user,
                            // exact temporary self-test image.
                        }
                    }
                }
                if (!found)
                {
                    return;
                }
                Thread.Sleep(50);
            }
            throw new ContainmentFailure("self-test-worker-survived");
        }

        private static void DeleteSelfTestRoot(string root)
        {
            for (int attempt = 0; attempt < 300; attempt += 1)
            {
                try
                {
                    Directory.Delete(root, true);
                    return;
                }
                catch (UnauthorizedAccessException)
                {
                    if (attempt == 299)
                    {
                        throw;
                    }
                }
                catch (IOException)
                {
                    if (attempt == 299)
                    {
                        throw;
                    }
                }
                Thread.Sleep(100);
            }
        }

        private static byte[] ReadToEnd(Stream stream, int maximumBytes)
        {
            using (MemoryStream output = new MemoryStream())
            {
                byte[] buffer = new byte[64];
                int count;
                while ((count = stream.Read(buffer, 0, buffer.Length)) > 0)
                {
                    if (output.Length + count > maximumBytes)
                    {
                        throw new ContainmentFailure("self-test-stream-overflow");
                    }
                    output.Write(buffer, 0, count);
                }
                return output.ToArray();
            }
        }

        private static bool BytesEqual(byte[] bytes, byte[] expected)
        {
            if (bytes.Length != expected.Length)
            {
                return false;
            }
            for (int index = 0; index < expected.Length; index += 1)
            {
                if (bytes[index] != expected[index])
                {
                    return false;
                }
            }
            return true;
        }

        private static string BuildArguments(string[] arguments)
        {
            StringBuilder commandLine = new StringBuilder();
            foreach (string argument in arguments)
            {
                if (commandLine.Length > 0)
                {
                    commandLine.Append(' ');
                }
                commandLine.Append(QuoteArgument(argument));
            }
            return commandLine.ToString();
        }

        private static string QuoteArgument(string argument)
        {
            StringBuilder quoted = new StringBuilder();
            quoted.Append('"');
            int backslashes = 0;
            foreach (char character in argument)
            {
                if (character == '\\')
                {
                    backslashes += 1;
                }
                else if (character == '"')
                {
                    quoted.Append('\\', backslashes * 2 + 1).Append('"');
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

        private static bool IsInvalidHandle(IntPtr handle)
        {
            return handle == IntPtr.Zero || handle == InvalidHandleValue;
        }

        private static void CloseRequired(ref IntPtr handle, string stage)
        {
            IntPtr closing = handle;
            handle = IntPtr.Zero;
            if (!IsInvalidHandle(closing) && !CloseHandle(closing))
            {
                throw Win32Failure(stage);
            }
        }

        private static void CloseIfPresent(ref IntPtr handle)
        {
            IntPtr closing = handle;
            handle = IntPtr.Zero;
            if (!IsInvalidHandle(closing))
            {
                CloseHandle(closing);
            }
        }

        private static ContainmentFailure Win32Failure(string stage)
        {
            return new ContainmentFailure(
                stage,
                "win32=" + Marshal.GetLastWin32Error().ToString(CultureInfo.InvariantCulture));
        }

        private static ContainmentFailure HResultFailure(string stage, int result)
        {
            return new ContainmentFailure(
                stage,
                "hresult=0x" + unchecked((uint)result).ToString("x8", CultureInfo.InvariantCulture));
        }
    }
}
'@

function Write-BuildFailure {
    param(
        [Parameter(Mandatory)]
        [string] $Stage
    )

    [Console]::Error.WriteLine("[parser-containment-builder] failed at $Stage")
    exit 125
}

function Test-X64Pe {
    param(
        [Parameter(Mandatory)]
        [string] $Path
    )

    $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 0x42 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
        return $false
    }
    $peOffset = [System.BitConverter]::ToInt32($bytes, 0x3c)
    if ($peOffset -lt 0 -or $peOffset + 26 -gt $bytes.Length) {
        return $false
    }
    if ($bytes[$peOffset] -ne 0x50 `
        -or $bytes[$peOffset + 1] -ne 0x45 `
        -or $bytes[$peOffset + 2] -ne 0 `
        -or $bytes[$peOffset + 3] -ne 0) {
        return $false
    }
    $machine = [System.BitConverter]::ToUInt16($bytes, $peOffset + 4)
    $optionalMagic = [System.BitConverter]::ToUInt16($bytes, $peOffset + 24)
    return $machine -eq 0x8664 -and $optionalMagic -eq 0x020b
}

$fullOutputPath = $null
$mayDeleteOutput = $false

if ($env:OS -ne 'Windows_NT' -or -not [System.Environment]::Is64BitProcess) {
    Write-BuildFailure -Stage 'require-windows-x64'
}
if ($PSVersionTable.PSEdition -ne 'Desktop') {
    Write-BuildFailure -Stage 'require-windows-powershell'
}

try {
    $fullOutputPath = [System.IO.Path]::GetFullPath($OutputPath)
    $outputParent = [System.IO.Path]::GetDirectoryName($fullOutputPath)
    if ([string]::IsNullOrWhiteSpace($outputParent) -or -not [System.IO.Directory]::Exists($outputParent)) {
        throw 'output-parent'
    }
    if ([System.IO.File]::Exists($fullOutputPath) -or [System.IO.Directory]::Exists($fullOutputPath)) {
        throw 'output-exists'
    }
    if ([System.IO.Path]::GetFileName($fullOutputPath) -ne 'projectatlas-parser-containment.exe') {
        throw 'output-name'
    }
    $mayDeleteOutput = $true
    $provider = New-Object Microsoft.CSharp.CSharpCodeProvider
    try {
        $compiler = New-Object System.CodeDom.Compiler.CompilerParameters
        $compiler.GenerateExecutable = $true
        $compiler.GenerateInMemory = $false
        $compiler.IncludeDebugInformation = $false
        $compiler.OutputAssembly = $fullOutputPath
        $compiler.CompilerOptions = '/platform:x64 /optimize+'
        $compiler.TreatWarningsAsErrors = $true
        $compiler.WarningLevel = 4
        [void] $compiler.ReferencedAssemblies.Add('System.dll')
        [void] $compiler.ReferencedAssemblies.Add('System.Core.dll')
        $compileResult = $provider.CompileAssemblyFromSource($compiler, $brokerSource)
        if ($compileResult.Errors.HasErrors) {
            throw 'compile-errors'
        }
    }
    finally {
        $provider.Dispose()
    }
    if (-not [System.IO.File]::Exists($fullOutputPath)) {
        throw 'missing-output'
    }
    if (-not (Test-X64Pe -Path $fullOutputPath)) {
        throw 'x64-pe-contract'
    }

    & $fullOutputPath --version | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'version-smoke'
    }
    $buildContract = @(& $fullOutputPath --build-contract)
    if ($LASTEXITCODE -ne 0 `
        -or $buildContract.Count -ne 1 `
        -or $buildContract[0] -notmatch '^projectatlas-parser-containment-build-contract-v1\|runtime=windows-net-framework-clr-v4\|architecture=x86_64\|modules=advapi32\.dll,kernel32\.dll,userenv\.dll\|methods=[1-9][0-9]*\|imports_sha256=[0-9a-f]{64}$') {
        throw 'build-contract-smoke'
    }
    if ($RunSelfTest) {
        & $fullOutputPath self-test
        if ($LASTEXITCODE -ne 0) {
            throw 'self-test'
        }
    }
    $digest = (Get-FileHash -LiteralPath $fullOutputPath -Algorithm SHA256).Hash.ToLowerInvariant()
    [Console]::Out.WriteLine("[parser-containment-builder] sha256=$digest")
}
catch {
    if ($mayDeleteOutput -and $null -ne $fullOutputPath -and [System.IO.File]::Exists($fullOutputPath)) {
        [System.IO.File]::Delete($fullOutputPath)
    }
    Write-BuildFailure -Stage 'compile-or-smoke'
}
