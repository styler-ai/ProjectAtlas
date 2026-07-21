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
        "windows-app-container"
    )]
    [string]$NetworkIsolation,

    [Parameter(Mandatory = $true)]
    [string]$ResolverAddress
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$targetIsolation = @{
    "x86_64-unknown-linux-gnu" = "linux-network-namespace"
    "x86_64-pc-windows-msvc" = "windows-app-container"
}
$sourceRevisionPattern = '\A[0-9a-f]{40}\z'
$sha256Pattern = '\A[0-9a-f]{64}\z'
$rustcReleasePattern = '\A[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?\z'

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
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = $null
    try {
        $process = [System.Diagnostics.Process]::Start($startInfo)
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
        if ($null -ne $process) {
            $process.Dispose()
        }
    }
    if ($commandExitCode -ne 0) {
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

$source = Get-CanonicalDirectory -Path $SourceRoot -Role "SourceRoot"
$inputs = Get-CanonicalDirectory -Path $InputDirectory -Role "InputDirectory"
$output = Get-CanonicalDirectory -Path $OutputDirectory -Role "OutputDirectory"
$script:constructionStatusPath = [System.IO.Path]::Combine(
    $output,
    "construction-status.json"
)
$script:constructionStage = "validate-inputs"
$script:constructionFailureRecorded = $false
trap {
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
    exit 1
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
foreach ($directory in @($buildDirectory, $workingDirectory, $publishDirectory)) {
    if ([System.IO.Directory]::Exists($directory) -or [System.IO.File]::Exists($directory)) {
        throw "Contained construction output already exists: $directory"
    }
    [System.IO.Directory]::CreateDirectory($directory) | Out-Null
}

$env:CARGO_NET_OFFLINE = "true"
$env:CARGO_TARGET_DIR = $buildDirectory
$env:TSLP_OFFLINE = "1"
$env:TSLP_LINK_MODE = "dynamic"

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
        "optional-parser-worker"
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
    $script:constructionStage = "optional-parser-worker-build"
    Write-ConstructionStatus -Stage $script:constructionStage -State "running"
    Invoke-Checked `
        -Executable $cargo `
        -Arguments $workerBuildArguments `
        -Role "optional parser worker build"
    $script:constructionStage = "artifact-assembler-build"
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
            "--example",
            "assemble_optional_parser_artifact"
        ) `
        -Role "parser-pack artifact assembler build"
    $script:constructionStage = "release-verifier-build"
    Write-ConstructionStatus -Stage $script:constructionStage -State "running"
    Invoke-Checked `
        -Executable $cargo `
        -Arguments @(
            "build",
            "--frozen",
            "--offline",
            "--release",
            "--package",
            "projectatlas-cli",
            "--features",
            "optional-parser-supervisor",
            "--example",
            "optional_parser_pack_release"
        ) `
        -Role "parser-pack release verifier build"

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
                "-OutputPath", $brokerOutput,
                "-RunSelfTest"
            ) `
            -Role "runtime-containment broker build and self-test"
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
$archives = @(
    [System.IO.Path]::Combine($workingDirectory, "archive-a.tar.zst"),
    [System.IO.Path]::Combine($workingDirectory, "archive-b.tar.zst")
)
for ($index = 0; $index -lt 2; $index += 1) {
    $assemblyStage = if ($index -eq 0) { "artifact-assembly-a" } else { "artifact-assembly-b" }
    $script:constructionStage = $assemblyStage
    Write-ConstructionStatus -Stage $script:constructionStage -State "running"
    Invoke-Checked `
        -Executable $assembler `
        -Arguments @(
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
        ) `
        -Role "artifact assembly $index"
    $archiveStage = if ($index -eq 0) { "archive-creation-a" } else { "archive-creation-b" }
    $script:constructionStage = $archiveStage
    Write-ConstructionStatus -Stage $script:constructionStage -State "running"
    Invoke-Checked `
        -Executable $releaseTool `
        -Arguments @(
            "create",
            $stagedDirectories[$index],
            $archives[$index]
        ) `
        -Role "deterministic archive creation $index"
}

$script:constructionStage = "deterministic-archive-comparison"
Write-ConstructionStatus -Stage $script:constructionStage -State "running"
$archiveA = Get-Item -LiteralPath $archives[0] -Force
$archiveB = Get-Item -LiteralPath $archives[1] -Force
$archiveAHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archiveA.FullName).Hash.ToLowerInvariant()
$archiveBHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archiveB.FullName).Hash.ToLowerInvariant()
if ($archiveA.Length -ne $archiveB.Length -or $archiveAHash -ne $archiveBHash) {
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
[System.IO.File]::Copy($archiveA.FullName, $publishedArchive, $false)
[System.IO.File]::Copy($releaseTool, $publishedVerifier, $false)
[System.IO.File]::WriteAllText(
    $publishedVerifierDigest,
    ((Get-FileHash -Algorithm SHA256 -LiteralPath $releaseTool).Hash.ToLowerInvariant() + "`n"),
    $utf8WithoutBom
)
[System.IO.File]::Copy($acceptedManifest, $publishedManifest, $false)
[System.IO.File]::Copy($networkCheck, $publishedNetworkCheck, $false)
[System.IO.File]::Delete($script:constructionStatusPath)

[pscustomobject]@{
    target = $Target
    network_isolation = $NetworkIsolation
    archive = [System.IO.Path]::GetFileName($publishedArchive)
    archive_bytes = $archiveA.Length
    archive_sha256 = $archiveAHash
    verifier = [System.IO.Path]::GetFileName($publishedVerifier)
    verifier_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $releaseTool).Hash.ToLowerInvariant()
    accepted_manifest = [System.IO.Path]::GetFileName($publishedManifest)
    independent_assemblies = 2
} | ConvertTo-Json -Compress
