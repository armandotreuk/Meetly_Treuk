#requires -Version 7.0
# Mechanics-only self-tests. Synthetic mock installers are NOT MSI/NSIS or
# installed-model inference evidence and never touch Program Files/user data.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-package-smoke.ps1') -Mode Library
$script:Checks = 0
function Assert-Check([bool]$Condition, [string]$Name) {
    if (-not $Condition) { throw "selftest_failed:$Name" }
    $script:Checks++
}

$expectedProgressMarkers = @(
    'installed-smoke-progress: phase=preflight'
    'installed-smoke-progress: phase=install'
    'installed-smoke-progress: phase=ownership'
    'installed-smoke-progress: phase=resource'
    'installed-smoke-progress: phase=dbstat'
    'installed-smoke-progress: phase=retrieval'
    'installed-smoke-progress: phase=teardown'
    'installed-smoke-progress: phase=residue'
)
$actualProgressMarkers = foreach ($phase in @('preflight', 'install', 'ownership', 'resource', 'dbstat', 'retrieval', 'teardown', 'residue')) {
    Get-SmokeProgressMarker $phase
}
Assert-Check (($actualProgressMarkers -join "`n") -ceq ($expectedProgressMarkers -join "`n")) 'progress_markers_are_fixed_and_complete'
$progressWriterOutput = (& { Write-SmokeProgressMarker 'preflight' } 6>&1 | Out-String).Trim()
Assert-Check ($progressWriterOutput -ceq 'installed-smoke-progress: phase=preflight') 'progress_marker_writer_is_bounded'
$progressRejected = $false
try { $null = Get-SmokeProgressMarker 'private-runtime-value' } catch { $progressRejected = $true }
Assert-Check $progressRejected 'progress_marker_rejects_untrusted_phase'

$temporaryParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$script:TestRoot = Join-Path $temporaryParent ('meetily-package-smoke-selftest-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
$originalProcess = ${function:Invoke-SmokeProcess}
$originalSignature = ${function:Get-SignatureState}
$originalMsiIdentity = ${function:Get-MsiPackageIdentity}
$originalRegistrationInventory = ${function:Get-InstallerRegistrationInventory}
$originalBundleOverride = $env:MEETLY_RAG_BUNDLE_DIR
$originalModelsOverride = $env:MEETLY_RAG_MODELS_DIR
$originalSigning = $env:DIGICERT_KEYPAIR_ALIAS
$measurementEnvironment = @{}
foreach ($name in @('GITHUB_ACTIONS', 'GITHUB_WORKSPACE', 'RUNNER_TEMP', 'GITHUB_SHA', 'GITHUB_SERVER_URL', 'GITHUB_REPOSITORY', 'GITHUB_RUN_ID', 'GITHUB_STEP_SUMMARY', 'MODEL_CACHE_PATH', 'MODEL_CACHE_HIT')) {
    $measurementEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

function New-MockFixture([string]$Name, [string]$PackageKind = 'msi') {
    $root = Join-Path $script:TestRoot $Name
    $bundleDirectory = Join-Path $root "packages/$PackageKind"
    $msiDirectory = Join-Path $root 'packages/msi'
    New-Item -ItemType Directory -Path $bundleDirectory, $msiDirectory -Force | Out-Null
    $extension = if ($PackageKind -eq 'msi') { 'msi' } else { 'exe' }
    [IO.File]::WriteAllText((Join-Path $bundleDirectory "meetily_0.4.0_x64.$extension"), 'synthetic installer; not executable')
    if ($PackageKind -ne 'msi') { [IO.File]::WriteAllText((Join-Path $msiDirectory 'meetily_0.4.0_x64.msi'), 'synthetic MSI authority') }
    $installDirectory = Join-Path $root 'isolated install'
    $roots = @($installDirectory)
    if ($PackageKind -eq 'msi') { $roots += (Join-Path $root 'program-files'); $roots += (Join-Path $root 'program-files-x86') }
    $manifest = Join-Path $PSScriptRoot '../resources/retrieval/model-bundle.manifest.json'
    return @{ kind = $PackageKind; root = $root; bundle = $bundleDirectory; install = $installDirectory; roots = $roots
        manifest = $manifest; expected = @{ bytes = (Get-Item -LiteralPath $manifest).Length; files = 1 }
        identity = @{ commit = ('a' * 40); run_url = 'https://github.com/example/repo/actions/runs/1' }
        install_root_index = 0; install_status = 'completed'; install_code = 0; dbstat_code = 0; retrieval_code = 0
        retrieval_status = 'completed'; retrieval_stdout = $script:RetrievalSuccess; teardown_code = 0; remove = $true
        corrupt = $false; ambiguous = $false; residue = $false; registration_residue = $false
        teardown_poll_lag = 0; teardown_poll_count = 0
        pre_registration = @(); post_registration = $null; inventory_error_phase = ''; phase = 'preflight'
        calls = [Collections.Generic.List[string]]::new() }
}

function Invoke-MockPackage($Fixture, [int]$ResidueTimeoutMs = 0) {
    $script:Fixture = $Fixture
    return Invoke-PackageSmoke $Fixture.kind $Fixture.bundle $Fixture.install $Fixture.roots $script:TestRoot $Fixture.identity $Fixture.expected $ResidueTimeoutMs
}

try {
    # Exercise the real process runner before replacing it with bounded fixtures.
    $pwsh = (Get-Process -Id $PID).Path
    $actual = Invoke-SmokeProcess $pwsh '-NoProfile -NonInteractive -Command "[Console]::Out.WriteLine(''private fixture output''); exit 27"' 10000 $script:TestRoot
    Assert-Check ($actual.status -eq 'completed' -and $actual.exit_code -eq 27 -and $actual.stdout.Contains('private fixture output')) 'native_exit_capture'
    $actualTimeout = Invoke-SmokeProcess $pwsh '-NoProfile -NonInteractive -Command "Start-Sleep -Seconds 30"' 200 $script:TestRoot
    Assert-Check ($actualTimeout.status -eq 'timeout' -and $null -eq $actualTimeout.exit_code) 'native_bounded_timeout'
    $childPidPath = Join-Path $script:TestRoot 'child-process.txt'
    $treeCommand = '$child = Start-Process -FilePath ' + "'" + $pwsh.Replace("'", "''") + "'" +
        ' -WindowStyle Hidden -PassThru -ArgumentList ''-NoProfile -NonInteractive -Command "Start-Sleep -Seconds 30"''; ' +
        '[IO.File]::WriteAllText(' + "'" + $childPidPath.Replace("'", "''") + "'" + ', [string]$child.Id); Start-Sleep -Seconds 30'
    $encodedTree = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($treeCommand))
    $treeTimeout = Invoke-SmokeProcess $pwsh "-NoProfile -NonInteractive -EncodedCommand $encodedTree" 2500 $script:TestRoot
    Assert-Check ($treeTimeout.status -eq 'timeout' -and (Test-Path -LiteralPath $childPidPath)) 'native_child_was_started'
    $childId = [int][IO.File]::ReadAllText($childPidPath)
    $remainingChild = Get-Process -Id $childId -ErrorAction SilentlyContinue
    if ($null -ne $remainingChild) { $remainingChild.Kill($true) }
    Assert-Check ($null -eq $remainingChild) 'native_entire_process_tree_terminated'
    Assert-Check (@(Get-ChildItem -LiteralPath $script:TestRoot -File -Filter '*.stdout').Count -eq 0) 'private_captures_removed'

    # Exercise the real read-only identity path against a database created in
    # this test's guarded temp root. This never invokes an installer action.
    $identityMsi = Join-Path $script:TestRoot 'identity-only.msi'
    $comInstaller = $null; $comDatabase = $null; $comView = $null
    try {
        $comInstaller = New-Object -ComObject WindowsInstaller.Installer
        $comDatabase = $comInstaller.OpenDatabase($identityMsi, 3)
        $statements = @(
            'CREATE TABLE `Property` (`Property` CHAR(72) NOT NULL, `Value` CHAR(0) LOCALIZABLE PRIMARY KEY `Property`)',
            'INSERT INTO `Property` (`Property`, `Value`) VALUES (''ProductCode'', ''{AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA}'')',
            'INSERT INTO `Property` (`Property`, `Value`) VALUES (''UpgradeCode'', ''{BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB}'')',
            'INSERT INTO `Property` (`Property`, `Value`) VALUES (''ProductName'', ''meetily'')',
            'INSERT INTO `Property` (`Property`, `Value`) VALUES (''Manufacturer'', ''meetily'')'
        )
        foreach ($statement in $statements) {
            $comView = $comDatabase.OpenView($statement)
            $null = $comView.Execute()
            $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($comView); $comView = $null
        }
        $null = $comDatabase.Commit()
    } finally {
        if ($null -ne $comView) { $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($comView) }
        if ($null -ne $comDatabase) { $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($comDatabase) }
        if ($null -ne $comInstaller) { $null = [Runtime.InteropServices.Marshal]::FinalReleaseComObject($comInstaller) }
    }
    $nativeIdentity = Get-MsiPackageIdentity $identityMsi
    Assert-Check ($nativeIdentity.product_code -ceq '{AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA}' -and
        $nativeIdentity.upgrade_code -ceq '{BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB}' -and
        $nativeIdentity.product_name -ceq 'meetily' -and $nativeIdentity.manufacturer -ceq 'meetily') 'native_msi_property_identity'
    $nativeInventoryProbe = 'permission_limited'
    try {
        $unknownProducts = @(Get-WindowsInstallerRegistration '{AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA}')
        $unknownRelated = @(Get-WindowsInstallerRelatedRegistration '{BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB}')
        Assert-Check ($unknownProducts.Count -eq 0 -and $unknownRelated.Count -eq 0) 'native_unknown_product_inventory'
        $nativeInventoryProbe = 'passed'
    } catch {
        # The runner may not be elevated enough for all-user enumeration. The
        # production smoke fails closed; the self-test reports the limitation.
    }
    # This exact-product machine-context query is permission independent. Keep
    # it outside the tolerated all-user probe so a native-null regression fails.
    $unknownLocation = [Text.StringBuilder]::new(32768)
    $unknownLocationLength = [uint32]$unknownLocation.Capacity
    $unknownLocationStatus = [Meetily.WindowsInstallerInventory]::GetProductInstallLocation(
        '{AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA}', [uint32]4, [Text.StringBuilder]::new(0),
        $unknownLocation, [ref]$unknownLocationLength)
    Assert-Check ($unknownLocationStatus -eq 1605) 'native_machine_context_uses_null_sid'

    function Get-SignatureState([string]$Path) { return 'unsigned' }
    function Get-MsiPackageIdentity([string]$Path) {
        Assert-Check ($Path.EndsWith('.msi', [StringComparison]::OrdinalIgnoreCase)) 'msi_is_registration_authority'
        return @{ product_code = '{11111111-1111-1111-1111-111111111111}'
            upgrade_code = '{22222222-2222-2222-2222-222222222222}'; product_name = 'meetily'; manufacturer = 'meetily' }
    }
    function Get-InstallerRegistrationInventory($Identity) {
        $f = $script:Fixture
        if ($f.inventory_error_phase -ceq $f.phase) { throw 'synthetic_inventory_failure' }
        if ($f.phase -eq 'preflight') { return @($f.pre_registration) }
        $actualRoot = $f.roots[$f.install_root_index]
        if ($f.phase -eq 'post_install') {
            if ($null -ne $f.post_registration) { return @($f.post_registration) }
            if ($f.kind -eq 'msi') {
                return @(@{ kind = 'msi_product'; product_code = $Identity.product_code; install_location = $actualRoot },
                    @{ kind = 'msi_related'; product_code = $Identity.product_code },
                    @{ kind = 'nsis_manufacturer'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'; key_name = 'meetily'
                        default_value = ''; install_dir_hint = $actualRoot; active = $true })
            }
            return @(@{ kind = 'nsis_uninstall'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'; key_name = 'meetily'
                    display_name = 'meetily'; publisher = 'meetily'; install_location = "`"$actualRoot`""; uninstall_string = "`"$(Join-Path $actualRoot 'uninstall.exe')`"" },
                @{ kind = 'nsis_manufacturer'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'; key_name = 'meetily'
                    default_value = $actualRoot; active = $true })
        }
        if ($f.phase -eq 'post_teardown' -and $f.teardown_poll_lag -gt 0) {
            $f.teardown_poll_count++
            if ($f.teardown_poll_count -le $f.teardown_poll_lag) {
                return @(@{ kind = 'nsis_uninstall'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'; key_name = 'meetily'
                        install_location = "`"$actualRoot`""; uninstall_string = "`"$(Join-Path $actualRoot 'uninstall.exe')`"" },
                    @{ kind = 'nsis_manufacturer'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'; key_name = 'meetily'
                        default_value = $actualRoot; active = $true })
            }
            if (Test-Path -LiteralPath $actualRoot) {
                Assert-NoReparseAncestors $actualRoot
                Remove-Item -LiteralPath $actualRoot -Recurse -Force
            }
        }
        if ($f.registration_residue) {
            return @(@{ kind = 'msi_product'; product_code = $Identity.product_code; install_location = $actualRoot })
        }
        if ($f.kind -eq 'nsis') {
            return @(@{ kind = 'nsis_manufacturer'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'
                    key_name = 'meetily'; default_value = $actualRoot; active = $false })
        }
        return @()
    }
    function Invoke-SmokeProcess([string]$FilePath, [string]$Arguments, [int]$TimeoutMs, [string]$CaptureRoot) {
        Assert-Check ($null -eq [Environment]::GetEnvironmentVariable('MEETLY_RAG_BUNDLE_DIR', 'Process') -and
            $null -eq [Environment]::GetEnvironmentVariable('MEETLY_RAG_MODELS_DIR', 'Process')) 'overrides_absent_before_launch'
        $f = $script:Fixture
        $reply = @{ status = 'completed'; exit_code = 0; stdout = 'PRIVATE_RAW_CAPTURE'; stderr = ''; capture_valid = $true }
        $actualRoot = $f.roots[$f.install_root_index]
        if ($Arguments.StartsWith('/i ') -or $Arguments.StartsWith('/S /D=')) {
            $f.calls.Add('install')
            $f.phase = 'post_install'
            Assert-Check ($TimeoutMs -eq 300000) 'bounded_install'
            if ($f.kind -eq 'nsis') { Assert-Check ($Arguments -ceq "/S /D=$($f.install)") 'nsis_unquoted_final_destination' }
            New-Item -ItemType Directory -Path (Join-Path $actualRoot 'resources/retrieval/bundle') -Force | Out-Null
            Copy-Item -LiteralPath $f.manifest -Destination (Join-Path $actualRoot 'resources/retrieval/bundle/model-bundle.manifest.json')
            [IO.File]::WriteAllText((Join-Path $actualRoot 'meetily.exe'), 'synthetic executable')
            if ($f.kind -eq 'nsis') { [IO.File]::WriteAllText((Join-Path $actualRoot 'uninstall.exe'), 'synthetic uninstaller') }
            if ($f.corrupt) { [IO.File]::AppendAllText((Join-Path $actualRoot 'resources/retrieval/bundle/model-bundle.manifest.json'), 'corrupt') }
            if ($f.ambiguous) {
                New-Item -ItemType Directory -Path (Join-Path $actualRoot 'other') | Out-Null
                [IO.File]::WriteAllText((Join-Path $actualRoot 'other/meetily.exe'), 'second executable')
            }
            $reply.status = $f.install_status; $reply.exit_code = $f.install_code
        } elseif ($Arguments -eq '--smoke-dbstat') {
            $f.calls.Add('dbstat'); $reply.exit_code = $f.dbstat_code
            Assert-Check ($TimeoutMs -eq 120000 -and $FilePath -ceq (Join-Path $actualRoot 'meetily.exe')) 'dbstat_actual_installed_executable'
        } elseif ($Arguments -eq '--smoke-retrieval') {
            $f.calls.Add('retrieval'); $reply.exit_code = $f.retrieval_code; $reply.status = $f.retrieval_status; $reply.stdout = $f.retrieval_stdout
            Assert-Check ($TimeoutMs -eq 120000 -and $FilePath -ceq (Join-Path $actualRoot 'meetily.exe')) 'retrieval_actual_installed_executable'
        } elseif ($Arguments.StartsWith('/x ') -or $Arguments -eq '/S') {
            $f.calls.Add('uninstall'); $reply.exit_code = $f.teardown_code
            $f.phase = 'post_teardown'
            Assert-Check ($TimeoutMs -eq 300000) 'bounded_uninstall'
            if ($f.remove -and $f.teardown_poll_lag -eq 0) {
                # This models the uninstaller, not the harness's residue check.
                Assert-Check ([IO.Path]::GetFullPath($actualRoot).StartsWith($script:TestRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) 'mock_uninstall_scoped'
                Assert-NoReparseAncestors $actualRoot
                $null = Get-TreeMeasurement $actualRoot
                Remove-Item -LiteralPath $actualRoot -Recurse -Force
            }
            if ($f.residue) {
                New-Item -ItemType Directory -Path $actualRoot -Force | Out-Null
                [IO.File]::WriteAllText((Join-Path $actualRoot 'orphaned-model.onnx'), 'residue without executable')
            }
        } else { throw 'unexpected_mock_invocation' }
        return $reply
    }
    $composedBundleOverride = 'private-development-path-' + [char]0x00E9
    $decomposedBundleOverride = 'private-development-path-e' + [char]0x0301
    Assert-Check (-not [string]::Equals($composedBundleOverride, $decomposedBundleOverride, [StringComparison]::Ordinal)) 'ordinal_override_comparison_is_normalization_sensitive'
    $env:MEETLY_RAG_BUNDLE_DIR = $composedBundleOverride
    $env:MEETLY_RAG_MODELS_DIR = 'private-cache-path'
    $env:DIGICERT_KEYPAIR_ALIAS = $null

    $sharedRoot = Join-Path $script:TestRoot 'shared-hkcu-root'
    $sharedRecords = @()
    foreach ($view in @('Registry32', 'Registry64')) {
        $sharedRecords += @{ kind = 'nsis_uninstall'; product_code = ''; hive = 'CurrentUser'; view = $view; key_name = 'MEETILY'
            default_value = ''; display_name = 'meetily'; publisher = 'meetily'; install_location = "`"$sharedRoot`""
            uninstall_string = "`"$(Join-Path $sharedRoot 'uninstall.exe')`""; active = $true }
        $sharedRecords += @{ kind = 'nsis_manufacturer'; product_code = ''; hive = 'CurrentUser'; view = $view; key_name = 'meetily'
            default_value = $sharedRoot; display_name = ''; publisher = ''; install_location = ''; uninstall_string = ''; active = $true }
    }
    $canonicalShared = @(Get-DistinctInstallerRegistration $sharedRecords)
    Assert-Check ($canonicalShared.Count -eq 2) 'shared_hkcu_dual_views_are_canonicalized'
    $machineView32 = $sharedRecords[0].Clone(); $machineView32.hive = 'LocalMachine'; $machineView32.view = 'Registry32'
    $machineView64 = $sharedRecords[0].Clone(); $machineView64.hive = 'LocalMachine'; $machineView64.view = 'Registry64'
    Assert-Check (@(Get-DistinctInstallerRegistration @($machineView32, $machineView64)).Count -eq 2) 'distinct_hklm_views_are_not_merged'
    $boundaryWix = @{ display_name = 'MEE'; publisher = 'tilymeetily'; uninstall_string = 'C:\tools\prefix-msiexec-helper.exe /x' }
    Assert-Check (Test-MatchingTauriWixRegistration $boundaryWix @{ product_name = 'meetily'; manufacturer = 'meetily' }) 'tauri_wix_concat_boundary_and_case_match'
    $absentHint = @{ default_value = ''; install_dir_hint = (Join-Path $script:TestRoot 'absent-wix-hint') }
    Assert-Check ((Test-ManufacturerRegistrationActive $absentHint 'msi') -and
        -not (Test-ManufacturerRegistrationActive $absentHint 'nsis')) 'absent_target_wix_hint_blocks_only_appsearch'

    $msi = New-MockFixture 'msi-pass'
    $msi.install_root_index = 1
    $pass = Invoke-MockPackage $msi
    Assert-Check ($pass.overall -eq 'passed' -and $pass.discovery.root -eq 'program_files' -and ($msi.calls -join ',') -eq 'install,dbstat,retrieval,uninstall') 'msi_default_root_and_both_diagnostics'
    Assert-Check ([string]::Equals([Environment]::GetEnvironmentVariable('MEETLY_RAG_BUNDLE_DIR', 'Process'), $composedBundleOverride, [StringComparison]::Ordinal) -and
        [string]::Equals([Environment]::GetEnvironmentVariable('MEETLY_RAG_MODELS_DIR', 'Process'), 'private-cache-path', [StringComparison]::Ordinal)) 'overrides_restored'
    Assert-Check ($pass.installer_sha256 -match '^[a-f0-9]{64}$' -and $pass.installer_bytes -gt 0) 'artifact_hash_and_native_size'

    Remove-Item -LiteralPath 'Env:\MEETLY_RAG_BUNDLE_DIR', 'Env:\MEETLY_RAG_MODELS_DIR' -ErrorAction SilentlyContinue
    $absentOverrides = Invoke-MockPackage (New-MockFixture 'absent-overrides')
    Assert-Check ($absentOverrides.overall -eq 'passed' -and
        $null -eq [Environment]::GetEnvironmentVariable('MEETLY_RAG_BUNDLE_DIR', 'Process') -and
        $null -eq [Environment]::GetEnvironmentVariable('MEETLY_RAG_MODELS_DIR', 'Process')) 'absent_overrides_restored_as_absent'

    [Environment]::SetEnvironmentVariable('MEETLY_RAG_BUNDLE_DIR', '', 'Process')
    [Environment]::SetEnvironmentVariable('MEETLY_RAG_MODELS_DIR', '', 'Process')
    $emptyOverrides = Invoke-MockPackage (New-MockFixture 'empty-overrides')
    $restoredEmptyBundle = [Environment]::GetEnvironmentVariable('MEETLY_RAG_BUNDLE_DIR', 'Process')
    $restoredEmptyModels = [Environment]::GetEnvironmentVariable('MEETLY_RAG_MODELS_DIR', 'Process')
    Assert-Check ($emptyOverrides.overall -eq 'passed' -and $null -ne $restoredEmptyBundle -and
        $restoredEmptyBundle.Length -eq 0 -and $null -ne $restoredEmptyModels -and
        $restoredEmptyModels.Length -eq 0) 'empty_overrides_cleared_then_restored_as_empty'

    $nsis = New-MockFixture 'nsis-pass' 'nsis'
    $nsisPass = Invoke-MockPackage $nsis
    Assert-Check ($nsisPass.overall -eq 'passed' -and $nsisPass.residue -eq 'passed') 'nsis_complete_lifecycle'
    $asyncNsis = New-MockFixture 'nsis-async-teardown' 'nsis'
    $asyncNsis.teardown_poll_lag = 2
    $asyncNsisResult = Invoke-MockPackage $asyncNsis 1000
    Assert-Check ($asyncNsisResult.overall -eq 'passed' -and $asyncNsis.teardown_poll_count -eq 3 -and
        $asyncNsisResult.registration.post_teardown -eq 'passed' -and $asyncNsisResult.residue -eq 'passed') 'nsis_registration_and_root_polled_together'
    $staleNsisHint = New-MockFixture 'nsis-stale-location-hint' 'nsis'
    $staleNsisHint.pre_registration = @(@{ kind = 'nsis_manufacturer'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'
            key_name = 'meetily'; default_value = (Join-Path $script:TestRoot 'absent-old-root'); active = $false })
    $staleNsisResult = Invoke-MockPackage $staleNsisHint
    Assert-Check ($staleNsisResult.overall -eq 'passed' -and $staleNsisResult.registration.preflight -eq 'passed') 'inactive_nsis_location_hint_does_not_override_explicit_destination'

    $failure = New-MockFixture 'independent-failures'
    $failure.dbstat_code = 2; $failure.retrieval_code = 27; $failure.teardown_code = 1603; $failure.remove = $false
    $failed = Invoke-MockPackage $failure
    Assert-Check ($failed.dbstat.exit_code -eq 2 -and $failed.retrieval.exit_code -eq 27 -and $failed.teardown.exit_code -eq 1603 -and $failed.residue -eq 'failed' -and $failed.overall -eq 'failed') 'independent_codes_preserved'
    Assert-Check (Test-Path -LiteralPath (Join-Path $failure.install 'meetily.exe')) 'harness_does_not_erase_failed_uninstall'
    $residue = New-MockFixture 'resource-residue'
    $residue.residue = $true
    $leftover = Invoke-MockPackage $residue
    Assert-Check ($leftover.teardown.status -eq 'passed' -and $leftover.residue -eq 'failed' -and $leftover.overall -eq 'failed') 'model_residue_without_executable_rejected'
    Assert-Check (Test-Path -LiteralPath (Join-Path $residue.install 'orphaned-model.onnx')) 'residue_checked_before_any_cleanup'
    $partial = New-MockFixture 'partial-timeout'
    $partial.install_status = 'timeout'; $partial.install_code = $null
    $partialResult = Invoke-MockPackage $partial
    Assert-Check ($partialResult.install.status -eq 'timeout' -and ($partial.calls -join ',') -eq 'install,uninstall' -and $partialResult.teardown.status -eq 'passed') 'partial_install_always_uninstalled'
    $timed = New-MockFixture 'retrieval-timeout'
    $timed.retrieval_status = 'timeout'; $timed.retrieval_code = $null
    $timedResult = Invoke-MockPackage $timed
    Assert-Check ($timedResult.retrieval.status -eq 'timeout' -and $timedResult.teardown.status -eq 'passed' -and $timedResult.overall -eq 'failed') 'diagnostic_timeout_always_uninstalled'
    $corrupt = New-MockFixture 'corrupt-layout'
    $corrupt.corrupt = $true; $corrupt.retrieval_code = 23
    $corruptResult = Invoke-MockPackage $corrupt
    Assert-Check ($corruptResult.resources.status -eq 'failed' -and $corruptResult.retrieval.exit_code -eq 23 -and ($corrupt.calls -join ',') -eq 'install,dbstat,retrieval,uninstall') 'corrupt_installed_manifest_rejected'
    $ambiguous = New-MockFixture 'ambiguous-layout'
    $ambiguous.ambiguous = $true
    $ambiguousResult = Invoke-MockPackage $ambiguous
    Assert-Check ($ambiguousResult.discovery.status -eq 'failed' -and $ambiguousResult.teardown.status -eq 'passed') 'ambiguous_executable_rejected'
    $preexisting = New-MockFixture 'preexisting-install'
    New-Item -ItemType Directory -Path $preexisting.install | Out-Null
    [IO.File]::WriteAllText((Join-Path $preexisting.install 'user-owned.txt'), 'preserve')
    $preexistingResult = Invoke-MockPackage $preexisting
    Assert-Check ($preexistingResult.harness -eq 'preexisting_install' -and $preexisting.calls.Count -eq 0 -and (Test-Path -LiteralPath (Join-Path $preexisting.install 'user-owned.txt'))) 'preexisting_user_state_preserved'

    $customRoot = Join-Path $script:TestRoot 'user-custom-install'
    New-Item -ItemType Directory -Path $customRoot | Out-Null
    [IO.File]::WriteAllText((Join-Path $customRoot 'user-owned.txt'), 'preserve')
    $sameProduct = New-MockFixture 'same-product-registration'
    $sameProduct.pre_registration = @(@{ kind = 'msi_product'; product_code = '{11111111-1111-1111-1111-111111111111}'; install_location = $customRoot })
    $sameProductResult = Invoke-MockPackage $sameProduct
    Assert-Check ($sameProductResult.harness -eq 'preexisting_registration' -and $sameProductResult.registration.preflight -eq 'blocked' -and
        $sameProduct.calls.Count -eq 0 -and (Test-Path -LiteralPath (Join-Path $customRoot 'user-owned.txt'))) 'same_product_custom_root_untouched'
    $related = New-MockFixture 'related-upgrade-registration'
    $related.pre_registration = @(@{ kind = 'msi_related'; product_code = '{33333333-3333-3333-3333-333333333333}' })
    $relatedResult = Invoke-MockPackage $related
    Assert-Check ($relatedResult.harness -eq 'preexisting_registration' -and $related.calls.Count -eq 0) 'related_upgrade_blocks_before_invocation'
    $namedInstallHint = New-MockFixture 'named-install-dir-hint'
    $namedInstallHint.pre_registration = @(@{ kind = 'nsis_manufacturer'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'
            key_name = 'meetily'; default_value = ''; install_dir_hint = $customRoot; active = $true })
    $namedInstallHintResult = Invoke-MockPackage $namedInstallHint
    Assert-Check ($namedInstallHintResult.harness -eq 'preexisting_registration' -and $namedInstallHint.calls.Count -eq 0 -and
        (Test-Path -LiteralPath (Join-Path $customRoot 'user-owned.txt'))) 'wix_named_install_dir_hint_blocks_custom_root_reuse'
    $existingNsis = New-MockFixture 'nsis-custom-registration' 'nsis'
    $existingNsis.pre_registration = @(@{ kind = 'nsis_uninstall'; hive = 'CurrentUser'; view = 'Registry32'; active = $true
            product_code = ''; key_name = 'meetily'; install_location = "`"$customRoot`""; uninstall_string = "`"$(Join-Path $customRoot 'uninstall.exe')`"" },
        @{ kind = 'nsis_manufacturer'; hive = 'CurrentUser'; view = 'Registry32'; active = $true; product_code = ''
            key_name = 'meetily'; default_value = $customRoot })
    $existingNsisResult = Invoke-MockPackage $existingNsis
    Assert-Check ($existingNsisResult.harness -eq 'preexisting_registration' -and $existingNsis.calls.Count -eq 0 -and
        (Test-Path -LiteralPath (Join-Path $customRoot 'user-owned.txt'))) 'nsis_per_user_custom_root_untouched'
    $matchingWix = New-MockFixture 'matching-wix-registration' 'nsis'
    $matchingWix.pre_registration = @(@{ kind = 'wix_uninstall'; product_code = '{44444444-4444-4444-4444-444444444444}' })
    $matchingWixResult = Invoke-MockPackage $matchingWix
    Assert-Check ($matchingWixResult.harness -eq 'preexisting_registration' -and $matchingWix.calls.Count -eq 0) 'nsis_matching_wix_blocks_before_invocation'
    $inventoryError = New-MockFixture 'preflight-inventory-error'
    $inventoryError.inventory_error_phase = 'preflight'
    $inventoryErrorResult = Invoke-MockPackage $inventoryError
    Assert-Check ($inventoryErrorResult.harness -eq 'failed' -and $inventoryErrorResult.registration.preflight -eq 'inspection_failed' -and
        $inventoryError.calls.Count -eq 0) 'registration_inspection_error_fails_closed'
    $postInventoryError = New-MockFixture 'post-install-inventory-error'
    $postInventoryError.inventory_error_phase = 'post_install'
    $postInventoryErrorResult = Invoke-MockPackage $postInventoryError
    Assert-Check ($postInventoryErrorResult.registration.ownership -eq 'inspection_failed' -and
        $postInventoryErrorResult.teardown.status -eq 'cleanup_skipped' -and
        $postInventoryErrorResult.registration.post_teardown -eq 'inspection_failed' -and
        ($postInventoryError.calls -join ',') -eq 'install') 'post_install_inspection_error_never_guesses_cleanup_owner'

    $alreadyInstalled = New-MockFixture 'msi-1638-race'
    $alreadyInstalled.install_code = 1638
    $alreadyInstalled.post_registration = @(@{ kind = 'msi_related'; product_code = '{55555555-5555-5555-5555-555555555555}' })
    $alreadyInstalledResult = Invoke-MockPackage $alreadyInstalled
    Assert-Check ($alreadyInstalledResult.install.exit_code -eq 1638 -and $alreadyInstalledResult.registration.ownership -eq 'unproven' -and
        $alreadyInstalledResult.teardown.status -eq 'cleanup_skipped' -and ($alreadyInstalled.calls -join ',') -eq 'install') 'msi_1638_never_uninstalls_other_product'
    $partialError = New-MockFixture 'msi-owned-partial-error'
    $partialError.install_code = 1603
    $partialErrorResult = Invoke-MockPackage $partialError
    Assert-Check ($partialErrorResult.install.exit_code -eq 1603 -and $partialErrorResult.registration.ownership -eq 'proved' -and
        ($partialError.calls -join ',') -eq 'install,uninstall') 'owned_partial_error_cleanup_attempted'
    $unprovedTimeout = New-MockFixture 'msi-unproved-timeout'
    $unprovedTimeout.install_status = 'timeout'; $unprovedTimeout.install_code = $null
    $unprovedTimeout.post_registration = @()
    $unprovedTimeoutResult = Invoke-MockPackage $unprovedTimeout
    Assert-Check ($unprovedTimeoutResult.registration.ownership -eq 'unproven' -and $unprovedTimeoutResult.teardown.status -eq 'cleanup_skipped' -and
        ($unprovedTimeout.calls -join ',') -eq 'install') 'timeout_without_ownership_skips_cleanup'
    $wrongNsisOwner = New-MockFixture 'nsis-wrong-owner' 'nsis'
    $wrongNsisOwner.post_registration = @(@{ kind = 'nsis_uninstall'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'; key_name = 'meetily'
            install_location = "`"$customRoot`""; uninstall_string = "`"$(Join-Path $customRoot 'uninstall.exe')`"" },
        @{ kind = 'nsis_manufacturer'; product_code = ''; hive = 'CurrentUser'; view = 'Registry64'; key_name = 'meetily'
            default_value = $customRoot; active = $true })
    $wrongNsisOwnerResult = Invoke-MockPackage $wrongNsisOwner
    Assert-Check ($wrongNsisOwnerResult.registration.ownership -eq 'unproven' -and $wrongNsisOwnerResult.teardown.status -eq 'cleanup_skipped' -and
        ($wrongNsisOwner.calls -join ',') -eq 'install' -and (Test-Path -LiteralPath (Join-Path $customRoot 'user-owned.txt'))) 'nsis_custom_root_owner_not_uninstalled'
    $registrationResidue = New-MockFixture 'registration-residue'
    $registrationResidue.registration_residue = $true
    $registrationResidueResult = Invoke-MockPackage $registrationResidue
    Assert-Check ($registrationResidueResult.teardown.status -eq 'passed' -and $registrationResidueResult.residue -eq 'passed' -and
        $registrationResidueResult.registration.post_teardown -eq 'failed' -and $registrationResidueResult.overall -eq 'failed') 'registration_residue_blocks_pass'
    Assert-Check ($nsisPass.registration.auxiliary -eq 'retained_inactive' -and $nsisPass.registration.post_teardown -eq 'passed') 'inactive_nsis_location_hint_reported'

    $capture = @{ status = 'completed'; exit_code = 0; stdout = $script:RetrievalSuccess; stderr = ''; capture_valid = $true }
    Assert-Check ((Convert-DiagnosticResult $capture 'retrieval').status -eq 'passed') 'exact_success_contract'
    foreach ($wrong in @('', ($script:RetrievalSuccess -replace 'sources=2', 'sources=1'), ($script:RetrievalSuccess + "`nPRIVATE_RAW_CAPTURE"))) {
        $capture.stdout = $wrong
        Assert-Check ((Convert-DiagnosticResult $capture 'retrieval').status -eq 'output_contract_failed') 'false_zero_exit_rejected'
    }
    $capture.stdout = $script:RetrievalSuccess; $capture.stderr = 'private stderr'
    Assert-Check ((Convert-DiagnosticResult $capture 'retrieval').status -eq 'output_contract_failed') 'unexpected_stderr_rejected'
    $capture.stderr = ''; $capture.capture_valid = $false
    Assert-Check ((Convert-DiagnosticResult $capture 'retrieval').status -eq 'output_contract_failed') 'oversize_capture_rejected'
    foreach ($code in 20..29) {
        $capture.exit_code = $code
        Assert-Check ((Convert-DiagnosticResult $capture 'retrieval').exit_code -eq $code) 'all_typed_retrieval_codes_preserved'
    }
    $capture.exit_code = -1073741819
    Assert-Check ((Convert-DiagnosticResult $capture 'retrieval').exit_code -eq -1073741819) 'native_crash_code_preserved'

    $summary = Join-Path $script:TestRoot 'summary.md'
    $evidence = Join-Path $script:TestRoot 'result.json'
    Write-SmokeEvidence $pass $evidence $summary
    $publicText = [IO.File]::ReadAllText($evidence) + [IO.File]::ReadAllText($summary)
    Assert-Check (-not $publicText.Contains($script:TestRoot) -and -not $publicText.Contains('PRIVATE_RAW_CAPTURE') -and -not $publicText.Contains('private-development-path')) 'evidence_privacy_whitelist'
    Assert-Check ((Test-EvidenceGate 'success' $evidence 'msi' ('a' * 40)) -eq 'passed') 'current_evidence_pass'
    Assert-Check ((Test-EvidenceGate 'skipped' $evidence 'msi' ('b' * 40)) -eq 'skipped') 'earlier_build_failure_not_relabelled'
    Assert-Check ((Test-EvidenceGate '' $evidence 'msi' ('b' * 40)) -eq 'skipped') 'unexecuted_step_not_relabelled'
    Assert-Check ((Test-EvidenceGate 'success' $evidence 'msi' ('b' * 40)) -eq 'failed') 'stale_commit_rejected'
    Assert-Check ((Test-EvidenceGate 'failure' $evidence 'msi' ('a' * 40)) -eq 'failed') 'step_failure_rejected_despite_old_file'
    Assert-Check ((Test-EvidenceGate 'success' ($evidence + '.missing') 'msi' ('a' * 40)) -eq 'failed') 'missing_executed_evidence_rejected'
    $pass.retrieval.exit_code = 29
    [IO.File]::WriteAllText($evidence, ($pass | ConvertTo-Json -Depth 8))
    Assert-Check ((Test-EvidenceGate 'success' $evidence 'msi' ('a' * 40)) -eq 'failed') 'overall_pass_cannot_hide_diagnostic_failure'
    $pass.retrieval.exit_code = 0
    Assert-Check ((Get-SigningPolicy @{ credentials = 'unavailable'; installer = 'unsigned'; executable = 'unsigned' }) -eq 'passed') 'unchanged_unsigned_policy_with_evidence_limitation'
    Assert-Check ((Get-SigningPolicy @{ credentials = 'unavailable'; installer = 'invalid'; executable = 'valid' }) -eq 'failed') 'known_invalid_signature_rejected'
    Assert-Check ((Convert-SignatureState 'UnknownError') -eq 'invalid' -and
        (Get-SigningPolicy @{ credentials = 'unavailable'; installer = (Convert-SignatureState 'UnknownError'); executable = 'unsigned' }) -eq 'failed') 'unknownerror_is_invalid_signature'
    Assert-Check ((Get-SigningPolicy @{ credentials = 'available'; installer = 'valid'; executable = 'unsigned' }) -eq 'failed') 'configured_signing_requires_both_valid'
    Assert-Check ((Get-SigningPolicy @{ credentials = 'available'; installer = 'valid'; executable = 'valid' }) -eq 'passed') 'valid_signed_pair_pass'

    $emptyResidue = Join-Path $script:TestRoot 'empty-install-residue'
    New-Item -ItemType Directory -Path $emptyResidue | Out-Null
    Assert-Check ((Test-InstallResidue @($emptyResidue) 0) -eq 'failed' -and
        (Test-Path -LiteralPath $emptyResidue)) 'empty_install_directory_is_preserved_failed_residue'
    Assert-Check ((Test-InstallResidue @($emptyResidue + '-absent') 0) -eq 'passed') 'absent_install_roots_pass'

    $fakeWorkspace = Join-Path $script:TestRoot 'build-workspace'
    $target = Join-Path $fakeWorkspace 'upstream/frontend/target'
    $cache = Join-Path $fakeWorkspace 'upstream/target'
    New-Item -ItemType Directory -Path $target, $cache -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $target 'output.bin'), '12345')
    [IO.File]::WriteAllText((Join-Path $cache 'cached.bin'), '123')
    Assert-Check ((Get-TreeMeasurement $target).bytes -eq 5 -and (Get-TreeMeasurement $cache).files -eq 1) 'native_byte_measurements'
    $linkedCacheTarget = Join-Path $script:TestRoot 'linked-rust-cache-target'
    New-Item -ItemType Directory -Path $linkedCacheTarget | Out-Null
    [IO.File]::WriteAllText((Join-Path $linkedCacheTarget 'not-physical-cache.bin'), '1234567')
    $cacheJunction = Join-Path $cache 'linked-cache-entry'
    New-Item -ItemType Junction -Path $cacheJunction -Target $linkedCacheTarget | Out-Null
    $rustMeasurement = Get-TreeMeasurement $cache 'skip'
    Assert-Check ($rustMeasurement.bytes -eq 3 -and $rustMeasurement.files -eq 1 -and $rustMeasurement.reparse_entries -eq 1) 'rust_cache_junction_is_excluded_without_following'
    $rejected = $false
    try { $null = Get-TreeMeasurement $cache } catch { $rejected = $true }
    Assert-Check $rejected 'non_rust_cache_junction_remains_rejected'
    # Remove only the junction itself, never recurse through its target.
    Remove-Item -LiteralPath $cacheJunction -Force
    $rejected = $false
    try { Clear-ExactFrontendBuildOutput $fakeWorkspace $cache } catch { $rejected = $true }
    Assert-Check ($rejected -and (Test-Path -LiteralPath (Join-Path $cache 'cached.bin'))) 'wrong_cleanup_target_rejected'
    $linkedBuildTarget = Join-Path $script:TestRoot 'linked-build-output-target'
    New-Item -ItemType Directory -Path $linkedBuildTarget | Out-Null
    [IO.File]::WriteAllText((Join-Path $linkedBuildTarget 'not-physical-build.bin'), '1234567')
    $buildJunction = Join-Path $target 'linked-build-entry'
    New-Item -ItemType Junction -Path $buildJunction -Target $linkedBuildTarget | Out-Null
    $rejected = $false
    try { Clear-ExactFrontendBuildOutput $fakeWorkspace $target } catch { $rejected = $true }
    Assert-Check ($rejected -and (Test-Path -LiteralPath (Join-Path $target 'output.bin')) -and
        (Test-Path -LiteralPath (Join-Path $linkedBuildTarget 'not-physical-build.bin'))) 'build_cleanup_reparse_rejected_and_preserves_target'
    Remove-Item -LiteralPath $buildJunction -Force
    Clear-ExactFrontendBuildOutput $fakeWorkspace $target
    Assert-Check (-not (Test-Path -LiteralPath $target) -and (Test-Path -LiteralPath $cache)) 'only_exact_frontend_target_cleaned'
    $env:GITHUB_SHA = (& git rev-parse HEAD | Out-String).Trim()
    $env:GITHUB_SERVER_URL = 'https://github.com'; $env:GITHUB_REPOSITORY = 'example/repo'; $env:GITHUB_RUN_ID = '1'
    $env:GITHUB_STEP_SUMMARY = Join-Path $script:TestRoot 'sizes-summary.md'
    $coldMeasurementRoot = Join-Path $script:TestRoot 'cold-miss-measurement'
    $coldWorkspace = Join-Path $coldMeasurementRoot 'workspace'
    $coldTemporary = Join-Path $coldMeasurementRoot 'temporary'
    New-Item -ItemType Directory -Path $coldWorkspace, $coldTemporary -Force | Out-Null
    $env:MODEL_CACHE_PATH = Join-Path $coldMeasurementRoot 'absent-model-cache'; $env:MODEL_CACHE_HIT = $null
    Invoke-SizeEvidence 'before' $coldWorkspace $coldTemporary
    $coldSizes = Get-Content -LiteralPath (Join-Path $coldTemporary 'meetily-package-sizes.json') -Raw | ConvertFrom-Json -AsHashtable
    Assert-Check ($coldSizes.model_cache_restore -eq 'miss' -and $coldSizes.before.model_cache.bytes -eq 0 -and
        $coldSizes.before.model_cache.files -eq 0 -and $coldSizes.before.staged_bundle.files -eq 0 -and
        $coldSizes.before.rust_cache.files -eq 0 -and $coldSizes.before.build_output.files -eq 0) 'cold_miss_absent_cache_and_trees_measure_zero'
    Assert-Check ((Get-SanitizedMeasurementReason ([UnauthorizedAccessException]::new())) -eq 'access_denied' -and
        (Get-SanitizedMeasurementReason ([IO.IOException]::new())) -eq 'io_failure' -and
        (Get-SanitizedMeasurementReason ([Exception]::new('reparse_rejected'))) -eq 'reparse_rejected' -and
        (Get-SanitizedMeasurementReason ([Exception]::new('C:\\private\\path'))) -eq 'unexpected_failure') 'measurement_failure_reason_is_typed_and_private'
    $env:GITHUB_ACTIONS = 'false'
    $boundaryOutput = (& pwsh -NoProfile -File (Join-Path $PSScriptRoot 'windows-package-smoke.ps1') -Mode MeasureBefore 2>&1 | Out-String).Trim()
    $boundaryExit = $LASTEXITCODE
    Assert-Check ($boundaryExit -eq 1 -and
        $boundaryOutput -ceq 'installed-smoke-harness: operation=MeasureBefore status=failed stage=workflow_guard reason=unexpected_failure' -and
        -not $boundaryOutput.Contains($script:TestRoot)) 'measurement_top_level_unknown_error_is_typed_and_private'
    $env:MODEL_CACHE_PATH = Join-Path $fakeWorkspace 'model-cache'; $env:MODEL_CACHE_HIT = 'true'
    New-Item -ItemType Directory -Path $env:MODEL_CACHE_PATH -Force | Out-Null
    $staged = Join-Path $fakeWorkspace 'upstream/frontend/src-tauri/resources/retrieval/bundle'
    New-Item -ItemType Directory -Path $staged -Force | Out-Null
    Copy-Item -LiteralPath $msi.manifest -Destination (Join-Path $staged 'model-bundle.manifest.json')
    New-Item -ItemType Junction -Path $cacheJunction -Target $linkedCacheTarget | Out-Null
    Invoke-SizeEvidence 'before' $fakeWorkspace $script:TestRoot
    # Metadata remains readable while this native handle denies deletion.
    # Exercise the real top-level cleanup error, not only the stage helper.
    New-Item -ItemType Directory -Path $target -Force | Out-Null
    $lockedOutput = Join-Path $target 'locked-output.bin'
    [IO.File]::WriteAllText($lockedOutput, '123')
    $outputLock = [IO.File]::Open($lockedOutput, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        $env:GITHUB_ACTIONS = 'true'; $env:GITHUB_WORKSPACE = $fakeWorkspace; $env:RUNNER_TEMP = $script:TestRoot
        $cleanupOutput = (& pwsh -NoProfile -File (Join-Path $PSScriptRoot 'windows-package-smoke.ps1') -Mode PrepareBuild 2>&1 | Out-String).Trim()
        $cleanupExit = $LASTEXITCODE
        Assert-Check ($cleanupExit -eq 1 -and
            $cleanupOutput -match '^installed-smoke-harness: operation=PrepareBuild status=failed stage=clear_build_output reason=(access_denied|io_failure|unexpected_failure)$' -and
            -not $cleanupOutput.Contains($script:TestRoot)) 'prepare_build_cleanup_failure_has_its_own_private_stage'
        Assert-Check (Test-Path -LiteralPath $lockedOutput -PathType Leaf) 'failed_build_cleanup_does_not_claim_removal'
    } finally { $outputLock.Dispose() }
    Clear-ExactFrontendBuildOutput $fakeWorkspace $target
    Invoke-SizeEvidence 'prepare_build' $fakeWorkspace $script:TestRoot
    New-Item -ItemType Directory -Path $target -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $target 'new-output.bin'), '12345')
    [IO.File]::WriteAllText((Join-Path $env:MODEL_CACHE_PATH 'new-cache.bin'), '1234567')
    New-Item -ItemType Junction -Path $buildJunction -Target $linkedBuildTarget | Out-Null
    Invoke-SizeEvidence 'after' $fakeWorkspace $script:TestRoot
    $sizesPath = Join-Path $script:TestRoot 'meetily-package-sizes.json'
    $sizes = Get-Content -LiteralPath $sizesPath -Raw | ConvertFrom-Json -AsHashtable
    Assert-Check ($sizes.model_cache_restore -eq 'exact_hit' -and $sizes.model_cache_delta_bytes -eq 7 -and
        $sizes.rust_cache_delta_bytes -eq 0 -and $sizes.build_output_delta_bytes -eq 5 -and
        $sizes.before.rust_cache.reparse_entries -eq 1 -and $sizes.after.rust_cache.reparse_entries -eq 1 -and
        $sizes.before.build_output.reparse_entries -eq 0 -and $sizes.prepare_build.build_output.reparse_entries -eq 0 -and
        $sizes.after.build_output.bytes -eq 5 -and $sizes.after.build_output.files -eq 1 -and $sizes.after.build_output.reparse_entries -eq 1 -and
        $sizes.after.staged_bundle.bytes -eq $msi.expected.bytes -and $sizes.manifest_sha256 -ceq $script:ManifestDigest) 'native_size_phases_and_cache_hit'
    Assert-Check (-not [IO.File]::ReadAllText($sizesPath).Contains($script:TestRoot) -and
        -not [IO.File]::ReadAllText($env:GITHUB_STEP_SUMMARY).Contains($script:TestRoot)) 'measurement_paths_private'
    Remove-Item -LiteralPath $cacheJunction -Force
    Remove-Item -LiteralPath $buildJunction -Force
    $junction = Join-Path $fakeWorkspace 'junction'
    New-Item -ItemType Junction -Path $junction -Target $cache | Out-Null
    $rejected = $false
    try { $null = Get-TreeMeasurement $junction } catch { $rejected = $true }
    Assert-Check $rejected 'reparse_measurement_rejected'
    # Remove only the junction itself, never recurse through its target.
    Remove-Item -LiteralPath $junction -Force
    Write-Host "installed-smoke-selftest: status=passed assertions=$script:Checks native_inventory_probe=$nativeInventoryProbe native_installer_evidence=false"
} catch {
    $reason = if ($_.Exception.Message -match '^selftest_failed:[a-z_]+$') { $_.Exception.Message } else { $_.Exception.GetType().Name }
    Write-Host "installed-smoke-selftest: status=failed reason=$reason"
    exit 1
} finally {
    ${function:Invoke-SmokeProcess} = $originalProcess
    ${function:Get-SignatureState} = $originalSignature
    ${function:Get-MsiPackageIdentity} = $originalMsiIdentity
    ${function:Get-InstallerRegistrationInventory} = $originalRegistrationInventory
    $env:MEETLY_RAG_BUNDLE_DIR = $originalBundleOverride
    $env:MEETLY_RAG_MODELS_DIR = $originalModelsOverride
    $env:DIGICERT_KEYPAIR_ALIAS = $originalSigning
    foreach ($name in $measurementEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $measurementEnvironment[$name], 'Process') }
    $resolved = [IO.Path]::GetFullPath($script:TestRoot)
    if (-not $resolved.StartsWith($temporaryParent.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolved) -notmatch '^meetily-package-smoke-selftest-[a-f0-9]{32}$') { throw 'selftest_cleanup_target_rejected' }
    Assert-NoReparseAncestors $resolved
    $null = Get-TreeMeasurement $resolved
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
