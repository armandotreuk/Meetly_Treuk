#requires -Version 7.0
# Task 5.4c: installed-package evidence only; never a release/quality verdict.
[CmdletBinding()]
param(
    [ValidateSet('Smoke', 'Gate', 'MeasureBefore', 'PrepareBuild', 'MeasureAfter', 'Library')]
    [string]$Mode = 'Smoke',
    [ValidateSet('msi', 'nsis')][string]$Kind = 'msi',
    [string]$MsiOutcome = '',
    [string]$NsisOutcome = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:BundleId = 'meetily-retrieval-bundle-1'
$script:ManifestDigest = '8a3751069f4c77ddec4db7c92f75d99900525bbe48e00e28ec1cf3ffff264ff4'
$script:ResourceRelative = 'resources/retrieval/bundle'
$script:RetrievalSuccess = "smoke-retrieval: stage=complete status=passed bundle=$script:BundleId manifest_sha256=$script:ManifestDigest dimensions=768 embeddings=6 pairs=5 sources=2 finite=true exit_code=0"

function Get-TreeMeasurement([string]$Path) {
    $bytes = [long]0
    $files = [long]0
    if (-not (Test-Path -LiteralPath $Path)) { return @{ bytes = $bytes; files = $files } }
    $pending = [System.Collections.Generic.Stack[string]]::new()
    $pending.Push([IO.Path]::GetFullPath($Path))
    while ($pending.Count -gt 0) {
        $item = Get-Item -LiteralPath $pending.Pop() -Force
        # Do not count/follow a junction into a cache, source tree, or user data.
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'reparse_rejected' }
        if ($item.PSIsContainer) {
            foreach ($child in Get-ChildItem -LiteralPath $item.FullName -Force) { $pending.Push($child.FullName) }
        } else { $bytes += $item.Length; $files++ }
    }
    return @{ bytes = $bytes; files = $files }
}

function Assert-NoReparseAncestors([string]$Path) {
    $cursor = [IO.Path]::GetFullPath($Path)
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw 'reparse_rejected'
            }
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
}

function Get-EvidenceIdentity {
    $sha = $env:GITHUB_SHA
    if ($sha -notmatch '^[0-9a-f]{40}$') { throw 'commit_identity_unavailable' }
    $head = (& git rev-parse HEAD 2>$null | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $head -cne $sha) { throw 'commit_identity_mismatch' }
    if ($env:GITHUB_SERVER_URL -notmatch '^https://[a-zA-Z0-9.-]+$' -or
        $env:GITHUB_REPOSITORY -notmatch '^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+$' -or
        $env:GITHUB_RUN_ID -notmatch '^[0-9]+$') { throw 'run_identity_unavailable' }
    return @{ commit = $sha; run_url = "$env:GITHUB_SERVER_URL/$env:GITHUB_REPOSITORY/actions/runs/$env:GITHUB_RUN_ID" }
}

function Convert-SignatureState([string]$Status) {
    switch ($Status) {
        'Valid' { return 'valid' }
        'NotSigned' { return 'unsigned' }
        # PowerShell defines UnknownError as an invalid signature, not absence
        # of signing evidence. Never treat that as a missing-credentials pass.
        'UnknownError' { return 'invalid' }
        'HashMismatch' { return 'invalid' }
        'NotTrusted' { return 'invalid' }
        default { return 'unverified' }
    }
}

function Get-SignatureState([string]$Path) {
    try { return Convert-SignatureState ([string](Get-AuthenticodeSignature -LiteralPath $Path).Status) }
    catch { return 'unavailable' }
}

function Invoke-SmokeProcess([string]$FilePath, [string]$Arguments, [int]$TimeoutMs, [string]$CaptureRoot) {
    # Raw streams are private temporary files, never workflow outputs/artifacts.
    Assert-NoReparseAncestors $CaptureRoot
    $captureId = [Guid]::NewGuid().ToString('N')
    $stdoutPath = Join-Path $CaptureRoot "$captureId.stdout"
    $stderrPath = Join-Path $CaptureRoot "$captureId.stderr"
    $process = $null
    $result = @{ status = 'launch_failed'; exit_code = $null; stdout = ''; stderr = ''; capture_valid = $true }
    try {
        $process = Start-Process -FilePath $FilePath -ArgumentList $Arguments -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
        if (-not $process.WaitForExit($TimeoutMs)) {
            $result.status = 'timeout'
            try { $process.Kill($true); $null = $process.WaitForExit(5000) } catch { }
        } else {
            $process.Refresh()
            $result.status = 'completed'
            $result.exit_code = $process.ExitCode
        }
        foreach ($entry in @(@{ key = 'stdout'; path = $stdoutPath }, @{ key = 'stderr'; path = $stderrPath })) {
            if (Test-Path -LiteralPath $entry.path) {
                if ((Get-Item -LiteralPath $entry.path).Length -gt 65536) { $result.capture_valid = $false }
                else { $result[$entry.key] = [IO.File]::ReadAllText($entry.path) }
            }
        }
    } catch {
        # Never serialize exception messages, process arguments, or captured text.
        if ($result.status -ne 'timeout') { $result.status = 'launch_failed' }
    } finally {
        if ($null -ne $process) { $process.Dispose() }
        foreach ($capture in @($stdoutPath, $stderrPath)) {
            if (Test-Path -LiteralPath $capture) { Remove-Item -LiteralPath $capture -Force -ErrorAction SilentlyContinue }
        }
    }
    return $result
}

function Convert-DiagnosticResult($Process, [ValidateSet('dbstat', 'retrieval')][string]$Diagnostic) {
    $result = @{ status = $Process.status; exit_code = $Process.exit_code; output_contract = 'not_checked' }
    if ($Process.status -ne 'completed') { return $result }
    $result.status = if ($Process.exit_code -eq 0) { 'passed' } else { 'failed' }
    if ($Diagnostic -eq 'dbstat') {
        # The existing GUI-subsystem dbstat diagnostic has an exit-code contract.
        $result.output_contract = 'exit_code_only'
    } elseif ($Process.exit_code -eq 0) {
        if ($Process.capture_valid -and $Process.stdout.Trim() -ceq $script:RetrievalSuccess -and
            [string]::IsNullOrWhiteSpace($Process.stderr)) { $result.output_contract = 'passed' }
        else { $result.output_contract = 'invalid'; $result.status = 'output_contract_failed' }
    } else {
        # Preserve even unknown/crash codes, without trusting or echoing any text.
        $result.output_contract = 'failure_exit_code'
    }
    return $result
}

function Get-MsiPackageIdentity([string]$Path) {
    # Read package authority directly from the MSI database. Win32_Product is
    # deliberately forbidden because querying it can repair installed apps.
    $installer = $null
    $database = $null
    $view = $null
    $record = $null
    try {
        $installer = New-Object -ComObject WindowsInstaller.Installer
        $database = $installer.OpenDatabase([IO.Path]::GetFullPath($Path), 0)
        $identity = @{}
        foreach ($property in @('ProductCode', 'UpgradeCode', 'ProductName', 'Manufacturer')) {
            $view = $database.OpenView(('SELECT `Value` FROM `Property` WHERE `Property`=''{0}''' -f $property))
            $null = $view.Execute()
            $record = $view.Fetch()
            if ($null -eq $record) { throw 'msi_identity_incomplete' }
            $value = [string]$record.StringData(1)
            if ([string]::IsNullOrWhiteSpace($value)) { throw 'msi_identity_incomplete' }
            $identity[$property] = $value
            $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($record); $record = $null
            $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($view); $view = $null
        }
    } finally {
        if ($null -ne $record) { $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($record) }
        if ($null -ne $view) { $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($view) }
        if ($null -ne $database) { $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($database) }
        if ($null -ne $installer) { $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer) }
    }
    foreach ($name in @('ProductCode', 'UpgradeCode')) {
        if ($identity[$name] -notmatch '^\{[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}\}$') {
            throw 'msi_identity_invalid'
        }
        $identity[$name] = $identity[$name].ToUpperInvariant()
    }
    return @{ product_code = $identity.ProductCode; upgrade_code = $identity.UpgradeCode
        product_name = $identity.ProductName; manufacturer = $identity.Manufacturer }
}

function Get-DistinctInstallerRegistration($Records) {
    $seen = @{}
    $result = @()
    foreach ($record in $Records) {
        if ($record.kind -notin @('nsis_uninstall', 'wix_uninstall', 'nsis_manufacturer')) {
            $result += $record
            continue
        }
        # HKCU\Software is a shared WOW64 key. We still inspect both views,
        # then collapse only identical observations of the same physical key.
        $viewPart = if ($record.hive -eq 'CurrentUser') { 'shared' } else { [string]$record.view }
        $key = "$($record.hive)|$viewPart|$($record.kind)|$([string]$record.key_name)".ToLowerInvariant()
        $fingerprintValues = foreach ($property in @('default_value', 'display_name', 'publisher', 'install_location', 'install_dir_hint', 'uninstall_string', 'active')) {
            if ($record.ContainsKey($property)) { [string]$record[$property] } else { '' }
        }
        $fingerprint = $fingerprintValues -join "`0"
        if ($seen.ContainsKey($key)) {
            if ($seen[$key] -cne $fingerprint) { throw 'registry_inventory_inconsistent' }
            continue
        }
        $seen[$key] = $fingerprint
        $result += $record
    }
    return $result
}

function Get-WindowsInstallerRegistration([string]$ProductCode) {
    if (-not ('Meetily.WindowsInstallerInventory' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
namespace Meetily {
  public static class WindowsInstallerInventory {
    [DllImport("msi.dll", CharSet = CharSet.Unicode)]
    public static extern uint MsiEnumProductsEx(string productCode, string userSid, uint context,
      uint index, StringBuilder installedProductCode, out uint installedContext,
      StringBuilder sid, ref uint sidLength);
    [DllImport("msi.dll", CharSet = CharSet.Unicode)]
    public static extern uint MsiGetProductInfoEx(string productCode, string userSid, uint context,
      string property, StringBuilder value, ref uint valueLength);
    [DllImport("msi.dll", CharSet = CharSet.Unicode)]
    public static extern uint MsiEnumRelatedProducts(string upgradeCode, uint reserved, uint index,
      StringBuilder productCode);
  }
}
'@
    }
    $records = @()
    $index = [uint32]0
    do {
        $code = [Text.StringBuilder]::new(39)
        $sid = [Text.StringBuilder]::new(256)
        $sidLength = [uint32]$sid.Capacity
        $context = [uint32]0
        # S-1-1-0 requests every user; ALL covers managed, unmanaged, machine.
        $status = [Meetily.WindowsInstallerInventory]::MsiEnumProductsEx(
            $ProductCode, 'S-1-1-0', 7, $index, $code, [ref]$context, $sid, [ref]$sidLength)
        if ($status -eq 259) { break }
        if ($status -ne 0) { throw "windows_installer_inventory_failed_$status" }
        $location = [Text.StringBuilder]::new(32768)
        $locationLength = [uint32]$location.Capacity
        $instanceSid = if ($context -eq 4 -or $sid.Length -eq 0) { $null } else { $sid.ToString() }
        $locationStatus = [Meetily.WindowsInstallerInventory]::MsiGetProductInfoEx(
            $code.ToString(), $instanceSid, $context, 'InstallLocation', $location, [ref]$locationLength)
        if ($locationStatus -ne 0) { throw "windows_installer_inventory_failed_$locationStatus" }
        $records += @{ kind = 'msi_product'; product_code = $code.ToString().ToUpperInvariant(); context = $context
            install_location = $location.ToString() }
        $index++
    } while ($true)
    return $records
}

function Get-WindowsInstallerRelatedRegistration([string]$UpgradeCode) {
    # This API matches the effective current-user plus machine scope used by
    # Windows Installer's FindRelatedProducts action. Exact ProductCode uses
    # MsiEnumProductsEx above across every user and installation context.
    $records = @()
    $index = [uint32]0
    do {
        $code = [Text.StringBuilder]::new(39)
        $status = [Meetily.WindowsInstallerInventory]::MsiEnumRelatedProducts($UpgradeCode, 0, $index, $code)
        if ($status -eq 259) { break }
        if ($status -ne 0) { throw "windows_installer_inventory_failed_$status" }
        $records += @{ kind = 'msi_related'; product_code = $code.ToString().ToUpperInvariant() }
        $index++
    } while ($true)
    return $records
}

function Get-RegistryRegistration([Microsoft.Win32.RegistryHive]$Hive, [Microsoft.Win32.RegistryView]$View,
    [string]$Path, [string]$Kind, [string]$ProductCode = '') {
    $base = $null
    $key = $null
    try {
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($Hive, $View)
        $key = $base.OpenSubKey($Path, $false)
        if ($null -eq $key) { return $null }
        return @{ kind = $Kind; product_code = $ProductCode; hive = [string]$Hive; view = [string]$View
            key_name = [IO.Path]::GetFileName($Path); default_value = [string]$key.GetValue($null, '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            display_name = [string]$key.GetValue('DisplayName', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            publisher = [string]$key.GetValue('Publisher', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            install_location = [string]$key.GetValue('InstallLocation', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            install_dir_hint = [string]$key.GetValue('InstallDir', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            uninstall_string = [string]$key.GetValue('UninstallString', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
    } finally {
        if ($null -ne $key) { $key.Dispose() }
        if ($null -ne $base) { $base.Dispose() }
    }
}

function Convert-RegisteredPath([string]$Value) {
    $trimmed = $Value.Trim()
    if ($trimmed.Length -ge 2 -and $trimmed[0] -eq '"' -and $trimmed[$trimmed.Length - 1] -eq '"') {
        $trimmed = $trimmed.Substring(1, $trimmed.Length - 2)
    }
    if ([string]::IsNullOrWhiteSpace($trimmed) -or -not [IO.Path]::IsPathFullyQualified($trimmed)) { return $null }
    try { return [IO.Path]::GetFullPath($trimmed) } catch { return $null }
}

function Test-MatchingTauriWixRegistration($Candidate, $Identity) {
    # Mirror the pinned Tauri NSIS migration predicate conservatively: NSIS
    # StrCmp compares the concatenated values case-insensitively and its
    # StrLoc check accepts "msiexec" at any position.
    return (([string]$Candidate.display_name + [string]$Candidate.publisher) -ieq
        ([string]$Identity.product_name + [string]$Identity.manufacturer) -and
        ([string]$Candidate.uninstall_string).IndexOf('msiexec', [StringComparison]::OrdinalIgnoreCase) -ge 0)
}

function Test-ManufacturerRegistrationActive($Registration, [ValidateSet('msi', 'nsis')][string]$PackageKind) {
    $values = @([string]$Registration.default_value, [string]$Registration.install_dir_hint)
    if ($PackageKind -eq 'msi') { return @($values | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }).Count -ne 0 }
    foreach ($value in $values) {
        $storedRoot = Convert-RegisteredPath $value
        if ($null -ne $storedRoot -and (Test-Path -LiteralPath $storedRoot)) { return $true }
    }
    return $false
}

function Get-InstallerRegistrationInventory($Identity, [ValidateSet('msi', 'nsis')][string]$PackageKind = 'nsis') {
    $records = @(Get-WindowsInstallerRegistration $Identity.product_code)
    $records += @(Get-WindowsInstallerRelatedRegistration $Identity.upgrade_code)
    $uninstallRoot = 'Software\Microsoft\Windows\CurrentVersion\Uninstall'
    foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryHive]::LocalMachine)) {
        foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32, [Microsoft.Win32.RegistryView]::Registry64)) {
            $base = $null
            $uninstall = $null
            try {
                $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, $view)
                $uninstall = $base.OpenSubKey($uninstallRoot, $false)
                if ($null -ne $uninstall) {
                    foreach ($name in $uninstall.GetSubKeyNames()) {
                        $candidate = Get-RegistryRegistration $hive $view "$uninstallRoot\$name" 'uninstall'
                        # The key can vanish between enumeration and opening
                        # while a bounded teardown poll is observing NSIS.
                        if ($null -eq $candidate) { continue }
                        $isProduct = $name -ieq $Identity.product_code
                        $isNsis = $name -ieq $Identity.product_name
                        $isMatchingWix = Test-MatchingTauriWixRegistration $candidate $Identity
                        if ($isProduct -or $isNsis -or $isMatchingWix) {
                            $candidate.kind = if ($isNsis) { 'nsis_uninstall' } else { 'wix_uninstall' }
                            if ($name -match '^\{[0-9A-Fa-f-]{36}\}$') { $candidate.product_code = $name.ToUpperInvariant() }
                            $records += $candidate
                        }
                    }
                }
                $manufacturer = Get-RegistryRegistration $hive $view "Software\$($Identity.manufacturer)\$($Identity.product_name)" 'nsis_manufacturer'
                if ($null -ne $manufacturer) {
                    # Tauri NSIS intentionally leaves this location hint behind
                    # after silent uninstall. It can collide only while it still
                    # designates an extant installation; explicit /D bypasses it.
                    # WiX AppSearch consumes either value even when the target
                    # is absent or malformed. NSIS with explicit /D bypasses
                    # RestorePreviousInstallLocation, so only its extant hint
                    # is active; a stale one is retained evidence, not deleted.
                    $manufacturer.active = Test-ManufacturerRegistrationActive $manufacturer $PackageKind
                    $records += $manufacturer
                }
            } finally {
                if ($null -ne $uninstall) { $uninstall.Dispose() }
                if ($null -ne $base) { $base.Dispose() }
            }
        }
    }
    return @(Get-DistinctInstallerRegistration $records)
}

function Get-OwnedMsiRegistration($Records, $Identity, [Nullable[int]]$InstallExitCode, [string[]]$AllowedRoots) {
    # 1638 means another version is already present. A registration appearing
    # in that race is never ours to remove.
    if ($InstallExitCode -eq 1638) { return $null }
    $owned = @($Records | Where-Object { $_.kind -eq 'msi_product' -and $_.product_code -ieq $Identity.product_code })
    if ($owned.Count -ne 1) { return $null }
    $registeredRoot = Convert-RegisteredPath $owned[0].install_location
    $approvedRoot = $false
    if ($null -ne $registeredRoot) {
        foreach ($root in $AllowedRoots) {
            if ($registeredRoot.TrimEnd('\', '/').Equals([IO.Path]::GetFullPath($root).TrimEnd('\', '/'),
                [StringComparison]::OrdinalIgnoreCase)) { $approvedRoot = $true; break }
        }
    }
    if (-not $approvedRoot) { return $null }
    foreach ($record in $Records) {
        if ($record.kind -eq 'nsis_manufacturer' -and -not $record.active) { continue }
        if ($record.kind -eq 'nsis_manufacturer') {
            $hintApproved = $false
            foreach ($property in @('default_value', 'install_dir_hint')) {
                $value = if ($record.ContainsKey($property)) { [string]$record[$property] } else { '' }
                $hint = Convert-RegisteredPath $value
                if ($null -eq $hint) { continue }
                foreach ($root in $AllowedRoots) {
                    if ($hint.TrimEnd('\', '/').Equals([IO.Path]::GetFullPath($root).TrimEnd('\', '/'),
                        [StringComparison]::OrdinalIgnoreCase)) { $hintApproved = $true; break }
                }
                if ($hintApproved) { break }
            }
            if ($hintApproved) { continue }
        }
        if ($record.kind -in @('msi_product', 'msi_related', 'wix_uninstall') -and
            $record.product_code -ieq $Identity.product_code) { continue }
        return $null
    }
    return $Identity.product_code
}

function Get-OwnedNsisUninstaller($Records, $Identity, [string]$InstallDirectory) {
    $expectedRoot = [IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\', '/')
    $uninstalls = @($Records | Where-Object { $_.kind -eq 'nsis_uninstall' })
    $locations = @($Records | Where-Object { $_.kind -eq 'nsis_manufacturer' })
    if ($uninstalls.Count -ne 1 -or $locations.Count -ne 1 -or $Records.Count -ne 2) { return $null }
    $registeredRoot = Convert-RegisteredPath $locations[0].default_value
    $installLocation = Convert-RegisteredPath $uninstalls[0].install_location
    $uninstaller = Convert-RegisteredPath $uninstalls[0].uninstall_string
    if ($null -eq $registeredRoot -or $null -eq $installLocation -or $null -eq $uninstaller -or
        -not $registeredRoot.Equals($expectedRoot, [StringComparison]::OrdinalIgnoreCase) -or
        -not $installLocation.Equals($expectedRoot, [StringComparison]::OrdinalIgnoreCase) -or
        -not $uninstaller.Equals((Join-Path $expectedRoot 'uninstall.exe'), [StringComparison]::OrdinalIgnoreCase) -or
        $uninstalls[0].hive -cne $locations[0].hive -or $uninstalls[0].view -cne $locations[0].view) { return $null }
    return $uninstaller
}

function Get-InstalledExecutable([string[]]$Roots) {
    $found = @()
    for ($index = 0; $index -lt $Roots.Count; $index++) {
        if (-not (Test-Path -LiteralPath $Roots[$index])) { continue }
        Assert-NoReparseAncestors $Roots[$index]
        $null = Get-TreeMeasurement $Roots[$index]
        foreach ($exe in Get-ChildItem -LiteralPath $Roots[$index] -Filter 'meetily.exe' -File -Recurse) {
            $relative = [IO.Path]::GetRelativePath($Roots[$index], $exe.FullName).Replace('\', '/')
            if ($relative -notmatch '^(?:[a-zA-Z0-9_.-]+/)*meetily\.exe$') { throw 'unsafe_relative_layout' }
            $found += @{ path = $exe.FullName; relative = $relative; root_index = $index }
        }
    }
    if ($found.Count -ne 1) { throw 'installed_executable_ambiguous_or_missing' }
    return $found[0]
}

function Test-InstallResidue([string[]]$Roots, [int]$TimeoutMs = 60000) {
    # NSIS may launch its real uninstaller in a child process and return early.
    # Poll the *actual searched roots*, not a directory erased by this harness.
    $watch = [Diagnostics.Stopwatch]::StartNew()
    do {
        $remainingRoots = 0
        foreach ($root in $Roots) {
            Assert-NoReparseAncestors $root
            if (Test-Path -LiteralPath $root) {
                # An empty installation directory is still uninstall residue.
                # Never erase it here to turn an incomplete teardown into PASS.
                $null = Get-TreeMeasurement $root
                $remainingRoots++
            }
        }
        if ($remainingRoots -eq 0) { return 'passed' }
        if ($watch.ElapsedMilliseconds -ge $TimeoutMs) { return 'failed' }
        Start-Sleep -Milliseconds 200
    } while ($true)
}

function Wait-PackageTeardown([string[]]$Roots, $Identity, [string]$PackageKind, [int]$TimeoutMs = 60000) {
    # Poll registration and files together because the NSIS parent may return
    # before its temporary child removes either one.
    $watch = [Diagnostics.Stopwatch]::StartNew()
    do {
        try {
            $inventory = @(Get-InstallerRegistrationInventory $Identity $PackageKind)
            $active = @($inventory | Where-Object { $_.kind -ne 'nsis_manufacturer' -or $_.active })
            $registration = if ($active.Count -eq 0) { 'passed' } else { 'failed' }
            $auxiliary = if (@($inventory | Where-Object { $_.kind -eq 'nsis_manufacturer' -and -not $_.active }).Count) {
                'retained_inactive'
            } else { 'absent' }
        } catch {
            return @{ registration = 'inspection_failed'; residue = 'inspection_failed'; auxiliary = 'not_checked' }
        }
        try {
            $remainingRoots = 0
            foreach ($root in $Roots) {
                Assert-NoReparseAncestors $root
                if (Test-Path -LiteralPath $root) { $null = Get-TreeMeasurement $root; $remainingRoots++ }
            }
            $residue = if ($remainingRoots -eq 0) { 'passed' } else { 'failed' }
        } catch {
            return @{ registration = $registration; residue = 'inspection_failed'; auxiliary = $auxiliary }
        }
        if ($registration -eq 'passed' -and $residue -eq 'passed') {
            return @{ registration = $registration; residue = $residue; auxiliary = $auxiliary }
        }
        if ($watch.ElapsedMilliseconds -ge $TimeoutMs) {
            return @{ registration = $registration; residue = $residue; auxiliary = $auxiliary }
        }
        Start-Sleep -Milliseconds 200
    } while ($true)
}

function Test-SmokePassed($Result) {
    return ($Result.harness -eq 'passed' -and $Result.install.status -eq 'passed' -and
        $Result.registration.preflight -eq 'passed' -and $Result.registration.ownership -eq 'proved' -and
        $Result.registration.post_teardown -eq 'passed' -and
        $Result.discovery.status -eq 'passed' -and $Result.resources.status -eq 'passed' -and
        $Result.dbstat.status -eq 'passed' -and $Result.dbstat.exit_code -eq 0 -and
        $Result.retrieval.status -eq 'passed' -and $Result.retrieval.exit_code -eq 0 -and
        $Result.retrieval.output_contract -eq 'passed' -and $Result.teardown.status -eq 'passed' -and
        $Result.residue -eq 'passed' -and $Result.signing.policy -eq 'passed' -and
        (Get-SigningPolicy $Result.signing) -eq 'passed')
}

function Get-SigningPolicy($Signing) {
    if ($Signing.installer -eq 'invalid' -or $Signing.executable -eq 'invalid') { return 'failed' }
    if ($Signing.credentials -eq 'available' -and
        ($Signing.installer -ne 'valid' -or $Signing.executable -ne 'valid')) { return 'failed' }
    return 'passed'
}

function Invoke-PackageSmoke([string]$PackageKind, [string]$BundleDirectory, [string]$InstallDirectory,
    [string[]]$SearchRoots, [string]$CaptureRoot, $Identity, $ExpectedBundle, [int]$ResidueTimeoutMs = 60000) {
    $result = [ordered]@{
        schema = 2; package = $PackageKind; commit = $Identity.commit; run_url = $Identity.run_url
        bundle = $script:BundleId; manifest_sha256 = $script:ManifestDigest
        harness = 'failed'; overall = 'failed'; installer_relative = ''; installer_bytes = [long]0; installer_sha256 = ''
        install = @{ status = 'not_run'; exit_code = $null }
        registration = @{ preflight = 'not_run'; ownership = 'not_checked'; post_teardown = 'not_checked'; auxiliary = 'not_checked' }
        discovery = @{ status = 'not_run'; executable_relative = ''; root = 'not_discovered' }
        resources = @{ status = 'not_run'; relative = $script:ResourceRelative; bytes = [long]0; files = [long]0 }
        dbstat = @{ status = 'not_run'; exit_code = $null; output_contract = 'not_checked' }
        retrieval = @{ status = 'not_run'; exit_code = $null; output_contract = 'not_checked' }
        teardown = @{ status = 'not_run'; exit_code = $null }; residue = 'not_checked'
        signing = @{ credentials = $(if ($env:DIGICERT_KEYPAIR_ALIAS) { 'available' } else { 'unavailable' })
            tool = $(if (Get-Command smctl -ErrorAction SilentlyContinue) { 'available' } else { 'unavailable' })
            installer = 'not_checked'; executable = 'not_checked'; policy = 'not_checked' }
    }
    $installer = $null
    $packageIdentity = $null
    $invoked = $false
    $cleanupTarget = $null
    $overrides = @{}
    try {
        foreach ($name in @('MEETLY_RAG_BUNDLE_DIR', 'MEETLY_RAG_MODELS_DIR')) {
            $overrides[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
            [Environment]::SetEnvironmentVariable($name, $null, 'Process')
        }
        Assert-NoReparseAncestors $BundleDirectory
        $extension = if ($PackageKind -eq 'msi') { '*.msi' } else { '*.exe' }
        $installers = @(Get-ChildItem -LiteralPath $BundleDirectory -Filter $extension -File)
        if ($installers.Count -ne 1) { $result.harness = 'installer_discovery_failed'; return $result }
        $installer = $installers[0].FullName
        if ($installers[0].Name -notmatch '^[a-zA-Z0-9_. -]+$') { throw 'unsafe_installer_name' }
        Assert-NoReparseAncestors $installer
        $result.installer_relative = "bundle/$PackageKind/$($installers[0].Name)"
        $result.installer_bytes = $installers[0].Length
        $result.installer_sha256 = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
        $result.signing.installer = Get-SignatureState $installer
        $msiAuthority = if ($PackageKind -eq 'msi') { $installer } else {
            $msiDirectory = Join-Path ([IO.Path]::GetDirectoryName($BundleDirectory)) 'msi'
            Assert-NoReparseAncestors $msiDirectory
            $msiPackages = @(Get-ChildItem -LiteralPath $msiDirectory -Filter '*.msi' -File)
            if ($msiPackages.Count -ne 1) { throw 'msi_authority_discovery_failed' }
            $msiPackages[0].FullName
        }
        $packageIdentity = Get-MsiPackageIdentity $msiAuthority
        try { $preflight = @(Get-InstallerRegistrationInventory $packageIdentity $PackageKind) }
        catch { $result.registration.preflight = 'inspection_failed'; throw }
        $result.registration.auxiliary = if (@($preflight | Where-Object { $_.kind -eq 'nsis_manufacturer' -and -not $_.active }).Count) {
            'retained_inactive'
        } else { 'absent' }
        $blocking = @($preflight | Where-Object { $_.kind -ne 'nsis_manufacturer' -or $_.active })
        if ($blocking.Count -ne 0) { $result.harness = 'preexisting_registration'; $result.registration.preflight = 'blocked'; return $result }
        $result.registration.preflight = 'passed'
        foreach ($root in $SearchRoots) {
            Assert-NoReparseAncestors $root
            # Registration and filesystem checks jointly protect custom roots.
            if (Test-Path -LiteralPath $root) { $result.harness = 'preexisting_install'; return $result }
        }
        $invoked = $true
        if ($PackageKind -eq 'msi') {
            $process = Invoke-SmokeProcess 'msiexec.exe' "/i `"$installer`" /qn /norestart INSTALLDIR=`"$InstallDirectory`"" 300000 $CaptureRoot
        } else {
            # NSIS /D must be last and unquoted, including paths with spaces.
            $process = Invoke-SmokeProcess $installer "/S /D=$InstallDirectory" 300000 $CaptureRoot
        }
        $result.install = @{ status = $process.status; exit_code = $process.exit_code }
        try { $postInstall = @(Get-InstallerRegistrationInventory $packageIdentity $PackageKind) }
        catch { $result.registration.ownership = 'inspection_failed'; throw }
        $postBlocking = @($postInstall | Where-Object { $_.kind -ne 'nsis_manufacturer' -or $_.active })
        if ($PackageKind -eq 'msi') {
            $cleanupTarget = Get-OwnedMsiRegistration $postBlocking $packageIdentity $process.exit_code $SearchRoots
        } else {
            $cleanupTarget = Get-OwnedNsisUninstaller $postBlocking $packageIdentity $InstallDirectory
        }
        $result.registration.ownership = if ($null -ne $cleanupTarget) { 'proved' } else { 'unproven' }
        if ($process.status -eq 'completed' -and $process.exit_code -ne 0) { $result.install.status = 'failed' }
        if ($process.status -ne 'completed' -or $process.exit_code -ne 0) { return $result }
        $result.install.status = 'passed'
        if ($null -eq $cleanupTarget) { $result.harness = 'cleanup_ownership_unproven'; return $result }
        $result.discovery.status = 'failed'
        $executable = Get-InstalledExecutable $SearchRoots
        $result.discovery = @{ status = 'passed'; executable_relative = $executable.relative
            root = @('isolated', 'program_files', 'program_files_x86')[$executable.root_index] }
        $result.signing.executable = Get-SignatureState $executable.path
        $result.resources.status = 'failed'
        $resources = Join-Path ([IO.Path]::GetDirectoryName($executable.path)) $script:ResourceRelative
        Assert-NoReparseAncestors $resources
        $measurement = Get-TreeMeasurement $resources
        $manifest = Join-Path $resources 'model-bundle.manifest.json'
        $resourceStatus = 'passed'
        if (-not (Test-Path -LiteralPath $manifest -PathType Leaf) -or
            (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash.ToLowerInvariant() -cne $script:ManifestDigest -or
            $measurement.bytes -ne $ExpectedBundle.bytes -or $measurement.files -ne $ExpectedBundle.files) {
            $resourceStatus = 'failed'
        }
        $result.resources = @{ status = $resourceStatus; relative = $script:ResourceRelative; bytes = $measurement.bytes; files = $measurement.files }
        # Both diagnostics run, even if dbstat returns non-zero, preserving each
        # independent verdict, including retrieval's own invalid-resource code.
        # No argument can point retrieval at source/cache.
        $result.dbstat = Convert-DiagnosticResult (Invoke-SmokeProcess $executable.path '--smoke-dbstat' 120000 $CaptureRoot) 'dbstat'
        $result.retrieval = Convert-DiagnosticResult (Invoke-SmokeProcess $executable.path '--smoke-retrieval' 120000 $CaptureRoot) 'retrieval'
        $result.harness = 'passed'
    } catch {
        $result.harness = 'failed'
    } finally {
        if ($invoked) {
            # Cleanup runs only after post-invocation registration proves that
            # this run owns the exact isolated installation.
            try {
                if ($null -eq $cleanupTarget) {
                    $result.teardown = @{ status = 'cleanup_skipped'; exit_code = $null }
                } elseif ($PackageKind -eq 'msi') {
                    $process = Invoke-SmokeProcess 'msiexec.exe' "/x $cleanupTarget /qn /norestart" 300000 $CaptureRoot
                    $result.teardown = @{ status = $process.status; exit_code = $process.exit_code }
                } else {
                    Assert-NoReparseAncestors $cleanupTarget
                    if (-not (Test-Path -LiteralPath $cleanupTarget -PathType Leaf)) { throw 'uninstaller_missing' }
                    $process = Invoke-SmokeProcess $cleanupTarget '/S' 300000 $CaptureRoot
                    $result.teardown = @{ status = $process.status; exit_code = $process.exit_code }
                }
                if ($null -ne $cleanupTarget) {
                    if ($process.status -eq 'completed' -and $process.exit_code -eq 0) { $result.teardown.status = 'passed' }
                    elseif ($process.status -eq 'completed') { $result.teardown.status = 'failed' }
                }
            } catch { $result.teardown.status = 'failed' }
            $waitTimeout = if ($null -eq $cleanupTarget) { 0 } else { $ResidueTimeoutMs }
            $teardownState = Wait-PackageTeardown $SearchRoots $packageIdentity $PackageKind $waitTimeout
            $result.registration.post_teardown = $teardownState.registration
            $result.registration.auxiliary = $teardownState.auxiliary
            $result.residue = $teardownState.residue
            # Deliberately do not Remove-Item any installation root. Residue
            # remains available for private runner investigation and is a fail.
        }
        foreach ($name in $overrides.Keys) { [Environment]::SetEnvironmentVariable($name, $overrides[$name], 'Process') }
        $result.signing.policy = Get-SigningPolicy $result.signing
        if (Test-SmokePassed $result) { $result.overall = 'passed' }
    }
    return $result
}

function Write-SmokeEvidence($Result, [string]$ResultPath, [string]$SummaryPath) {
    [IO.File]::WriteAllText($ResultPath, ($Result | ConvertTo-Json -Depth 8))
    $lines = @(
        "### Installed $($Result.package) package smoke"
        ''
        "- Commit: $($Result.commit); workflow: $($Result.run_url)"
        "- Installer: $($Result.installer_relative); bytes=$($Result.installer_bytes)"
        "- Installer SHA-256: $($Result.installer_sha256)"
        "- Installed root class: $($Result.discovery.root); executable: $($Result.discovery.executable_relative)"
        "- Resource shape: $($Result.resources.relative); bytes=$($Result.resources.bytes); files=$($Result.resources.files)"
        "- Bundle: $($Result.bundle); manifest SHA-256: $($Result.manifest_sha256)"
        "- Harness=$($Result.harness); install=$($Result.install.status) code=$($Result.install.exit_code); discovery=$($Result.discovery.status); resources=$($Result.resources.status)"
        "- Registration: preflight=$($Result.registration.preflight); ownership=$($Result.registration.ownership); post_teardown=$($Result.registration.post_teardown); auxiliary=$($Result.registration.auxiliary)"
        "- Dbstat=$($Result.dbstat.status) code=$($Result.dbstat.exit_code); retrieval=$($Result.retrieval.status) code=$($Result.retrieval.exit_code) contract=$($Result.retrieval.output_contract)"
        "- Uninstall=$($Result.teardown.status) code=$($Result.teardown.exit_code); residue=$($Result.residue); overall=$($Result.overall)"
        "- Signing: credentials=$($Result.signing.credentials); tool=$($Result.signing.tool); installer=$($Result.signing.installer); executable=$($Result.signing.executable); policy=$($Result.signing.policy)"
        '- Signing command/identity unchanged. Unavailable credentials or unverified signatures are a signing-evidence limitation, not proof of signing.'
        '- Intermediate installed-package evidence only; no Sprint 5 or release qualification claim.'
        ''
    )
    if ($SummaryPath) { [IO.File]::AppendAllLines($SummaryPath, [string[]]$lines) }
    Write-Host "installed-smoke: package=$($Result.package) status=$($Result.overall) dbstat_exit=$($Result.dbstat.exit_code) retrieval_exit=$($Result.retrieval.exit_code) teardown=$($Result.teardown.status) residue=$($Result.residue)"
}

function Test-EvidenceGate([string]$Outcome, [string]$Path, [string]$PackageKind, [string]$Commit) {
    if ($Outcome -eq 'skipped' -or $Outcome -eq '') { return 'skipped' }
    if ($Outcome -ne 'success' -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) { return 'failed' }
    try {
        if ((Get-Item -LiteralPath $Path).Length -gt 65536) { return 'failed' }
        $result = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json -AsHashtable
        if ($result.schema -ne 2 -or $result.package -cne $PackageKind -or $result.commit -cne $Commit -or
            $result.manifest_sha256 -cne $script:ManifestDigest -or $result.bundle -cne $script:BundleId -or
            $result.overall -ne 'passed' -or -not (Test-SmokePassed $result)) { return 'failed' }
        return 'passed'
    } catch { return 'failed' }
}

function Invoke-SizeEvidence([string]$Phase, [string]$Workspace, [string]$TemporaryRoot) {
    $path = Join-Path $TemporaryRoot 'meetily-package-sizes.json'
    $identity = Get-EvidenceIdentity
    if ($Phase -eq 'before') {
        $cacheState = switch ($env:MODEL_CACHE_HIT) { 'true' { 'exact_hit' }; 'false' { 'prefix_restore' }; default { 'miss' } }
        $data = @{ schema = 1; commit = $identity.commit; run_url = $identity.run_url; model_cache_restore = $cacheState }
    } else { $data = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable }
    if ($data.commit -cne $identity.commit) { throw 'size_identity_mismatch' }
    $data[$Phase] = @{
        staged_bundle = Get-TreeMeasurement (Join-Path $Workspace "upstream/frontend/src-tauri/$script:ResourceRelative")
        model_cache = Get-TreeMeasurement $env:MODEL_CACHE_PATH
        rust_cache = Get-TreeMeasurement (Join-Path $Workspace 'upstream/target')
        build_output = Get-TreeMeasurement (Join-Path $Workspace 'upstream/frontend/target')
    }
    if ($Phase -eq 'after') {
        $data.manifest_sha256 = (Get-FileHash -LiteralPath (Join-Path $Workspace "upstream/frontend/src-tauri/$script:ResourceRelative/model-bundle.manifest.json") -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($data.manifest_sha256 -cne $script:ManifestDigest) { throw 'manifest_identity_mismatch' }
        $data.model_cache_delta_bytes = $data.after.model_cache.bytes - $data.before.model_cache.bytes
        $data.rust_cache_delta_bytes = $data.after.rust_cache.bytes - $data.before.rust_cache.bytes
        $data.build_output_delta_bytes = $data.after.build_output.bytes - $data.before.build_output.bytes
        $lines = @('### Native package/cache measurements', '', "- Commit: $($identity.commit); workflow: $($identity.run_url)",
            "- Model cache restore=$($data.model_cache_restore); before_bytes=$($data.before.model_cache.bytes); after_bytes=$($data.after.model_cache.bytes); delta_bytes=$($data.model_cache_delta_bytes)",
            "- Staged bundle bytes=$($data.after.staged_bundle.bytes); files=$($data.after.staged_bundle.files); manifest SHA-256=$($data.manifest_sha256)",
            "- Cargo cache before_bytes=$($data.before.rust_cache.bytes); after_bytes=$($data.after.rust_cache.bytes); delta_bytes=$($data.rust_cache_delta_bytes)",
            "- Build output before_bytes=$($data.before.build_output.bytes); removed_before_build_bytes=$($data.prepare_build.build_output.bytes); after_bytes=$($data.after.build_output.bytes); delta_bytes=$($data.build_output_delta_bytes)", '')
        if ($env:GITHUB_STEP_SUMMARY) { [IO.File]::AppendAllLines($env:GITHUB_STEP_SUMMARY, [string[]]$lines) }
    }
    [IO.File]::WriteAllText($path, ($data | ConvertTo-Json -Depth 8))
}

function Clear-ExactFrontendBuildOutput([string]$Workspace, [string]$Target) {
    $workspacePath = [IO.Path]::GetFullPath($Workspace)
    $expected = [IO.Path]::GetFullPath((Join-Path $workspacePath 'upstream/frontend/target'))
    $actual = [IO.Path]::GetFullPath($Target)
    if (-not $actual.Equals($expected, [StringComparison]::OrdinalIgnoreCase) -or
        -not $actual.StartsWith($workspacePath.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'build_target_rejected'
    }
    Assert-NoReparseAncestors $actual
    $null = Get-TreeMeasurement $actual
    if (Test-Path -LiteralPath $actual) { Remove-Item -LiteralPath $actual -Recurse -Force }
}

if ($Mode -eq 'Library') { return }
try {
    if ($env:GITHUB_ACTIONS -ne 'true') { throw 'workflow_only' }
    $workspace = [IO.Path]::GetFullPath($env:GITHUB_WORKSPACE)
    $temporary = [IO.Path]::GetFullPath($env:RUNNER_TEMP)
    switch ($Mode) {
        'MeasureBefore' { Invoke-SizeEvidence 'before' $workspace $temporary }
        'PrepareBuild' {
            Invoke-SizeEvidence 'prepare_build' $workspace $temporary
            Clear-ExactFrontendBuildOutput $workspace (Join-Path $workspace 'upstream/frontend/target')
        }
        'MeasureAfter' { Invoke-SizeEvidence 'after' $workspace $temporary }
        'Smoke' {
            $identity = Get-EvidenceIdentity
            $sizes = Get-Content -LiteralPath (Join-Path $temporary 'meetily-package-sizes.json') -Raw | ConvertFrom-Json -AsHashtable
            if ($sizes.commit -cne $identity.commit -or $sizes.manifest_sha256 -cne $script:ManifestDigest) { throw 'size_identity_mismatch' }
            $installDirectory = Join-Path $temporary "meetily-smoke-$Kind"
            $roots = @($installDirectory)
            if ($Kind -eq 'msi') {
                # Tauri WiX may not expose INSTALLDIR; inspect the actual default
                # locations as well and include all of them in teardown checks.
                $roots += (Join-Path $env:ProgramFiles 'meetily')
                $roots += (Join-Path ${env:ProgramFiles(x86)} 'meetily')
            }
            if ($env:ARTIFACT_DIR -notmatch '^upstream/frontend/target/(debug|release)$') { throw 'artifact_path_rejected' }
            $bundleDirectory = Join-Path $workspace "$env:ARTIFACT_DIR/bundle/$Kind"
            $result = Invoke-PackageSmoke $Kind $bundleDirectory $installDirectory $roots $temporary $identity $sizes.after.staged_bundle
            Write-SmokeEvidence $result (Join-Path $temporary "meetily-smoke-$Kind-result.json") $env:GITHUB_STEP_SUMMARY
            # Persisted independent verdicts, not this continued step, own failure.
        }
        'Gate' {
            $failed = $false
            foreach ($entry in @(@{ kind = 'msi'; outcome = $MsiOutcome }, @{ kind = 'nsis'; outcome = $NsisOutcome })) {
                $status = Test-EvidenceGate $entry.outcome (Join-Path $temporary "meetily-smoke-$($entry.kind)-result.json") $entry.kind $env:GITHUB_SHA
                Write-Host "installed-smoke-gate: package=$($entry.kind) status=$status"
                if ($status -eq 'failed') { $failed = $true }
            }
            if ($failed) { exit 1 }
        }
    }
} catch {
    Write-Host "installed-smoke-harness: operation=$Mode status=failed"
    exit 1
}
