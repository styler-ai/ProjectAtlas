[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateNotNullOrEmpty()]
    [string] $OutputPath,

    [switch] $RunSelfTest
)

# Build once with Windows PowerShell during acquired-input construction. Transfer the resulting
# executable and its SHA-256 to fresh verification; fresh jobs invoke no compiler:
#   launcher.exe self-test
#   launcher.exe launch --executable <absolute> --working-directory <absolute>
#     --temp-directory <absolute> --timeout-seconds <1..86400>
#     [--read-root <absolute>]... --write-root <absolute> [--environment NAME=VALUE]... -- [argv]...
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$launcherSource = @'
using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Threading;

namespace ProjectAtlas.Release
{
    internal static class ParserPackAppContainerLauncher
    {
        private const int FailureExitCode = 125;
        private const int TimeoutExitCode = 124;
        private const int MaximumRootCount = 64;
        private const int MaximumArgumentCount = 256;
        private const int MaximumEnvironmentCount = 128;
        private const int MaximumEnvironmentBytes = 65534;
        private const int MaximumTimeoutSeconds = 86400;
        private const int ErrorInsufficientBuffer = 122;
        private const uint TokenQuery = 0x0008;
        private const uint TokenIsAppContainer = 29;
        private const uint CreateSuspended = 0x00000004;
        private const uint CreateNoWindow = 0x08000000;
        private const uint CreateUnicodeEnvironment = 0x00000400;
        private const uint ExtendedStartupInfoPresent = 0x00080000;
        private const uint WaitObject0 = 0x00000000;
        private const uint WaitTimeout = 0x00000102;
        private const uint JobObjectExtendedLimitInformation = 9;
        private const uint JobObjectLimitKillOnJobClose = 0x00002000;
        private static readonly IntPtr ProcThreadAttributeSecurityCapabilities = new IntPtr(0x00020009);
        private static readonly StringComparer PathComparer = StringComparer.OrdinalIgnoreCase;

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

        private sealed class LaunchConfiguration
        {
            internal LaunchConfiguration()
            {
                Arguments = new List<string>();
                ReadOnlyRoots = new List<string>();
                ReadWriteRoots = new List<string>();
                Environment = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
                TimeoutSeconds = 3600;
            }

            internal string Executable { get; set; }
            internal string WorkingDirectory { get; set; }
            internal string TempDirectory { get; set; }
            internal int TimeoutSeconds { get; set; }
            internal List<string> Arguments { get; private set; }
            internal List<string> ReadOnlyRoots { get; private set; }
            internal List<string> ReadWriteRoots { get; private set; }
            internal Dictionary<string, string> Environment { get; private set; }
        }

        private sealed class Profile : IDisposable
        {
            internal Profile(string name, SecurityIdentifier sid)
            {
                Name = name;
                Sid = sid;
            }

            internal string Name { get; private set; }
            internal SecurityIdentifier Sid { get; private set; }

            public void Dispose()
            {
                int result = DeleteAppContainerProfile(Name);
                if (result < 0)
                {
                    throw HResultFailure("delete-profile", result);
                }
            }
        }

        private sealed class AclSnapshot
        {
            // Preserve the binary DACL for restoration and its effective contract for verification.
            internal AclSnapshot(string path, byte[] descriptor, AclContract contract)
            {
                Path = path;
                Descriptor = descriptor;
                Contract = contract;
            }

            internal string Path { get; private set; }
            internal byte[] Descriptor { get; private set; }
            internal AclContract Contract { get; private set; }
        }

        private sealed class AclContract
        {
            internal AclContract(string owner, bool daclPresent, bool protectedRules, string rules)
            {
                Owner = owner;
                DaclPresent = daclPresent;
                ProtectedRules = protectedRules;
                Rules = rules;
            }

            private string Owner { get; set; }
            private bool DaclPresent { get; set; }
            private bool ProtectedRules { get; set; }
            private string Rules { get; set; }

            internal string Difference(AclContract other)
            {
                if (!String.Equals(Owner, other.Owner, StringComparison.Ordinal))
                {
                    return "owner";
                }
                if (DaclPresent != other.DaclPresent)
                {
                    return "dacl-presence";
                }
                if (ProtectedRules != other.ProtectedRules)
                {
                    return "protection";
                }
                if (!String.Equals(Rules, other.Rules, StringComparison.Ordinal))
                {
                    return "rules";
                }
                return null;
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
            out uint tokenInformation,
            uint tokenInformationLength,
            out uint returnLength);

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
            ref JobObjectExtendedLimitInformationValue information,
            uint informationLength);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool TerminateJobObject(IntPtr job, uint exitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint ResumeThread(IntPtr thread);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool TerminateProcess(IntPtr process, uint exitCode);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CloseHandle(IntPtr handle);

        public static int Main(string[] arguments)
        {
            try
            {
                if (!Environment.Is64BitProcess || Environment.OSVersion.Platform != PlatformID.Win32NT)
                {
                    throw new ContainmentFailure("require-windows-x64");
                }
                if (arguments.Length == 1 && String.Equals(arguments[0], "--version", StringComparison.Ordinal))
                {
                    Console.Out.WriteLine("projectatlas-parser-pack-appcontainer-launcher 1");
                    return 0;
                }
                if (arguments.Length == 1 && String.Equals(arguments[0], "self-test", StringComparison.Ordinal))
                {
                    RunSelfTest();
                    Console.Out.WriteLine("[appcontainer] self-test passed");
                    return 0;
                }
                if (arguments.Length > 0 && String.Equals(arguments[0], "canary", StringComparison.Ordinal))
                {
                    return RunCanary(arguments);
                }
                if (arguments.Length > 0 && String.Equals(arguments[0], "launch", StringComparison.Ordinal))
                {
                    return LaunchContained(ParseLaunch(arguments));
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

        private static LaunchConfiguration ParseLaunch(string[] arguments)
        {
            LaunchConfiguration configuration = new LaunchConfiguration();
            int index = 1;
            bool reachedArguments = false;
            while (index < arguments.Length)
            {
                string option = arguments[index];
                index += 1;
                if (String.Equals(option, "--", StringComparison.Ordinal))
                {
                    reachedArguments = true;
                    break;
                }
                if (index >= arguments.Length)
                {
                    throw new ContainmentFailure("parse-launch-option");
                }
                string value = arguments[index];
                index += 1;
                if (String.Equals(option, "--executable", StringComparison.Ordinal))
                {
                    RequireUnset(configuration.Executable, "duplicate-executable");
                    configuration.Executable = value;
                }
                else if (String.Equals(option, "--working-directory", StringComparison.Ordinal))
                {
                    RequireUnset(configuration.WorkingDirectory, "duplicate-working-directory");
                    configuration.WorkingDirectory = value;
                }
                else if (String.Equals(option, "--temp-directory", StringComparison.Ordinal))
                {
                    RequireUnset(configuration.TempDirectory, "duplicate-temp-directory");
                    configuration.TempDirectory = value;
                }
                else if (String.Equals(option, "--timeout-seconds", StringComparison.Ordinal))
                {
                    int timeout;
                    if (!Int32.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out timeout)
                        || timeout < 1
                        || timeout > MaximumTimeoutSeconds)
                    {
                        throw new ContainmentFailure("invalid-timeout");
                    }
                    configuration.TimeoutSeconds = timeout;
                }
                else if (String.Equals(option, "--read-root", StringComparison.Ordinal))
                {
                    configuration.ReadOnlyRoots.Add(value);
                }
                else if (String.Equals(option, "--write-root", StringComparison.Ordinal))
                {
                    configuration.ReadWriteRoots.Add(value);
                }
                else if (String.Equals(option, "--environment", StringComparison.Ordinal))
                {
                    AddEnvironment(configuration.Environment, value);
                }
                else
                {
                    throw new ContainmentFailure("unknown-launch-option");
                }
            }
            if (reachedArguments)
            {
                while (index < arguments.Length)
                {
                    configuration.Arguments.Add(arguments[index]);
                    index += 1;
                }
            }
            return configuration;
        }

        private static void RequireUnset(string value, string stage)
        {
            if (value != null)
            {
                throw new ContainmentFailure(stage);
            }
        }

        private static void AddEnvironment(Dictionary<string, string> environment, string assignment)
        {
            int separator = assignment.IndexOf('=');
            if (separator <= 0)
            {
                throw new ContainmentFailure("invalid-environment-assignment");
            }
            string name = assignment.Substring(0, separator);
            string value = assignment.Substring(separator + 1);
            ValidateEnvironmentName(name);
            ValidateEnvironmentValue(value);
            if (environment.ContainsKey(name))
            {
                throw new ContainmentFailure("duplicate-environment-name");
            }
            environment.Add(name, value);
        }

        private static int LaunchContained(LaunchConfiguration configuration)
        {
            ValidateConfiguration(configuration);
            string executable = CanonicalFile(configuration.Executable, "executable");
            List<string> readOnlyRoots = CanonicalRoots(configuration.ReadOnlyRoots, "read-root");
            List<string> readWriteRoots = CanonicalRoots(configuration.ReadWriteRoots, "write-root");
            string workingDirectory = CanonicalDirectory(configuration.WorkingDirectory, "working-directory");
            string tempDirectory = CanonicalDirectory(configuration.TempDirectory, "temp-directory");

            List<string> allRoots = new List<string>(readOnlyRoots);
            allRoots.AddRange(readWriteRoots);
            if (!AnyRootContains(allRoots, executable))
            {
                throw new ContainmentFailure("executable-outside-declared-roots");
            }
            if (!AnyRootContains(allRoots, workingDirectory))
            {
                throw new ContainmentFailure("working-directory-outside-declared-roots");
            }
            if (!AnyRootContains(readWriteRoots, tempDirectory))
            {
                throw new ContainmentFailure("temp-directory-outside-write-roots");
            }

            Dictionary<string, string> environment = BuildSafeEnvironment(configuration.Environment, tempDirectory);
            List<AclSnapshot> snapshots = new List<AclSnapshot>();
            Profile profile = null;
            bool cleanupFailed = false;
            try
            {
                profile = CreateProfile();
                HashSet<string> writeSet = new HashSet<string>(readWriteRoots, PathComparer);
                foreach (string root in readOnlyRoots)
                {
                    if (!writeSet.Contains(root))
                    {
                        GrantRoot(root, profile.Sid, FileSystemRights.ReadAndExecute | FileSystemRights.Synchronize, snapshots);
                    }
                }
                foreach (string root in readWriteRoots)
                {
                    GrantRoot(root, profile.Sid, FileSystemRights.Modify | FileSystemRights.Synchronize, snapshots);
                }
                uint timeoutMilliseconds = checked((uint)configuration.TimeoutSeconds * 1000U);
                return LaunchCore(
                    profile.Name,
                    executable,
                    configuration.Arguments.ToArray(),
                    workingDirectory,
                    environment,
                    timeoutMilliseconds);
            }
            finally
            {
                if (!RestoreRoots(snapshots))
                {
                    cleanupFailed = true;
                }
                if (profile != null)
                {
                    try
                    {
                        profile.Dispose();
                    }
                    catch
                    {
                        cleanupFailed = true;
                    }
                }
                if (cleanupFailed)
                {
                    throw new ContainmentFailure("containment-cleanup");
                }
            }
        }

        private static void ValidateConfiguration(LaunchConfiguration configuration)
        {
            if (String.IsNullOrWhiteSpace(configuration.Executable)
                || String.IsNullOrWhiteSpace(configuration.WorkingDirectory)
                || String.IsNullOrWhiteSpace(configuration.TempDirectory))
            {
                throw new ContainmentFailure("missing-launch-input");
            }
            if (configuration.ReadOnlyRoots.Count > MaximumRootCount
                || configuration.ReadWriteRoots.Count == 0
                || configuration.ReadWriteRoots.Count > MaximumRootCount)
            {
                throw new ContainmentFailure("root-count-bound");
            }
            if (configuration.Arguments.Count > MaximumArgumentCount)
            {
                throw new ContainmentFailure("argument-count-bound");
            }
            foreach (string argument in configuration.Arguments)
            {
                if (argument == null || argument.IndexOf('\0') >= 0 || argument.Length > 32767)
                {
                    throw new ContainmentFailure("invalid-argument");
                }
            }
            if (configuration.Environment.Count > MaximumEnvironmentCount)
            {
                throw new ContainmentFailure("environment-count-bound");
            }
        }

        private static List<string> CanonicalRoots(List<string> roots, string role)
        {
            HashSet<string> unique = new HashSet<string>(PathComparer);
            foreach (string root in roots)
            {
                string canonical = CanonicalDirectory(root, role);
                if (!unique.Add(canonical))
                {
                    throw new ContainmentFailure("duplicate-" + role);
                }
            }
            List<string> result = new List<string>(unique);
            result.Sort(PathComparer);
            return result;
        }

        private static string CanonicalDirectory(string path, string role)
        {
            if (String.IsNullOrWhiteSpace(path) || path.IndexOf('\0') >= 0)
            {
                throw new ContainmentFailure("invalid-" + role);
            }
            string full = Path.GetFullPath(path).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
            if (!Directory.Exists(full))
            {
                throw new ContainmentFailure("missing-" + role);
            }
            FileAttributes attributes = File.GetAttributes(full);
            if ((attributes & FileAttributes.ReparsePoint) != 0)
            {
                throw new ContainmentFailure("reparse-" + role);
            }
            string volume = Path.GetPathRoot(full).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
            if (PathComparer.Equals(full, volume))
            {
                throw new ContainmentFailure("volume-" + role);
            }
            return full;
        }

        private static string CanonicalFile(string path, string role)
        {
            if (String.IsNullOrWhiteSpace(path) || path.IndexOf('\0') >= 0)
            {
                throw new ContainmentFailure("invalid-" + role);
            }
            string full = Path.GetFullPath(path);
            if (!File.Exists(full) || (File.GetAttributes(full) & FileAttributes.ReparsePoint) != 0)
            {
                throw new ContainmentFailure("missing-or-reparse-" + role);
            }
            return full;
        }

        private static bool AnyRootContains(List<string> roots, string path)
        {
            foreach (string root in roots)
            {
                if (PathComparer.Equals(root, path)
                    || path.StartsWith(root + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
            }
            return false;
        }

        private static Dictionary<string, string> BuildSafeEnvironment(
            Dictionary<string, string> requested,
            string tempDirectory)
        {
            Dictionary<string, string> result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            string[] safeAmbient = new string[]
            {
                "ALLUSERSPROFILE", "APPDATA", "CommonProgramFiles", "CommonProgramFiles(x86)",
                "CommonProgramW6432", "ComSpec", "HOMEDRIVE", "HOMEPATH", "LOCALAPPDATA",
                "NUMBER_OF_PROCESSORS", "OS", "PATHEXT", "PROCESSOR_ARCHITECTURE", "ProgramData",
                "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "PUBLIC", "SystemDrive",
                "SystemRoot", "USERDOMAIN", "USERNAME", "USERPROFILE", "WINDIR"
            };
            foreach (string name in safeAmbient)
            {
                string value = Environment.GetEnvironmentVariable(name);
                if (!String.IsNullOrEmpty(value))
                {
                    result.Add(name, value);
                }
            }

            List<string> ambientSecrets = AmbientSecretValues();
            foreach (KeyValuePair<string, string> entry in requested)
            {
                ValidateEnvironmentName(entry.Key);
                ValidateEnvironmentValue(entry.Value);
                foreach (string secret in ambientSecrets)
                {
                    if (entry.Value.IndexOf(secret, StringComparison.Ordinal) >= 0)
                    {
                        throw new ContainmentFailure("environment-secret-value");
                    }
                }
                result[entry.Key] = entry.Value;
            }
            result["TEMP"] = tempDirectory;
            result["TMP"] = tempDirectory;

            int bytes = 2;
            foreach (KeyValuePair<string, string> entry in result)
            {
                bytes = checked(bytes + 2 * (entry.Key.Length + 1 + entry.Value.Length + 1));
            }
            if (bytes > MaximumEnvironmentBytes)
            {
                throw new ContainmentFailure("environment-byte-bound");
            }
            return result;
        }

        private static List<string> AmbientSecretValues()
        {
            List<string> values = new List<string>();
            foreach (DictionaryEntry entry in Environment.GetEnvironmentVariables())
            {
                string name = Convert.ToString(entry.Key, CultureInfo.InvariantCulture);
                string value = Convert.ToString(entry.Value, CultureInfo.InvariantCulture);
                if (IsSensitiveEnvironmentName(name) && !String.IsNullOrEmpty(value) && value.Length >= 8)
                {
                    values.Add(value);
                }
            }
            return values;
        }

        private static void ValidateEnvironmentName(string name)
        {
            if (String.IsNullOrEmpty(name) || name.Length > 128 || !IsEnvironmentIdentifier(name))
            {
                throw new ContainmentFailure("invalid-environment-name");
            }
            if (name.StartsWith("GITHUB_", StringComparison.OrdinalIgnoreCase)
                || name.StartsWith("ACTIONS_", StringComparison.OrdinalIgnoreCase)
                || name.StartsWith("RUNNER_", StringComparison.OrdinalIgnoreCase)
                || IsSensitiveEnvironmentName(name))
            {
                throw new ContainmentFailure("forbidden-environment-name");
            }
        }

        private static bool IsEnvironmentIdentifier(string name)
        {
            if (!(Char.IsLetter(name[0]) || name[0] == '_'))
            {
                return false;
            }
            for (int index = 1; index < name.Length; index += 1)
            {
                if (!(Char.IsLetterOrDigit(name[index]) || name[index] == '_'))
                {
                    return false;
                }
            }
            return true;
        }

        private static bool IsSensitiveEnvironmentName(string name)
        {
            string normalized = name.ToUpperInvariant();
            string[] segments = normalized.Split('_');
            foreach (string segment in segments)
            {
                if (segment == "TOKEN"
                    || segment == "SECRET"
                    || segment == "PASSWORD"
                    || segment == "PASSWD"
                    || segment == "CREDENTIAL"
                    || segment == "AUTH"
                    || segment == "COOKIE"
                    || segment == "KEY"
                    || segment == "DSN")
                {
                    return true;
                }
            }
            return normalized.Contains("PRIVATE_KEY")
                || normalized.Contains("ACCESS_KEY")
                || normalized.Contains("CONNECTION_STRING");
        }

        private static void ValidateEnvironmentValue(string value)
        {
            if (value == null || value.IndexOf('\0') >= 0 || value.Length > 32767)
            {
                throw new ContainmentFailure("invalid-environment-value");
            }
        }

        private static Profile CreateProfile()
        {
            string name = "ProjectAtlas.ParserPack." + Guid.NewGuid().ToString("N");
            IntPtr sidPointer = IntPtr.Zero;
            int result = CreateAppContainerProfile(
                name,
                "ProjectAtlas parser-pack containment",
                "Temporary zero-capability parser-pack containment profile",
                IntPtr.Zero,
                0,
                out sidPointer);
            if (result < 0 || sidPointer == IntPtr.Zero)
            {
                throw HResultFailure("create-profile", result);
            }
            try
            {
                return new Profile(name, new SecurityIdentifier(sidPointer));
            }
            finally
            {
                FreeSid(sidPointer);
            }
        }

        private static AclContract CaptureAclContract(string path, string stage)
        {
            DirectorySecurity security = new DirectoryInfo(path)
                .GetAccessControl(AccessControlSections.Access | AccessControlSections.Owner);
            if (!security.AreAccessRulesCanonical)
            {
                throw new ContainmentFailure(stage + "-noncanonical-acl");
            }
            SecurityIdentifier owner = security.GetOwner(typeof(SecurityIdentifier))
                as SecurityIdentifier;
            if (owner == null)
            {
                throw new ContainmentFailure(stage + "-missing-owner");
            }
            RawSecurityDescriptor descriptor = new RawSecurityDescriptor(
                security.GetSecurityDescriptorBinaryForm(),
                0);
            bool daclPresent = (descriptor.ControlFlags & ControlFlags.DiscretionaryAclPresent) != 0;
            List<string> rules = new List<string>();
            AuthorizationRuleCollection accessRules = security.GetAccessRules(
                true,
                true,
                typeof(SecurityIdentifier));
            foreach (AuthorizationRule authorizationRule in accessRules)
            {
                FileSystemAccessRule rule = authorizationRule as FileSystemAccessRule;
                SecurityIdentifier identity = authorizationRule.IdentityReference
                    as SecurityIdentifier;
                if (rule == null || identity == null)
                {
                    throw new ContainmentFailure(stage + "-invalid-access-rule");
                }
                rules.Add(String.Join("|", new string[]
                {
                    identity.Value,
                    unchecked((uint)rule.FileSystemRights).ToString("X8", CultureInfo.InvariantCulture),
                    rule.AccessControlType.ToString(),
                    rule.InheritanceFlags.ToString(),
                    rule.PropagationFlags.ToString(),
                    rule.IsInherited ? "inherited" : "explicit"
                }));
            }
            rules.Sort(StringComparer.Ordinal);
            // Windows may normalize auto-inheritance/defaulted/self-relative
            // descriptor metadata when the same DACL is persisted. The stable
            // cleanup contract is the owner, DACL presence, protection state,
            // canonicality, and the complete effective ACE set.
            return new AclContract(
                owner.Value,
                daclPresent,
                security.AreAccessRulesProtected,
                String.Join("\n", rules.ToArray()));
        }

        private static void GrantRoot(
            string path,
            SecurityIdentifier sid,
            FileSystemRights rights,
            List<AclSnapshot> snapshots)
        {
            DirectoryInfo directory = new DirectoryInfo(path);
            DirectorySecurity security = directory.GetAccessControl(AccessControlSections.Access);
            byte[] descriptor = security.GetSecurityDescriptorBinaryForm();
            AclContract contract = CaptureAclContract(path, "grant-root-before");
            snapshots.Add(new AclSnapshot(path, descriptor, contract));
            FileSystemAccessRule rule = new FileSystemAccessRule(
                sid,
                rights,
                InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                PropagationFlags.None,
                AccessControlType.Allow);
            bool modified;
            security.ModifyAccessRule(AccessControlModification.Add, rule, out modified);
            if (!modified)
            {
                throw new ContainmentFailure("grant-root-access");
            }
            directory.SetAccessControl(security);
        }

        private static bool RestoreRoots(List<AclSnapshot> snapshots)
        {
            bool restored = true;
            for (int index = snapshots.Count - 1; index >= 0; index -= 1)
            {
                try
                {
                    AclSnapshot snapshot = snapshots[index];
                    DirectoryInfo directory = new DirectoryInfo(snapshot.Path);
                    DirectorySecurity security = directory.GetAccessControl(AccessControlSections.Access);
                    security.SetSecurityDescriptorBinaryForm(
                        snapshot.Descriptor,
                        AccessControlSections.Access);
                    directory.SetAccessControl(security);
                    string difference = snapshot.Contract.Difference(
                        CaptureAclContract(snapshot.Path, "restore-root-after"));
                    if (difference != null)
                    {
                        if (restored)
                        {
                            WriteFailure("restore-root-" + difference, null);
                        }
                        restored = false;
                    }
                }
                catch
                {
                    if (restored)
                    {
                        WriteFailure("restore-root-operation", null);
                    }
                    restored = false;
                }
            }
            return restored;
        }

        private static int LaunchCore(
            string profileName,
            string applicationName,
            string[] arguments,
            string currentDirectory,
            IDictionary<string, string> environment,
            uint timeoutMilliseconds)
        {
            IntPtr sid = IntPtr.Zero;
            IntPtr attributeList = IntPtr.Zero;
            IntPtr securityCapabilitiesValue = IntPtr.Zero;
            IntPtr environmentBlock = IntPtr.Zero;
            IntPtr job = IntPtr.Zero;
            ProcessInformation process = new ProcessInformation();
            bool attributeListInitialized = false;

            try
            {
                int sidResult = DeriveAppContainerSidFromAppContainerName(profileName, out sid);
                if (sidResult < 0 || sid == IntPtr.Zero)
                {
                    throw HResultFailure("derive-profile-sid", sidResult);
                }

                IntPtr attributeListSize = IntPtr.Zero;
                bool sizeProbe = InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref attributeListSize);
                int sizeProbeError = Marshal.GetLastWin32Error();
                if (sizeProbe || sizeProbeError != ErrorInsufficientBuffer || attributeListSize == IntPtr.Zero)
                {
                    throw new ContainmentFailure("size-attribute-list");
                }
                attributeList = Marshal.AllocHGlobal(attributeListSize);
                if (!InitializeProcThreadAttributeList(attributeList, 1, 0, ref attributeListSize))
                {
                    throw Win32Failure("initialize-attribute-list");
                }
                attributeListInitialized = true;

                SecurityCapabilities capabilities = new SecurityCapabilities();
                capabilities.AppContainerSid = sid;
                securityCapabilitiesValue = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(SecurityCapabilities)));
                Marshal.StructureToPtr(capabilities, securityCapabilitiesValue, false);
                if (!UpdateProcThreadAttribute(
                    attributeList,
                    0,
                    ProcThreadAttributeSecurityCapabilities,
                    securityCapabilitiesValue,
                    new UIntPtr((uint)Marshal.SizeOf(typeof(SecurityCapabilities))),
                    environmentBlock,
                    IntPtr.Zero))
                {
                    throw Win32Failure("set-security-capabilities");
                }

                environmentBlock = Marshal.StringToHGlobalUni(BuildEnvironmentBlock(environment));
                StartupInfoEx startupInfo = new StartupInfoEx();
                startupInfo.StartupInfo.cb = (uint)Marshal.SizeOf(typeof(StartupInfoEx));
                startupInfo.AttributeList = attributeList;
                StringBuilder commandLine = new StringBuilder(BuildCommandLine(applicationName, arguments));
                uint flags = CreateSuspended | CreateNoWindow | CreateUnicodeEnvironment | ExtendedStartupInfoPresent;
                if (!CreateProcessW(
                    applicationName,
                    commandLine,
                    IntPtr.Zero,
                    IntPtr.Zero,
                    false,
                    flags,
                    environmentBlock,
                    currentDirectory,
                    ref startupInfo,
                    out process))
                {
                    throw Win32Failure("create-contained-process");
                }
                if (!ProcessIsAppContainer(process.Process))
                {
                    TerminateProcess(process.Process, FailureExitCode);
                    throw new ContainmentFailure("verify-contained-token");
                }

                job = CreateJobObject(IntPtr.Zero, null);
                if (job == IntPtr.Zero)
                {
                    TerminateProcess(process.Process, FailureExitCode);
                    throw Win32Failure("create-containment-job");
                }
                JobObjectExtendedLimitInformationValue jobInformation = new JobObjectExtendedLimitInformationValue();
                jobInformation.BasicLimitInformation.LimitFlags = JobObjectLimitKillOnJobClose;
                if (!SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    ref jobInformation,
                    (uint)Marshal.SizeOf(typeof(JobObjectExtendedLimitInformationValue))))
                {
                    TerminateProcess(process.Process, FailureExitCode);
                    throw Win32Failure("configure-containment-job");
                }
                if (!AssignProcessToJobObject(job, process.Process))
                {
                    TerminateProcess(process.Process, FailureExitCode);
                    throw Win32Failure("assign-containment-job");
                }
                if (ResumeThread(process.Thread) == UInt32.MaxValue)
                {
                    TerminateJobObject(job, FailureExitCode);
                    throw Win32Failure("resume-contained-process");
                }

                uint wait = WaitForSingleObject(process.Process, timeoutMilliseconds);
                if (wait == WaitTimeout)
                {
                    if (!TerminateJobObject(job, TimeoutExitCode))
                    {
                        throw Win32Failure("terminate-timed-out-process-tree");
                    }
                    WaitForSingleObject(process.Process, 5000);
                    return TimeoutExitCode;
                }
                if (wait != WaitObject0)
                {
                    TerminateJobObject(job, FailureExitCode);
                    throw Win32Failure("wait-contained-process");
                }
                uint exitCode;
                if (!GetExitCodeProcess(process.Process, out exitCode))
                {
                    throw Win32Failure("read-contained-exit-code");
                }
                return unchecked((int)exitCode);
            }
            finally
            {
                if (job != IntPtr.Zero)
                {
                    CloseHandle(job);
                }
                else if (process.Process != IntPtr.Zero)
                {
                    TerminateProcess(process.Process, FailureExitCode);
                }
                if (process.Thread != IntPtr.Zero)
                {
                    CloseHandle(process.Thread);
                }
                if (process.Process != IntPtr.Zero)
                {
                    CloseHandle(process.Process);
                }
                if (environmentBlock != IntPtr.Zero)
                {
                    Marshal.FreeHGlobal(environmentBlock);
                }
                if (attributeListInitialized)
                {
                    DeleteProcThreadAttributeList(attributeList);
                }
                if (attributeList != IntPtr.Zero)
                {
                    Marshal.FreeHGlobal(attributeList);
                }
                if (securityCapabilitiesValue != IntPtr.Zero)
                {
                    Marshal.FreeHGlobal(securityCapabilitiesValue);
                }
                if (sid != IntPtr.Zero)
                {
                    FreeSid(sid);
                }
            }
        }

        private static bool ProcessIsAppContainer(IntPtr process)
        {
            IntPtr token = IntPtr.Zero;
            try
            {
                if (!OpenProcessToken(process, TokenQuery, out token))
                {
                    throw Win32Failure("open-contained-token");
                }
                uint isAppContainer;
                uint returnedLength;
                if (!GetTokenInformation(
                    token,
                    TokenIsAppContainer,
                    out isAppContainer,
                    sizeof(uint),
                    out returnedLength))
                {
                    throw Win32Failure("read-contained-token");
                }
                return returnedLength == sizeof(uint) && isAppContainer != 0;
            }
            finally
            {
                if (token != IntPtr.Zero)
                {
                    CloseHandle(token);
                }
            }
        }

        private static string BuildEnvironmentBlock(IDictionary<string, string> environment)
        {
            List<KeyValuePair<string, string>> entries = new List<KeyValuePair<string, string>>(environment);
            entries.Sort(delegate(KeyValuePair<string, string> left, KeyValuePair<string, string> right)
            {
                return StringComparer.OrdinalIgnoreCase.Compare(left.Key, right.Key);
            });
            StringBuilder block = new StringBuilder();
            foreach (KeyValuePair<string, string> entry in entries)
            {
                block.Append(entry.Key).Append('=').Append(entry.Value).Append('\0');
            }
            block.Append('\0');
            return block.ToString();
        }

        private static string BuildCommandLine(string executable, string[] arguments)
        {
            StringBuilder commandLine = new StringBuilder(QuoteArgument(executable));
            foreach (string argument in arguments)
            {
                commandLine.Append(' ').Append(QuoteArgument(argument));
            }
            if (commandLine.Length >= 32767)
            {
                throw new ContainmentFailure("command-line-bound");
            }
            return commandLine.ToString();
        }

        private static string BuildArguments(string[] arguments)
        {
            StringBuilder commandLine = new StringBuilder();
            foreach (string argument in arguments)
            {
                if (commandLine.Length != 0)
                {
                    commandLine.Append(' ');
                }
                commandLine.Append(QuoteArgument(argument));
            }
            return commandLine.ToString();
        }

        private static string QuoteArgument(string argument)
        {
            if (argument.Length == 0)
            {
                return "\"\"";
            }
            if (argument.IndexOfAny(new char[] { ' ', '\t', '\"' }) < 0)
            {
                return argument;
            }
            StringBuilder quoted = new StringBuilder();
            quoted.Append('\"');
            int backslashes = 0;
            foreach (char character in argument)
            {
                if (character == '\\')
                {
                    backslashes += 1;
                }
                else if (character == '\"')
                {
                    quoted.Append('\\', backslashes * 2 + 1).Append('\"');
                    backslashes = 0;
                }
                else
                {
                    quoted.Append('\\', backslashes).Append(character);
                    backslashes = 0;
                }
            }
            quoted.Append('\\', backslashes * 2).Append('\"');
            return quoted.ToString();
        }

        private static void RunSelfTest()
        {
            string temporaryBase = Path.GetFullPath(Path.GetTempPath()).TrimEnd(
                Path.DirectorySeparatorChar,
                Path.AltDirectorySeparatorChar);
            string root = Path.Combine(
                temporaryBase,
                "projectatlas-appcontainer-self-test-" + Guid.NewGuid().ToString("N"));
            string readRoot = Path.Combine(root, "read");
            string writeRoot = Path.Combine(root, "write");
            string forbiddenRoot = Path.Combine(root, "forbidden");
            const string selfTestSecretName = "PROJECTATLAS_SELF_TEST_SECRET";
            string originalSelfTestSecret = Environment.GetEnvironmentVariable(selfTestSecretName);
            Directory.CreateDirectory(readRoot);
            Directory.CreateDirectory(writeRoot);
            Directory.CreateDirectory(forbiddenRoot);
            try
            {
                DirectoryInfo protectedDirectory = new DirectoryInfo(readRoot);
                DirectorySecurity protectedSecurity = protectedDirectory.GetAccessControl(
                    AccessControlSections.Access);
                protectedSecurity.SetAccessRuleProtection(true, true);
                protectedDirectory.SetAccessControl(protectedSecurity);
                Environment.SetEnvironmentVariable(selfTestSecretName, "self-test-sensitive-value");
                string sourceExecutable = Process.GetCurrentProcess().MainModule.FileName;
                string canaryExecutable = Path.Combine(readRoot, "projectatlas-appcontainer-canary.exe");
                File.Copy(sourceExecutable, canaryExecutable, false);
                string readMarker = Path.Combine(readRoot, "allowed-read.txt");
                string writeMarker = Path.Combine(writeRoot, "allowed-write.txt");
                string forbiddenMarker = Path.Combine(forbiddenRoot, "forbidden-read.txt");
                string baselineReport = Path.Combine(writeRoot, "baseline-report.txt");
                string containedReport = Path.Combine(writeRoot, "contained-report.txt");
                File.WriteAllText(readMarker, "read-marker");
                File.WriteAllText(forbiddenMarker, "forbidden-marker");
                string quotedArgument = "argument with spaces and \\\"quotes\\\"";
                string dnsResolver = GetDnsResolver();
                AclContract readAclBefore = CaptureAclContract(readRoot, "read-before");
                AclContract writeAclBefore = CaptureAclContract(writeRoot, "write-before");

                string[] baselineArguments = new string[]
                {
                    "canary", "baseline", readMarker, writeMarker, forbiddenMarker, baselineReport,
                    quotedArgument, dnsResolver
                };
                int baselineExit = RunBaseline(canaryExecutable, baselineArguments, writeRoot);
                if (baselineExit != 0 || File.ReadAllText(baselineReport) != BaselineReport())
                {
                    throw new ContainmentFailure(
                        "baseline-canary",
                        "exit=" + baselineExit.ToString(CultureInfo.InvariantCulture));
                }

                File.Delete(writeMarker);
                LaunchConfiguration contained = new LaunchConfiguration();
                contained.Executable = canaryExecutable;
                contained.WorkingDirectory = writeRoot;
                contained.TempDirectory = writeRoot;
                contained.TimeoutSeconds = 30;
                contained.ReadOnlyRoots.Add(readRoot);
                contained.ReadWriteRoots.Add(writeRoot);
                contained.Arguments.AddRange(new string[]
                {
                    "canary", "contained", readMarker, writeMarker, forbiddenMarker, containedReport,
                    quotedArgument, dnsResolver
                });
                int containedExit = LaunchContained(contained);
                if (containedExit != 0
                    || File.ReadAllText(containedReport) != ContainedReport()
                    || File.ReadAllText(writeMarker) != "write-marker")
                {
                    throw new ContainmentFailure(
                        "contained-canary",
                        "exit=" + containedExit.ToString(CultureInfo.InvariantCulture));
                }

                LaunchConfiguration timeout = new LaunchConfiguration();
                timeout.Executable = canaryExecutable;
                timeout.WorkingDirectory = writeRoot;
                timeout.TempDirectory = writeRoot;
                timeout.TimeoutSeconds = 1;
                timeout.ReadOnlyRoots.Add(readRoot);
                timeout.ReadWriteRoots.Add(writeRoot);
                timeout.Arguments.AddRange(new string[] { "canary", "sleep", "5000" });
                if (LaunchContained(timeout) != TimeoutExitCode)
                {
                    throw new ContainmentFailure("timeout-canary");
                }

                string descendantStarted = Path.Combine(writeRoot, "descendant-started.txt");
                string descendantCompleted = Path.Combine(writeRoot, "descendant-completed.txt");
                LaunchConfiguration tree = new LaunchConfiguration();
                tree.Executable = canaryExecutable;
                tree.WorkingDirectory = writeRoot;
                tree.TempDirectory = writeRoot;
                tree.TimeoutSeconds = 10;
                tree.ReadOnlyRoots.Add(readRoot);
                tree.ReadWriteRoots.Add(writeRoot);
                tree.Arguments.AddRange(new string[]
                {
                    "canary", "tree-parent", descendantStarted, descendantCompleted
                });
                if (LaunchContained(tree) != 0)
                {
                    throw new ContainmentFailure("descendant-parent-canary");
                }
                Thread.Sleep(1000);
                if (!File.Exists(descendantStarted)
                    || File.ReadAllText(descendantStarted) != "appcontainer"
                    || File.Exists(descendantCompleted))
                {
                    throw new ContainmentFailure("descendant-cleanup-canary");
                }
                AclContract readAclAfter = CaptureAclContract(readRoot, "read-after");
                AclContract writeAclAfter = CaptureAclContract(writeRoot, "write-after");
                string readDifference = readAclBefore.Difference(readAclAfter);
                string writeDifference = writeAclBefore.Difference(writeAclAfter);
                if (readDifference != null || writeDifference != null)
                {
                    string difference = readDifference == null
                        ? "write-" + writeDifference
                        : "read-" + readDifference;
                    throw new ContainmentFailure("acl-restoration-" + difference);
                }
            }
            finally
            {
                Environment.SetEnvironmentVariable(selfTestSecretName, originalSelfTestSecret);
                string expectedPrefix = temporaryBase + Path.DirectorySeparatorChar + "projectatlas-appcontainer-self-test-";
                if (root.StartsWith(expectedPrefix, StringComparison.OrdinalIgnoreCase) && Directory.Exists(root))
                {
                    Directory.Delete(root, true);
                }
            }
        }

        private static int RunBaseline(string executable, string[] arguments, string workingDirectory)
        {
            ProcessStartInfo start = new ProcessStartInfo();
            start.FileName = executable;
            start.Arguments = BuildArguments(arguments);
            start.WorkingDirectory = workingDirectory;
            start.UseShellExecute = false;
            start.CreateNoWindow = true;
            using (Process process = Process.Start(start))
            {
                if (process == null)
                {
                    throw new ContainmentFailure("start-baseline-canary");
                }
                if (!process.WaitForExit(30000))
                {
                    process.Kill();
                    process.WaitForExit();
                    throw new ContainmentFailure("timeout-baseline-canary");
                }
                return process.ExitCode;
            }
        }

        private static int RunCanary(string[] arguments)
        {
            if (arguments.Length == 3 && arguments[1] == "sleep")
            {
                int milliseconds;
                if (!Int32.TryParse(arguments[2], NumberStyles.None, CultureInfo.InvariantCulture, out milliseconds)
                    || milliseconds < 1
                    || milliseconds > 30000)
                {
                    return 22;
                }
                Thread.Sleep(milliseconds);
                return 0;
            }
            if (arguments.Length == 4 && arguments[1] == "tree-child")
            {
                if (!IsCurrentProcessAppContainer())
                {
                    return 23;
                }
                File.WriteAllText(arguments[2], "appcontainer");
                Thread.Sleep(5000);
                File.WriteAllText(arguments[3], "completed");
                return 0;
            }
            if (arguments.Length == 4 && arguments[1] == "tree-parent")
            {
                ProcessStartInfo childStart = new ProcessStartInfo();
                childStart.FileName = Process.GetCurrentProcess().MainModule.FileName;
                childStart.Arguments = BuildArguments(new string[]
                {
                    "canary", "tree-child", arguments[2], arguments[3]
                });
                childStart.UseShellExecute = false;
                childStart.CreateNoWindow = true;
                Process child = Process.Start(childStart);
                if (child == null)
                {
                    return 24;
                }
                child.Dispose();
                Stopwatch wait = Stopwatch.StartNew();
                while (!File.Exists(arguments[2]) && wait.Elapsed < TimeSpan.FromSeconds(3))
                {
                    Thread.Sleep(25);
                }
                return File.Exists(arguments[2]) && File.ReadAllText(arguments[2]) == "appcontainer" ? 0 : 25;
            }
            if (arguments.Length != 8)
            {
                return 10;
            }
            bool contained = String.Equals(arguments[1], "contained", StringComparison.Ordinal);
            if (!contained && !String.Equals(arguments[1], "baseline", StringComparison.Ordinal))
            {
                return 11;
            }
            if (IsCurrentProcessAppContainer() != contained)
            {
                return 12;
            }
            string inheritedSecret = Environment.GetEnvironmentVariable("PROJECTATLAS_SELF_TEST_SECRET");
            if (contained ? inheritedSecret != null : inheritedSecret != "self-test-sensitive-value")
            {
                return 26;
            }
            if (File.ReadAllText(arguments[2]) != "read-marker")
            {
                return 13;
            }
            File.WriteAllText(arguments[3], "write-marker");
            bool forbiddenRead = CanRead(arguments[4]);
            if (contained ? forbiddenRead : !forbiddenRead)
            {
                return 14;
            }
            bool dnsAvailable = CanSendDnsQuery(arguments[7]);
            bool tcpAvailable = CanConnectTcp();
            bool httpsAvailable = CanRequestHttps();
            if (contained)
            {
                if (dnsAvailable)
                {
                    return 15;
                }
                if (tcpAvailable)
                {
                    return 18;
                }
                if (httpsAvailable)
                {
                    return 19;
                }
            }
            else if (!dnsAvailable)
            {
                return 16;
            }
            else if (!tcpAvailable)
            {
                return 20;
            }
            else if (!httpsAvailable)
            {
                return 21;
            }
            if (arguments[6] != "argument with spaces and \\\"quotes\\\"")
            {
                return 17;
            }
            File.WriteAllText(arguments[5], contained ? ContainedReport() : BaselineReport());
            return 0;
        }

        private static bool IsCurrentProcessAppContainer()
        {
            return ProcessIsAppContainer(Process.GetCurrentProcess().Handle);
        }

        private static bool CanRead(string path)
        {
            try
            {
                File.ReadAllBytes(path);
                return true;
            }
            catch (IOException)
            {
                return false;
            }
            catch (UnauthorizedAccessException)
            {
                return false;
            }
        }

        private static string GetDnsResolver()
        {
            foreach (NetworkInterface network in NetworkInterface.GetAllNetworkInterfaces())
            {
                if (network.OperationalStatus != OperationalStatus.Up)
                {
                    continue;
                }
                foreach (IPAddress address in network.GetIPProperties().DnsAddresses)
                {
                    if (address.AddressFamily == AddressFamily.InterNetwork && !IPAddress.IsLoopback(address))
                    {
                        return address.ToString();
                    }
                }
            }
            throw new ContainmentFailure("dns-resolver-unavailable");
        }

        private static bool CanSendDnsQuery(string resolverAddress)
        {
            byte[] query = new byte[]
            {
                0x50, 0x41, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x07, (byte)'e', (byte)'x', (byte)'a', (byte)'m', (byte)'p', (byte)'l', (byte)'e',
                0x03, (byte)'c', (byte)'o', (byte)'m', 0x00, 0x00, 0x01, 0x00, 0x01
            };
            try
            {
                using (Socket socket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, ProtocolType.Udp))
                {
                    socket.ReceiveTimeout = 3000;
                    EndPoint resolver = new IPEndPoint(IPAddress.Parse(resolverAddress), 53);
                    socket.Connect(resolver);
                    if (socket.Send(query) != query.Length)
                    {
                        return false;
                    }
                    byte[] response = new byte[512];
                    return socket.Receive(response) >= 12;
                }
            }
            catch (SocketException)
            {
                return false;
            }
        }

        private static bool CanConnectTcp()
        {
            using (TcpClient client = new TcpClient(AddressFamily.InterNetwork))
            {
                IAsyncResult connection = client.BeginConnect(IPAddress.Parse("1.1.1.1"), 443, null, null);
                try
                {
                    if (!connection.AsyncWaitHandle.WaitOne(TimeSpan.FromSeconds(5)))
                    {
                        return false;
                    }
                    client.EndConnect(connection);
                    return client.Connected;
                }
                catch (SocketException)
                {
                    return false;
                }
                finally
                {
                    connection.AsyncWaitHandle.Close();
                }
            }
        }

        private static bool CanRequestHttps()
        {
            try
            {
                ServicePointManager.SecurityProtocol = SecurityProtocolType.Tls12;
                HttpWebRequest request = (HttpWebRequest)WebRequest.Create("https://example.com/");
                request.Proxy = null;
                request.Timeout = 10000;
                request.ReadWriteTimeout = 10000;
                using (HttpWebResponse response = (HttpWebResponse)request.GetResponse())
                {
                    int status = (int)response.StatusCode;
                    return status >= 200 && status < 500;
                }
            }
            catch (WebException)
            {
                return false;
            }
        }

        private static string BaselineReport()
        {
            return "identity=normal\nread=allowed\nwrite=allowed\nforbidden=allowed\ndns=allowed\ntcp=allowed\nhttps=allowed\n";
        }

        private static string ContainedReport()
        {
            return "identity=appcontainer\nread=allowed\nwrite=allowed\nforbidden=denied\ndns=denied\ntcp=denied\nhttps=denied\n";
        }

        private static void WriteFailure(string stage, string code)
        {
            if (String.IsNullOrEmpty(code))
            {
                Console.Error.WriteLine("[appcontainer] failed at " + stage);
            }
            else
            {
                Console.Error.WriteLine("[appcontainer] failed at " + stage + " (" + code + ")");
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
            return new ContainmentFailure(stage, "hresult=0x" + result.ToString("x8", CultureInfo.InvariantCulture));
        }
    }
}
'@

function Write-BuildFailure {
    param(
        [Parameter(Mandatory)]
        [string] $Stage
    )

    [Console]::Error.WriteLine("[appcontainer-builder] failed at $Stage")
    exit 125
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
    $mayDeleteOutput = $true
    Add-Type `
        -TypeDefinition $launcherSource `
        -Language CSharp `
        -OutputAssembly $fullOutputPath `
        -OutputType ConsoleApplication `
        -ErrorAction Stop
    if (-not [System.IO.File]::Exists($fullOutputPath)) {
        throw 'missing-output'
    }

    & $fullOutputPath --version | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'version-smoke'
    }
    if ($RunSelfTest) {
        & $fullOutputPath self-test
        if ($LASTEXITCODE -ne 0) {
            throw 'self-test'
        }
    }
    $digest = (Get-FileHash -LiteralPath $fullOutputPath -Algorithm SHA256).Hash.ToLowerInvariant()
    [Console]::Out.WriteLine("[appcontainer-builder] sha256=$digest")
}
catch {
    if ($mayDeleteOutput -and $null -ne $fullOutputPath -and [System.IO.File]::Exists($fullOutputPath)) {
        [System.IO.File]::Delete($fullOutputPath)
    }
    $detail = ($_.Exception.Message -replace '[\r\n]+', ' ').Trim()
    if ($detail.Length -gt 512) {
        $detail = $detail.Substring(0, 512)
    }
    if (-not [string]::IsNullOrWhiteSpace($detail)) {
        [Console]::Error.WriteLine("[appcontainer-builder] detail=$detail")
    }
    Write-BuildFailure -Stage 'compile-or-smoke'
}
