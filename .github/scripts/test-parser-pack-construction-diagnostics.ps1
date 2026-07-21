[CmdletBinding()]
param(
    [string]$ProductionScript = (Join-Path $PSScriptRoot "run-parser-pack-contained-construction.ps1"),
    [string]$WindowsWrapper = (Join-Path $PSScriptRoot "invoke-parser-pack-windows-construction.ps1")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

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

$production = Get-Item -LiteralPath $ProductionScript -Force
Require `
    (-not $production.PSIsContainer -and
        (($production.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0)) `
    "Production construction script is not one regular file."
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
    $production.FullName,
    [ref]$tokens,
    [ref]$parseErrors
)
Require ($parseErrors.Count -eq 0) "Production construction script did not parse."
foreach ($name in @(
    "Add-BoundedDiagnosticTail",
    "Invoke-Checked",
    "Write-ConstructionStatus"
)) {
    $definitions = @($ast.FindAll(
        {
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq $name
        },
        $true
    ))
    Require ($definitions.Count -eq 1) "Expected one $name definition."
    Invoke-Expression $definitions[0].Extent.Text
}

$testBase = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::Combine(
        [System.IO.Path]::GetTempPath(),
        "projectatlas-construction-diagnostic-tests"
    )
)
[System.IO.Directory]::CreateDirectory($testBase) | Out-Null
$testRoot = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::Combine($testBase, [guid]::NewGuid().ToString("N"))
)
Require `
    ($testRoot.StartsWith(
        "$($testBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar))$([System.IO.Path]::DirectorySeparatorChar)",
        [System.StringComparison]::OrdinalIgnoreCase
    )) `
    "Diagnostic test root escaped its temporary base."

try {
    $source = [System.IO.Directory]::CreateDirectory(
        [System.IO.Path]::Combine($testRoot, "source")
    ).FullName
    $inputs = [System.IO.Directory]::CreateDirectory(
        [System.IO.Path]::Combine($testRoot, "inputs")
    ).FullName
    $output = [System.IO.Directory]::CreateDirectory(
        [System.IO.Path]::Combine($testRoot, "output")
    ).FullName
    if ($env:OS -eq "Windows_NT") {
        $wrapperTokens = $null
        $wrapperParseErrors = $null
        $wrapperAst = [System.Management.Automation.Language.Parser]::ParseFile(
            (Get-Item -LiteralPath $WindowsWrapper -Force).FullName,
            [ref]$wrapperTokens,
            [ref]$wrapperParseErrors
        )
        Require ($wrapperParseErrors.Count -eq 0) "Windows construction wrapper did not parse."
        $nativeSourceAssignments = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$nativeSource'
            },
            $true
        ))
        Require ($nativeSourceAssignments.Count -eq 1) "Expected one native adapter source assignment."
        Invoke-Expression $nativeSourceAssignments[0].Extent.Text
        Require `
            (-not $nativeSource.Contains('LogonWithProfile') -and
                $nativeSource -match 'passwordPointer,\s*0,\s*executable,') `
            "Windows construction adapter did not retain zero process logon flags."
        if (-not ('ProjectAtlasConstructionProcess' -as [type])) {
            Add-Type -TypeDefinition $nativeSource -Language CSharp
        }

        $tokenRestrictionSourceAssignments = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$tokenRestrictionProbeSource'
            },
            $true
        ))
        Require `
            ($tokenRestrictionSourceAssignments.Count -eq 1) `
            "Expected one current-process token restriction probe source assignment."
        Invoke-Expression $tokenRestrictionSourceAssignments[0].Extent.Text
        Require `
            ($tokenRestrictionProbeSource.Contains('GetCurrentProcess()') -and
                $tokenRestrictionProbeSource.Contains('private const uint TokenQuery = 0x00000008;') -and
                $tokenRestrictionProbeSource.Contains('OpenProcessToken(GetCurrentProcess(), TokenQuery') -and
                $tokenRestrictionProbeSource.Contains('IsTokenRestricted(tokenHandle)') -and
                $tokenRestrictionProbeSource.Contains('using (tokenHandle)') -and
                $tokenRestrictionProbeSource.Contains('SetLastError(ErrorSuccess)')) `
            "Current-process token restriction probe did not retain query-only evaluation and cleanup."
        foreach ($forbiddenTokenProbeText in @(
            'TokenDuplicate',
            'TOKEN_DUPLICATE',
            'DuplicateToken',
            'AdjustTokenPrivileges',
            'SeDebugPrivilege',
            'ImpersonateLoggedOnUser',
            'AccessCheck',
            'OpenSemaphore',
            'MaximumAllowed',
            'MAXIMUM_ALLOWED',
            'TOKEN_ALL_ACCESS',
            'TokenAllAccess'
        )) {
            Require `
                (-not $tokenRestrictionProbeSource.Contains($forbiddenTokenProbeText)) `
                "Current-process token restriction probe retained forbidden capability text."
        }
        foreach ($removedParentAccessCheckText in @(
            'ProjectAtlasJobserverSynchronizeAccessCheckResult',
            'EvaluateJobserverSynchronizeAccess',
            'LastJobserverSynchronizeAccessCheck',
            'DuplicateToken',
            'AccessCheck'
        )) {
            Require `
                (-not $nativeSource.Contains($removedParentAccessCheckText)) `
                "Windows construction adapter retained dead parent-side authorization machinery."
        }
        if (-not ('ProjectAtlasCurrentProcessTokenRestrictionProbe' -as [type])) {
            Add-Type -TypeDefinition $tokenRestrictionProbeSource -Language CSharp
        }
        Require `
            (([enum]::GetNames([ProjectAtlasCurrentProcessTokenRestrictionResult]) -join ',') -eq
                'Unrestricted,Restricted,TokenUnavailable,EvaluationUnavailable') `
            "Current-process token restriction result domain was not closed."
        $restrictionResult =
            [ProjectAtlasCurrentProcessTokenRestrictionProbe]::ProbeCurrentProcessTokenRestriction()
        Require `
            ($restrictionResult -in @(
                [ProjectAtlasCurrentProcessTokenRestrictionResult]::Unrestricted,
                [ProjectAtlasCurrentProcessTokenRestrictionResult]::Restricted
            )) `
            "Current process token restriction could not be evaluated."
        $restrictionProcess = [System.Diagnostics.Process]::GetCurrentProcess()
        try {
            [ProjectAtlasCurrentProcessTokenRestrictionProbe]::ProbeCurrentProcessTokenRestriction() |
                Out-Null
            $restrictionHandleCountBefore = $restrictionProcess.HandleCount
            foreach ($restrictionIteration in 1..32) {
                Require `
                    ([ProjectAtlasCurrentProcessTokenRestrictionProbe]::ProbeCurrentProcessTokenRestriction() -eq
                        $restrictionResult) `
                    "Repeated current-process token restriction result changed."
            }
            $restrictionHandleCountAfter = $restrictionProcess.HandleCount
            Require `
                ($restrictionHandleCountAfter -eq $restrictionHandleCountBefore) `
                "Current-process token restriction probe leaked a native handle."
        }
        finally {
            $restrictionProcess.Dispose()
        }
        $restrictionClassifier =
            [ProjectAtlasCurrentProcessTokenRestrictionProbe].GetMethod(
                'ClassifyResult',
                [System.Reflection.BindingFlags]'NonPublic,Static'
            )
        Require ($null -ne $restrictionClassifier) "Token restriction classifier was missing."
        foreach ($restrictionCase in @(
            @{ opened = $true; restricted = $true; error = 5; expected = 'Restricted' },
            @{ opened = $true; restricted = $false; error = 0; expected = 'Unrestricted' },
            @{ opened = $true; restricted = $false; error = 5; expected = 'EvaluationUnavailable' },
            @{ opened = $false; restricted = $false; error = 0; expected = 'TokenUnavailable' }
        )) {
            $classifiedRestriction = $restrictionClassifier.Invoke(
                $null,
                [object[]]@(
                    [bool]$restrictionCase.opened,
                    [bool]$restrictionCase.restricted,
                    [int]$restrictionCase.error
                )
            )
            Require `
                ([string]$classifiedRestriction -eq $restrictionCase.expected) `
                "Token restriction classifier returned the wrong closed result."
        }

        $objectDirectorySourceAssignments = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$objectDirectoryProbeSource'
            },
            $true
        ))
        Require `
            ($objectDirectorySourceAssignments.Count -eq 1) `
            "Expected one object directory probe source assignment."
        Invoke-Expression $objectDirectorySourceAssignments[0].Extent.Text
        Require `
            ($objectDirectoryProbeSource.Contains(
                'private const string BaseNamedObjectsPath = @"\BaseNamedObjects";'
            ) -and
                [regex]::Matches(
                    $objectDirectoryProbeSource,
                    [regex]::Escape('\BaseNamedObjects')
                ).Count -eq 1 -and
                $objectDirectoryProbeSource.Contains(
                    'private const uint DirectoryTraverse = 0x00000002;'
                )) `
            "Object directory probe did not retain its exact path and traverse-only access."
        Require `
            ($objectDirectoryProbeSource.Contains('NativeLibrary.TryLoad(') -and
                $objectDirectoryProbeSource.Contains('NativeLibrary.TryGetExport(') -and
                $objectDirectoryProbeSource.Contains('NativeLibrary.Free(') -and
                $objectDirectoryProbeSource.Contains('CloseHandle(directoryHandle)') -and
                [regex]::Matches(
                    $objectDirectoryProbeSource,
                    'Marshal\.FreeHGlobal\('
                ).Count -eq 2) `
            "Object directory probe did not retain dynamic loading and bounded cleanup."
        foreach ($forbiddenProbeText in @(
            'NtQueryDirectoryObject',
            'DirectoryQuery',
            'DirectoryCreateObject',
            'DirectoryCreateSubdirectory',
            'DirectoryAllAccess',
            'GetTokenInformation',
            'NtQueryInformationToken',
            'OpenSemaphore',
            'SemaphoreAcl',
            '\Sessions\',
            'Console.Write',
            'status.ToString',
            'String.Format'
        )) {
            Require `
                (-not $objectDirectoryProbeSource.Contains($forbiddenProbeText)) `
                "Object directory probe retained forbidden capability text."
        }
        if (-not ('ProjectAtlasObjectDirectoryProbe' -as [type])) {
            Add-Type -TypeDefinition $objectDirectoryProbeSource -Language CSharp
        }
        Require `
            (([enum]::GetNames([ProjectAtlasObjectDirectoryProbeResult]) -join ',') -eq
                'Accessible,AccessDenied,NotFound,Unavailable,Unexpected') `
            "Object directory probe result domain was not closed."
        Require `
            ([ProjectAtlasObjectDirectoryProbe]::ProbeGlobalBaseNamedObjects() -eq
                [ProjectAtlasObjectDirectoryProbeResult]::Accessible) `
            "Current Windows principal could not traverse the global object directory."
        $probeProcess = [System.Diagnostics.Process]::GetCurrentProcess()
        [ProjectAtlasObjectDirectoryProbe]::ProbeGlobalBaseNamedObjects() | Out-Null
        $handleCountBefore = $probeProcess.HandleCount
        foreach ($probeIteration in 1..32) {
            Require `
                ([ProjectAtlasObjectDirectoryProbe]::ProbeGlobalBaseNamedObjects() -eq
                    [ProjectAtlasObjectDirectoryProbeResult]::Accessible) `
                "Repeated object directory probe did not remain accessible."
        }
        $handleCountAfter = $probeProcess.HandleCount
        Require `
            ($handleCountAfter -eq $handleCountBefore) `
            "Object directory probe leaked a process handle."
        $classifier = [ProjectAtlasObjectDirectoryProbe].GetMethod(
            'ClassifyStatus',
            [System.Reflection.BindingFlags]'NonPublic,Static'
        )
        Require ($null -ne $classifier) "Object directory probe classifier was missing."
        $statusAccessDenied = [System.BitConverter]::ToInt32(
            [System.BitConverter]::GetBytes([Convert]::ToUInt32('C0000022', 16)),
            0
        )
        $statusObjectNameNotFound = [System.BitConverter]::ToInt32(
            [System.BitConverter]::GetBytes([Convert]::ToUInt32('C0000034', 16)),
            0
        )
        $statusObjectPathNotFound = [System.BitConverter]::ToInt32(
            [System.BitConverter]::GetBytes([Convert]::ToUInt32('C000003A', 16)),
            0
        )
        foreach ($classifierCase in @(
            @{ status = 0; handle = [IntPtr]::new(1); expected = 'Accessible' },
            @{ status = $statusAccessDenied; handle = [IntPtr]::Zero; expected = 'AccessDenied' },
            @{ status = $statusObjectNameNotFound; handle = [IntPtr]::Zero; expected = 'NotFound' },
            @{ status = $statusObjectPathNotFound; handle = [IntPtr]::Zero; expected = 'NotFound' },
            @{ status = 0; handle = [IntPtr]::Zero; expected = 'Unexpected' },
            @{ status = 1; handle = [IntPtr]::Zero; expected = 'Unexpected' }
        )) {
            $classified = $classifier.Invoke(
                $null,
                [object[]]@([int]$classifierCase.status, [IntPtr]$classifierCase.handle)
            )
            Require `
                ([string]$classified -eq $classifierCase.expected) `
                "Object directory probe classifier returned the wrong closed result."
        }

        $probeSourceAssignments = @($wrapperAst.FindAll(
            {
                param($node)
                $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
                    $node.Left.Extent.Text -eq '$probeSource'
            },
            $true
        ))
        Require ($probeSourceAssignments.Count -eq 1) "Expected one boundary probe source assignment."
        Invoke-Expression $probeSourceAssignments[0].Extent.Text
        Require `
            ($probeSource.Contains('ReadToEndAsync()') -and
                $probeSource.Contains('$expectedCargoMakeflags') -and
                -not $probeSource.Contains('StandardOutput.ReadToEnd()')) `
            "Construction boundary probe did not drain both streams asynchronously or verify jobserver transport."
        Require `
            ($probeSource.Contains('$ExpectedSessionId') -and
                $probeSource.Contains('S-1-16-8192') -and
                $probeSource.Contains('$principal.IsInRole($expectedSecurityIdentifier)') -and
                $probeSource.Contains('$synchronizeJobserver = [System.Threading.SemaphoreAcl]::OpenExisting(') -and
                $probeSource.Contains('$modifyJobserver = [System.Threading.SemaphoreAcl]::OpenExisting(')) `
            "Construction boundary probe did not classify SID membership, session, integrity, and individual jobserver rights."
        Require `
            ($wrapperAst.Extent.Text.Contains('43 { "jobserver-synchronize-open-denied-restricted-token" }') -and
                $wrapperAst.Extent.Text.Contains('44 { "jobserver-synchronize-open-denied-unrestricted-token" }') -and
                $wrapperAst.Extent.Text.Contains('45 { "target-token-query-unavailable" }') -and
                $wrapperAst.Extent.Text.Contains('46 { "target-token-restriction-evaluation-unavailable" }') -and
                $wrapperAst.Extent.Text.Contains('28 { "jobserver-modify-access" }') -and
                $wrapperAst.Extent.Text.Contains('33 { "jobserver-combined-access" }') -and
                $wrapperAst.Extent.Text.Contains('37 { "target-sid-membership-query" }') -and
                $wrapperAst.Extent.Text.Contains('38 { "target-sid-not-effective" }') -and
                $wrapperAst.Extent.Text.Contains('39 { "global-object-directory-traverse-access" }') -and
                $wrapperAst.Extent.Text.Contains('40 { "global-object-directory-traverse-missing" }') -and
                $wrapperAst.Extent.Text.Contains('41 { "global-object-directory-probe-unavailable" }') -and
                $wrapperAst.Extent.Text.Contains('42 { "global-object-directory-traverse-open" }')) `
            "Construction boundary probe did not retain distinct jobserver access diagnostics."
        $wrapperText = $wrapperAst.Extent.Text
        $parentProbeIndex = $wrapperText.IndexOf(
            '[ProjectAtlasObjectDirectoryProbe]::ProbeGlobalBaseNamedObjects()',
            [System.StringComparison]::Ordinal
        )
        $accountCreationIndex = $wrapperText.IndexOf(
            '$account = New-LocalUser',
            [System.StringComparison]::Ordinal
        )
        Require `
            ($parentProbeIndex -ge 0 -and
                $accountCreationIndex -gt $parentProbeIndex -and
                $wrapperText.Contains('"parent-global-object-directory-traverse-access"') -and
                $wrapperText.Contains('"parent-global-object-directory-traverse-missing"') -and
                $wrapperText.Contains('"parent-global-object-directory-probe-unavailable"') -and
                $wrapperText.Contains('"parent-global-object-directory-traverse-open"') -and
                $wrapperText.Contains('-Role "object directory boundary probe"')) `
            "Parent object directory probe was not fail-closed before account creation."
        $childProbeIndex = $probeSource.IndexOf(
            '[ProjectAtlasObjectDirectoryProbe]::ProbeGlobalBaseNamedObjects()',
            [System.StringComparison]::Ordinal
        )
        $firstJobserverOpenIndex = $probeSource.IndexOf(
            '[System.Threading.SemaphoreAcl]::OpenExisting(',
            [System.StringComparison]::Ordinal
        )
        $childTokenRestrictionProbeIndex = $probeSource.IndexOf(
            '[ProjectAtlasCurrentProcessTokenRestrictionProbe]::ProbeCurrentProcessTokenRestriction()',
            [System.StringComparison]::Ordinal
        )
        Require `
            ($probeSource.Contains('Add-Type -Path $objectDirectoryProbe.FullName') -and
                $probeSource.Contains('Add-Type -Path $tokenRestrictionProbe.FullName') -and
                $childProbeIndex -ge 0 -and
                $childTokenRestrictionProbeIndex -gt $childProbeIndex -and
                $firstJobserverOpenIndex -gt $childTokenRestrictionProbeIndex -and
                $probeSource.Contains('catch [System.UnauthorizedAccessException] {') -and
                $probeSource.Contains('"Restricted" { exit 43 }') -and
                $probeSource.Contains('"Unrestricted" { exit 44 }') -and
                $probeSource.Contains('"TokenUnavailable" { exit 45 }')) `
            "Child object-directory and token-restriction probes did not precede or exclusively classify denied jobserver access."

        $currentIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
        $currentPrincipal = [System.Security.Principal.WindowsPrincipal]::new($currentIdentity)
        Require `
            $currentPrincipal.IsInRole($currentIdentity.User) `
            "Current user SID was not effective in its access token."
        Require `
            (-not $currentPrincipal.IsInRole(
                [System.Security.Principal.SecurityIdentifier]::new('S-1-0-0')
            )) `
            "Null SID unexpectedly participated in the access token."

        foreach ($cleanupFunctionName in @(
            "Remove-PrincipalAcl",
            "Assert-PrincipalAclAbsent"
        )) {
            $cleanupDefinitions = @($wrapperAst.FindAll(
                {
                    param($node)
                    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                        $node.Name -eq $cleanupFunctionName
                },
                $true
            ))
            Require ($cleanupDefinitions.Count -eq 1) "Expected one $cleanupFunctionName definition."
            Invoke-Expression $cleanupDefinitions[0].Extent.Text
        }
        Require `
            ($wrapperAst.Extent.Text.Contains('$acl.PurgeAccessRules($Sid)') -and
                $wrapperAst.Extent.Text -match 'GetAccessRules\(\s*\$true,\s*\$true,' -and
                -not $wrapperAst.Extent.Text.Contains('$acl.RemoveAccessRuleSpecific($rule)')) `
            "Construction ACL cleanup did not retain platform purge plus effective post-verification."

        $aclFixture = [System.IO.Path]::Combine($testRoot, "acl-fixture")
        [System.IO.Directory]::CreateDirectory($aclFixture) | Out-Null
        $aclChildFixture = [System.IO.Path]::Combine($aclFixture, "child.txt")
        [System.IO.File]::WriteAllText($aclChildFixture, "fixture")
        $fixtureSid = [System.Security.Principal.SecurityIdentifier]::new(
            "S-1-5-21-3141592653-2718281828-1618033988-424242"
        )
        $unrelatedFixtureSid = [System.Security.Principal.SecurityIdentifier]::new(
            "S-1-5-21-3141592653-2718281828-1618033988-424243"
        )
        $fixtureAcl = Get-Acl -LiteralPath $aclFixture
        $fixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $fixtureSid,
            [System.Security.AccessControl.FileSystemRights]::Write,
            [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
                [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [System.Security.AccessControl.PropagationFlags]::None,
            [System.Security.AccessControl.AccessControlType]::Deny
        ))
        $fixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $fixtureSid,
            [System.Security.AccessControl.FileSystemRights]::ReadAndExecute,
            [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
                [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [System.Security.AccessControl.PropagationFlags]::None,
            [System.Security.AccessControl.AccessControlType]::Allow
        ))
        Set-Acl -LiteralPath $aclFixture -AclObject $fixtureAcl
        $childFixtureAcl = Get-Acl -LiteralPath $aclChildFixture
        $childFixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $fixtureSid,
            [System.Security.AccessControl.FileSystemRights]::WriteData,
            [System.Security.AccessControl.AccessControlType]::Deny
        ))
        $childFixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $fixtureSid,
            [System.Security.AccessControl.FileSystemRights]::Read,
            [System.Security.AccessControl.AccessControlType]::Allow
        ))
        $childFixtureAcl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
            $unrelatedFixtureSid,
            [System.Security.AccessControl.FileSystemRights]::Read,
            [System.Security.AccessControl.AccessControlType]::Allow
        ))
        Set-Acl -LiteralPath $aclChildFixture -AclObject $childFixtureAcl
        $unrelatedRulesBefore = @(
            (Get-Acl -LiteralPath $aclChildFixture).GetAccessRules(
                $true,
                $true,
                [System.Security.Principal.SecurityIdentifier]
            ) | Where-Object { $_.IdentityReference -eq $unrelatedFixtureSid }
        )
        Require `
            ($unrelatedRulesBefore.Count -eq 1) `
            "Construction ACL fixture did not retain one unrelated principal rule."
        $unrelatedRuleBefore = $unrelatedRulesBefore[0]
        Require `
            (@(
                (Get-Acl -LiteralPath $aclChildFixture).GetAccessRules(
                    $true,
                    $true,
                    [System.Security.Principal.SecurityIdentifier]
                ) | Where-Object { $_.IdentityReference -eq $fixtureSid }
            ).Count -ge 4) `
            "Construction ACL fixture did not contain explicit and inherited target rules."

        Remove-PrincipalAcl -Path $aclChildFixture -Sid $fixtureSid
        Remove-PrincipalAcl -Path $aclFixture -Sid $fixtureSid
        Assert-PrincipalAclAbsent -Path $aclFixture -Sid $fixtureSid
        Assert-PrincipalAclAbsent -Path $aclChildFixture -Sid $fixtureSid
        $unrelatedRules = @(
            (Get-Acl -LiteralPath $aclChildFixture).GetAccessRules(
                $true,
                $true,
                [System.Security.Principal.SecurityIdentifier]
            ) | Where-Object { $_.IdentityReference -eq $unrelatedFixtureSid }
        )
        Require `
            ($unrelatedRules.Count -eq 1 -and
                $unrelatedRules[0].IdentityReference -eq
                    $unrelatedRuleBefore.IdentityReference -and
                $unrelatedRules[0].FileSystemRights -eq
                    $unrelatedRuleBefore.FileSystemRights -and
                $unrelatedRules[0].AccessControlType -eq
                    $unrelatedRuleBefore.AccessControlType -and
                $unrelatedRules[0].IsInherited -eq
                    $unrelatedRuleBefore.IsInherited -and
                $unrelatedRules[0].InheritanceFlags -eq
                    $unrelatedRuleBefore.InheritanceFlags -and
                $unrelatedRules[0].PropagationFlags -eq
                    $unrelatedRuleBefore.PropagationFlags) `
            "Construction ACL cleanup removed an unrelated principal."
        Remove-PrincipalAcl -Path $aclChildFixture -Sid $fixtureSid
        Remove-PrincipalAcl -Path $aclFixture -Sid $fixtureSid
        Assert-PrincipalAclAbsent -Path $aclFixture -Sid $fixtureSid
        Assert-PrincipalAclAbsent -Path $aclChildFixture -Sid $fixtureSid

        foreach ($functionName in @(
            "New-ConstructionJobserverSecurity",
            "New-ConstructionJobserver"
        )) {
            $jobserverDefinitions = @($wrapperAst.FindAll(
                {
                    param($node)
                    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                        $node.Name -eq $functionName
                },
                $true
            ))
            Require ($jobserverDefinitions.Count -eq 1) "Expected one $functionName definition."
            Invoke-Expression $jobserverDefinitions[0].Extent.Text
        }

        $currentSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
        $jobserverRights = [System.Security.AccessControl.SemaphoreRights]::Synchronize -bor
            [System.Security.AccessControl.SemaphoreRights]::Modify
        $jobserverSecurity = New-ConstructionJobserverSecurity -Sid $currentSid
        $jobserverRules = @($jobserverSecurity.GetAccessRules(
            $true,
            $false,
            [System.Security.Principal.SecurityIdentifier]
        ))
        Require $jobserverSecurity.AreAccessRulesProtected "Construction jobserver DACL was not protected."
        Require ($jobserverRules.Count -eq 1) "Construction jobserver DACL was not target-only."
        Require `
            ($jobserverRules[0].IdentityReference -eq $currentSid -and
                $jobserverRules[0].SemaphoreRights -eq $jobserverRights -and
                $jobserverRules[0].AccessControlType -eq
                    [System.Security.AccessControl.AccessControlType]::Allow) `
            "Construction jobserver DACL did not grant the exact target rights."
        $jobserverName = "Global\ProjectAtlasJobserverTest-$([guid]::NewGuid().ToString('N'))"
        $jobserver = New-ConstructionJobserver -Sid $currentSid -Name $jobserverName
        $openedJobserver = $null
        $daclReader = $null
        try {
            Require `
                ([ProjectAtlasConstructionProcess]::HasMediumMandatoryLabel(
                    $jobserver.SafeWaitHandle
                )) `
                "Construction jobserver did not retain its medium mandatory label."
            $daclReader = [System.Threading.SemaphoreAcl]::OpenExisting(
                $jobserverName,
                [System.Security.AccessControl.SemaphoreRights]::ReadPermissions
            )
            Require `
                ([ProjectAtlasConstructionProcess]::HasExpectedJobserverDacl(
                    $daclReader.SafeWaitHandle,
                    $currentSid.Value
                )) `
                "Construction jobserver did not retain its target-only DACL."
            $openedJobserver = [System.Threading.SemaphoreAcl]::OpenExisting(
                $jobserverName,
                $jobserverRights
            )
            Require ($openedJobserver.WaitOne(0)) "Construction jobserver did not expose one token."
            Require `
                ($openedJobserver.Release() -eq 0) `
                "Construction jobserver did not restore its token."
            $collisionRejected = $false
            try {
                $unexpectedJobserver = New-ConstructionJobserver `
                    -Sid $currentSid `
                    -Name $jobserverName
                $unexpectedJobserver.Dispose()
            }
            catch {
                $collisionRejected = $true
            }
            Require $collisionRejected "Construction jobserver accepted a live-name collision."
        }
        finally {
            if ($null -ne $daclReader) {
                $daclReader.Dispose()
            }
            if ($null -ne $openedJobserver) {
                $openedJobserver.Dispose()
            }
            $jobserver.Dispose()
        }
    }
    $Target = "x86_64-pc-windows-msvc"
    $commandDiagnosticTailBytes = 24 * 1024
    $constructionDiagnosticMaxBytes = 64 * 1024
    $script:constructionDiagnosticPath = [System.IO.Path]::Combine(
        $output,
        "construction-diagnostic.txt"
    )
    $script:constructionStatusPath = [System.IO.Path]::Combine(
        $output,
        "construction-status.json"
    )
    $script:constructionStage = "optional-parser-worker-build"
    $script:constructionFailureRecorded = $false
    $script:constructionFailureExitCode = 1
    $pwsh = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName

    Invoke-Checked `
        -Executable $pwsh `
        -Arguments @(
            "-NoProfile",
            "-Command",
            '[Console]::Out.Write("success"); [Console]::Error.Write("success-error"); exit 0'
        ) `
        -Role "success self-test"
    Require `
        (-not [System.IO.File]::Exists($script:constructionDiagnosticPath)) `
        "Successful construction wrote a diagnostic."

    $failureCommand = @'
[Console]::Out.Write("stdout-head" + ("o" * 30000) + "stdout-tail")
[Console]::Error.Write("stderr-head" + ("e" * 30000) + "stderr-tail")
exit 7
'@
    $failed = $false
    try {
        Invoke-Checked `
            -Executable $pwsh `
            -Arguments @("-NoProfile", "-Command", $failureCommand) `
            -Role "failure self-test"
    }
    catch {
        $failed = $true
    }
    Require $failed "Failing construction command succeeded."
    $diagnostic = Get-Item -LiteralPath $script:constructionDiagnosticPath -Force
    Require `
        (-not $diagnostic.PSIsContainer -and
            (($diagnostic.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0) -and
            $diagnostic.Length -gt 0 -and
            $diagnostic.Length -le $constructionDiagnosticMaxBytes) `
        "Failure diagnostic was not one bounded regular file."
    $diagnosticText = [System.IO.File]::ReadAllText($diagnostic.FullName)
    Require `
        ($diagnosticText.Contains("stdout-tail") -and
            $diagnosticText.Contains("stderr-tail")) `
        "Failure diagnostic lost a stream tail."
    Require `
        (-not $diagnosticText.Contains("stdout-head") -and
            -not $diagnosticText.Contains("stderr-head")) `
        "Failure diagnostic retained overflowed stream heads."
    $status = [System.IO.File]::ReadAllText($script:constructionStatusPath) |
        ConvertFrom-Json -Depth 4
    Require `
        ($status.stage -eq "optional-parser-worker-build" -and
            $status.state -eq "failed" -and
            $status.exit_code -eq 7 -and
            $script:constructionFailureExitCode -eq 7) `
        "Failure diagnostic changed the authoritative status record."
    Write-Output "Parser-pack construction diagnostic self-test passed."
}
finally {
    if ([System.IO.Directory]::Exists($testRoot)) {
        $resolvedTestRoot = [System.IO.Path]::GetFullPath($testRoot)
        Require `
            ($resolvedTestRoot.StartsWith(
                "$($testBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar))$([System.IO.Path]::DirectorySeparatorChar)",
                [System.StringComparison]::OrdinalIgnoreCase
            )) `
            "Refused to clean a diagnostic test root outside its temporary base."
        Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
    }
}
