[CmdletBinding()]
param(
    [string]$ProductionWrapper =
        (Join-Path $PSScriptRoot "invoke-parser-pack-windows-construction.ps1"),

    [string]$LauncherPath,

    [hashtable]$ConstructionParameters,

    [string]$RecoveryRoot,

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
        '$zeroProcesses = @(Get-PrincipalProcesses -Sid $sid.Value).Count -eq 0',
        [System.StringComparison]::Ordinal
    )
    $checkpointIndex = $cleanupText.IndexOf(
        'if ($zeroProcesses -and $null -ne $AfterProcessTermination)',
        [System.StringComparison]::Ordinal
    )
    $durableCleanupIndex = $cleanupText.IndexOf(
        'foreach ($path in @($state.acl_paths))',
        [System.StringComparison]::Ordinal
    )
    Require `
        ($zeroProcessIndex -ge 0 -and
            $checkpointIndex -gt $zeroProcessIndex -and
            $durableCleanupIndex -gt $checkpointIndex) `
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

    $breakpointVariableCollisions = @($Ast.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.VariableExpressionAst] -and
                $node.VariablePath.UserPath -like 'breakpoint*'
        },
        $true
    ))
    Require `
        ($breakpointVariableCollisions.Count -eq 0) `
        "Production wrapper collided with recovery breakpoint harness variables."

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

function Get-AccountSidAssignment {
    param(
        [Parameter(Mandatory = $true)]
        [System.Management.Automation.Language.ScriptBlockAst]$Ast
    )

    $assignments = @($Ast.FindAll(
        {
            param($node)
            if ($node -isnot [System.Management.Automation.Language.AssignmentStatementAst] -or
                $node.Left -isnot [System.Management.Automation.Language.VariableExpressionAst] -or
                $node.Left.VariablePath.UserPath -ne 'sid' -or
                $node.Right -isnot [System.Management.Automation.Language.CommandExpressionAst] -or
                $node.Right.Expression -isnot
                    [System.Management.Automation.Language.MemberExpressionAst]) {
                return $false
            }
            $member = $node.Right.Expression
            return `
                (-not $member.Static -and
                    $member.Expression -is
                        [System.Management.Automation.Language.VariableExpressionAst] -and
                    $member.Expression.VariablePath.UserPath -eq 'account' -and
                    $member.Member -is
                        [System.Management.Automation.Language.StringConstantExpressionAst] -and
                    $member.Member.Value -eq 'Sid')
        },
        $true
    ))
    Require `
        ($assignments.Count -eq 1 -and
            $assignments[0].Extent.StartLineNumber -gt 0) `
        "Expected one post-account construction SID assignment."
    return $assignments[0]
}

$productionAst = Get-ProductionWrapperAst
$nativeSourceAssignment = Assert-ProductionRecoveryContracts -Ast $productionAst
$accountSidAssignment = Get-AccountSidAssignment -Ast $productionAst
if ($StaticOnly) {
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

$ProductionWrapper = (Get-Item -LiteralPath $ProductionWrapper -Force).FullName
$LauncherPath = (Get-Item -LiteralPath $LauncherPath -Force).FullName
$RecoveryRoot = [System.IO.Path]::GetFullPath($RecoveryRoot)
$runnerTemp = [System.IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd(
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
    foreach ($argument in $Arguments) {
        $start.ArgumentList.Add($argument)
    }
    $process = [System.Diagnostics.Process]::Start($start)
    if ($null -eq $process) {
        throw "Could not start one bounded recovery process."
    }
    try {
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $process.Kill($true)
            if (-not $process.WaitForExit(5000)) {
                throw "Timed-out recovery process could not be reaped."
            }
            throw "Recovery process exceeded its fixed deadline."
        }
        return $process.ExitCode
    }
    finally {
        if (-not $process.HasExited) {
            $process.Kill($true)
            $process.WaitForExit(5000) | Out-Null
        }
        $process.Dispose()
    }
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

        [scriptblock]$AfterProcessTermination
    )

    if ($null -eq $AfterProcessTermination) {
        Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
            Invoke-Cleanup
        }
        return
    }
    Invoke-WithCleanupDefinitions -StatePath $StatePath -Operation {
        Invoke-Cleanup -AfterProcessTermination $AfterProcessTermination
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
            ($null -eq $Sid -or ($null -ne $_.Sid -and $_.Sid.Value -eq $Sid))
    })
    Require ($matches.Count -le 1) "Recovery identity resolved to multiple local accounts."
    if ($matches.Count -eq 0) {
        return $null
    }
    return $matches[0]
}

function Get-ExactSidProcesses {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Sid
    )

    $matches = [System.Collections.Generic.List[object]]::new()
    foreach ($process in @(Get-CimInstance -ClassName Win32_Process -ErrorAction Stop)) {
        $owner = Invoke-CimMethod -InputObject $process -MethodName GetOwnerSid -ErrorAction Stop
        if ($owner.ReturnValue -eq 0 -and [string]$owner.Sid -eq $Sid) {
            $matches.Add($process)
        }
    }
    return @($matches)
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
    Require `
        (@(Get-ExactSidProcesses -Sid $Sid).Count -eq 0) `
        "Recovery cleanup left an exact-SID process."
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
    $breakpointHarnessPath = Join-Path $scenarioDirectory 'hold-before-sid-assignment.ps1'
    $readyMarkerPath = Join-Path $scenarioDirectory 'account-created.ready'
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
            ready_marker = $readyMarkerPath
            breakpoint_line = $accountSidAssignment.Extent.StartLineNumber
            expected_description = $accountDescription
            placeholder_sid = $placeholderSid
        } | ConvertTo-Json -Depth 8 -Compress),
        [System.Text.UTF8Encoding]::new($false)
    )
    $breakpointHarnessSource = @'
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
$breakpointLine = [int]$payload.breakpoint_line
$breakpointReadyMarkerPath = [System.IO.Path]::GetFullPath([string]$payload.ready_marker)
$breakpointExpectedDescription = [string]$payload.expected_description
$breakpointPlaceholderSid = [string]$payload.placeholder_sid
$parameterDirectory = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::GetDirectoryName($ParameterPath)
)
if ([System.IO.Path]::GetDirectoryName($breakpointReadyMarkerPath) -ne $parameterDirectory -or
    [System.IO.File]::Exists($breakpointReadyMarkerPath) -or
    $breakpointExpectedDescription -ne "ProjectAtlas optional parser pack construction" -or
    $breakpointPlaceholderSid -ne "S-1-5-21-0-0-0-0") {
    throw "Account-ready marker path is unsafe."
}
$breakpointStatePath = [System.IO.Path]::GetFullPath([string]$parameters.StatePath)
$breakpointAction = {
    $stateItem = Get-Item -LiteralPath $breakpointStatePath -Force
    if (($stateItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
        $stateItem.Length -le 0) {
        throw "Protected construction journal was not ready for the account marker."
    }
    $breakpointJournal = [System.IO.File]::ReadAllText($stateItem.FullName) |
        ConvertFrom-Json -Depth 6
    if ($null -eq $account -or
        [string]$account.Name -cne [string]$breakpointJournal.username -or
        $null -eq $account.Sid -or
        $account.Sid.Value -notmatch '\AS-1-5-21-[0-9]+-[0-9]+-[0-9]+-[0-9]+\z' -or
        [string]$account.Description -ne $breakpointExpectedDescription -or
        [string]$breakpointJournal.sid -ne $breakpointPlaceholderSid -or
        [string]$breakpointJournal.stage -ne 'identity') {
        throw "Completed construction account did not match its placeholder journal."
    }

    $markerTemporaryPath = "$breakpointReadyMarkerPath.$([Guid]::NewGuid().ToString('N')).tmp"
    try {
        [System.IO.File]::WriteAllText(
            $markerTemporaryPath,
            "projectatlas-account-created-ready-v1`n",
            [System.Text.UTF8Encoding]::new($false)
        )
        Set-Acl `
            -LiteralPath $markerTemporaryPath `
            -AclObject (Get-Acl -LiteralPath $breakpointStatePath)
        if (-not (Get-Acl -LiteralPath $markerTemporaryPath).AreAccessRulesProtected) {
            throw "Account-ready marker ACL was not protected."
        }
        [System.IO.File]::Move($markerTemporaryPath, $breakpointReadyMarkerPath)
    }
    finally {
        if ([System.IO.File]::Exists($markerTemporaryPath)) {
            Remove-Item -LiteralPath $markerTemporaryPath -Force
        }
    }

    $sidAssignmentGate = [System.Threading.ManualResetEventSlim]::new($false)
    try {
        $sidAssignmentGate.Wait()
    }
    finally {
        $sidAssignmentGate.Dispose()
    }
    throw "Construction SID-assignment breakpoint returned unexpectedly."
}
$breakpoint = Set-PSBreakpoint `
    -Script $wrapperPath `
    -Line $breakpointLine `
    -Action $breakpointAction
if ($null -eq $breakpoint -or
    -not $breakpoint.Enabled -or
    [System.IO.Path]::GetFullPath([string]$breakpoint.Script) -ne $wrapperPath -or
    $breakpoint.Line -ne $breakpointLine -or
    @(Get-PSBreakpoint | Where-Object { $_.Id -eq $breakpoint.Id }).Count -ne 1) {
    throw "Construction SID-assignment breakpoint was not installed exactly once."
}
& $wrapperPath @parameters
throw "Construction wrapper returned after the SID-assignment breakpoint."
'@
    [System.IO.File]::WriteAllText(
        $breakpointHarnessPath,
        $breakpointHarnessSource,
        [System.Text.UTF8Encoding]::new($false)
    )

    $wrapperStart = [System.Diagnostics.ProcessStartInfo]::new()
    $wrapperStart.FileName = [string]$ConstructionParameters.PwshPath
    $wrapperStart.UseShellExecute = $false
    $wrapperStart.CreateNoWindow = $true
    foreach ($argument in @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
        '-File', $breakpointHarnessPath,
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
                break
            }
            Start-Sleep -Milliseconds 250
        } while ([DateTime]::UtcNow -lt $readyDeadline)
        $preKillObservation = @(
            "marker=$markerObserved"
            "marker_acl=$markerValidated"
            "journal=$($null -ne $observedPlaceholderState)"
            "process_alive=$(-not $wrapperProcess.HasExited)"
        ) -join ','
        Require `
            ($markerValidated -and
                $null -ne $observedPlaceholderState -and
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

        $accountVisibilityDeadline = [DateTime]::UtcNow.AddSeconds(10)
        do {
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
            Start-Sleep -Milliseconds 250
        } while ([DateTime]::UtcNow -lt $accountVisibilityDeadline)
        $postKillObservation = @(
            "account=$accountObserved"
            "account_sid=$accountSidValidated"
            "account_description=$accountDescriptionValidated"
            "process_absent=$wrapperProcessAbsent"
        ) -join ','
        Require `
            ($null -ne $observedAccount -and
                $accountSidValidated -and
                $accountDescriptionValidated -and
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
            [string]$boundState.stage -eq 'identity' -and
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
        [object]$Receipt
    )

    $processId = [int](Get-ReflectedReceiptValue `
        -ReceiptType $ReceiptType `
        -Receipt $Receipt `
        -Name ProcessId)
    Require `
        ($processId -gt 0 -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name TerminationAttempted) -and
            [uint32](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name WaitResult) -eq 0 -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name Reaped) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name JobHandleOwned) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name JobHandleClosed) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ProcessHandleOwned) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ProcessHandleClosed) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ThreadHandleOwned) -and
            [bool](Get-ReflectedReceiptValue -ReceiptType $ReceiptType -Receipt $Receipt -Name ThreadHandleClosed)) `
        "Construction admission recovery receipt was incomplete."
    Require `
        ($null -eq (Get-Process -Id $processId -ErrorAction SilentlyContinue)) `
        "Construction admission recovery left its exact PID alive."
}

function New-MinimalUserEnvironmentBlock {
    $values = [ordered]@{
        ComSpec = $env:ComSpec
        OS = 'Windows_NT'
        PATH = "$(Split-Path -Parent ([string]$ConstructionParameters.PwshPath));$env:SystemRoot\System32"
        PATHEXT = $env:PATHEXT
        SystemRoot = $env:SystemRoot
        TEMP = (Join-Path $env:SystemRoot 'Temp')
        TMP = (Join-Path $env:SystemRoot 'Temp')
        WINDIR = $env:WINDIR
    }
    return (($values.GetEnumerator() | ForEach-Object {
        "$($_.Key)=$($_.Value)"
    }) -join "`0") + "`0`0"
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
            'Normal,FailBeforeJobAssignment,FailBeforeJobAssignmentAndCleanupFailure') `
        "Construction adapter recovery scenario domain changed."

    $environmentBlock = New-MinimalUserEnvironmentBlock
    $arguments = [string[]]@(
        '-NoLogo', '-NoProfile', '-NonInteractive',
        '-Command', 'Start-Sleep -Seconds 30'
    )
    foreach ($scenarioName in @(
        'FailBeforeJobAssignment',
        'FailBeforeJobAssignmentAndCleanupFailure'
    )) {
        $admissionScenario = [enum]::Parse($scenarioType, $scenarioName)
        $receipt = [Activator]::CreateInstance($receiptType, $true)
        $invokeArguments = [object[]]::new(10)
        $invokeArguments[0] = $identity.Username
        $invokeArguments[1] = $identity.Sid
        $invokeArguments[2] = $identity.Password
        $invokeArguments[3] = [string]$ConstructionParameters.PwshPath
        $invokeArguments[4] = $arguments
        $invokeArguments[5] = $env:SystemRoot
        $invokeArguments[6] = $environmentBlock
        $invokeArguments[7] = 30
        $invokeArguments[8] = $admissionScenario
        $invokeArguments[9] = $receipt
        $failure = Get-ReflectedOperationFailure `
            -Method $runCore `
            -Arguments $invokeArguments
        if ($scenarioName -eq 'FailBeforeJobAssignment') {
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
            @(Get-ExactSidProcesses -Sid $identity.Sid | Where-Object {
                [int]$_.ProcessId -eq $launchReceipt.ProcessId
            }).Count -eq 1 -and
                @(Get-ExactProfile -Sid $identity.Sid).Count -eq 1
        }

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
        (@(Get-ExactSidProcesses -Sid $identity.Sid).Count -eq 0 -and
            $null -eq (Get-Process -Id $launchReceipt.ProcessId -ErrorAction SilentlyContinue)) `
        "Cleanup retry checkpoint left its exact process alive."
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
