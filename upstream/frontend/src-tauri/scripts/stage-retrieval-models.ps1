# Task 1.R2 hardening of the Task 1.5 staging pipeline: fetch pinned retrieval
# model artifacts into a build cache, verify byte length + SHA-256, stage the
# complete package outside Tauri resources, publish it atomically for
# `tauri build`, and recover a crashed publication before any cleanup.
#
# Package authority: `resources\retrieval\bundle` is the ONLY packaged
# retrieval resource (tauri.conf.json). The checked-in manifest and licenses
# beside it are build inputs and provenance sources; they are never signed
# package content themselves. A staged or published bundle may contain only
# manifest-managed artifacts, the manifest copy itself, and the pinned
# README.md placeholder - anything else fails the run.
#
# Run `-SelfTest` for the offline proof of crash recovery and package-
# integrity rejection (temp dirs only, no network, no model downloads).
#
# ponytail: publication swaps two same-volume renames; a crash between them can
# leave `.bundle-backup-*` behind with no `bundle`. Recovery restores a sole
# backup before cleanup and refuses ambiguity; upgrade to a journal only if
# concurrent publishers ever exist.
param(
    [string]$CacheRoot = (Join-Path $env:LOCALAPPDATA "meetily\model-cache"),
    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

if ($args.Count -gt 0) {
    throw 'unsupported argument: publication manifest authority is fixed to the checked-in manifest'
}

$scriptPath = $MyInvocation.MyCommand.Path
$scriptDir = Split-Path -Parent $scriptPath
$srcTauriDir = Split-Path -Parent $scriptDir
$ManifestPath = Join-Path $srcTauriDir "resources\retrieval\model-bundle.manifest.json"
$retDir = Join-Path $srcTauriDir "resources\retrieval"
$finalDir = Join-Path $retDir "bundle"
$manifestFileName = "model-bundle.manifest.json"
# Committed non-artifact placeholder retained across publications so fresh
# clones keep the Tauri resource directory non-empty in Git. Its bytes are
# pinned here: it is the only packaged file outside the manifest, so a stale
# or tampered copy fails closed instead of being silently repackaged. Update
# this pin in the same change as the committed file.
$allowedExtraFiles = @{
    "README.md" = @{
        byteLength = 584
        sha256     = "119cf95349bb32494a5d6d2b42ec9a7a8132e09c1bfdafb21aaeadb43d3c2fca"
    }
}

function Get-Sha256([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-ManifestEntry($Entry) {
    if ([string]::IsNullOrWhiteSpace([string]$Entry.path)) { throw "artifact entry without path" }
    $segments = @($Entry.path.Split([char]'/', [System.StringSplitOptions]::None))
    $unsafeSegments = @($segments | Where-Object { $_ -eq '' -or $_ -eq '.' -or $_ -eq '..' })
    if ($Entry.path -match '[\\:]' -or $Entry.path.StartsWith('/') -or
        $unsafeSegments.Count -gt 0) {
        throw "unsafe artifact path '$($Entry.path)'"
    }
    if ($Entry.sha256 -notmatch '^[0-9a-f]{64}$') { throw "malformed SHA-256 for '$($Entry.path)'" }
    if (-not $Entry.byteLength -or [int64]$Entry.byteLength -le 0) { throw "missing byteLength for '$($Entry.path)'" }
}

function Assert-ManifestEntries([object[]]$Entries) {
    if ($Entries.Count -eq 0) { throw "manifest declares no artifacts" }
    $seen = @{}
    foreach ($entry in $Entries) {
        Assert-ManifestEntry $entry
        $key = $entry.path.ToLowerInvariant()
        if ($seen.ContainsKey($key)) {
            throw "duplicate artifact path(s): $($entry.path)"
        }
        $seen[$key] = $true
    }
}

# Tauri's own ResourcePaths iterator is the single mapping authority.
# The helper has no app/sidecar dependency and is built before any cache/network
# work. Static validation permits a missing bundle during verified recovery.
function Assert-TauriResourceContract([switch]$StaticOnly) {
    $helperManifest = Join-Path $srcTauriDir '..\..\tools\retrieval-package-contract\Cargo.toml'
    $config = Join-Path $srcTauriDir 'tauri.conf.json'
    $helperArgs = @('run', '--quiet', '--locked', '--manifest-path', $helperManifest, '--')
    if ($StaticOnly) { $helperArgs += '--static' }
    $helperArgs += $config
    & cargo @helperArgs
    if ($LASTEXITCODE -ne 0) { throw 'Tauri retrieval resource contract rejected' }
}

function Get-ManagedRelativePaths($Entries, [hashtable]$AllowedExtra) {
    @(
        @($Entries | ForEach-Object { $_.path.Replace('/', '\') }) +
        $manifestFileName +
        @($AllowedExtra.Keys)
    )
}

function Get-UnexpectedFiles([string]$Dir, [string[]]$ManagedRelative) {
    if (-not (Test-Path -LiteralPath $Dir)) { return @() }
    # Windows paths are case-insensitive; match the file system's rules.
    $managed = @{}
    $managedDirectories = @{}
    foreach ($m in $ManagedRelative) {
        $normalized = $m.Replace('/', '\')
        $managed[$normalized.ToLowerInvariant()] = $true
        $parent = Split-Path -Parent $normalized
        while ($parent -and $parent -ne '.') {
            $managedDirectories[$parent.ToLowerInvariant()] = $true
            $parent = Split-Path -Parent $parent
        }
    }
    $unexpected = @()
    Get-ChildItem -LiteralPath $Dir -Recurse -Force | ForEach-Object {
        $relative = $_.FullName.Substring($Dir.Length + 1)
        $key = $relative.Replace('/', '\').ToLowerInvariant()
        if (($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            $unexpected += $relative
        } elseif ($_.PSIsContainer) {
            if (-not $managedDirectories.ContainsKey($key)) { $unexpected += $relative }
        } elseif (-not $managed.ContainsKey($key)) {
            $unexpected += $relative
        }
    }
    return ,@($unexpected)
}

function Assert-PackageIntegrity([string]$Dir, [object[]]$Entries, [string]$ManifestSourcePath, [hashtable]$AllowedExtra, [string]$Label) {
    # Single package-integrity gate shared by recovery, staging, and
    # post-publish: nothing becomes or stays `bundle` unless all of these hold.
    $dirItem = Get-Item -LiteralPath $Dir
    if (-not $dirItem.PSIsContainer -or ($dirItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw ("{0} is not a regular directory" -f $Label)
    }
    foreach ($entry in $Entries) {
        $path = Join-Path $Dir ($entry.path.Replace('/', '\'))
        if (-not (Test-Path -LiteralPath $path)) {
            throw ("{0} is missing manifest-managed artifact '{1}'" -f $Label, $entry.path)
        }
        $item = Get-Item -LiteralPath $path
        if ($item.PSIsContainer -or ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw ("{0} artifact '{1}' is not a regular file" -f $Label, $entry.path)
        }
        $actual = $item.Length
        if ($actual -ne [int64]$entry.byteLength) {
            throw ("{0} byte length mismatch for '{1}': expected {2}, got {3}" -f $Label, $entry.path, $entry.byteLength, $actual)
        }
        if ((Get-Sha256 $path) -ne $entry.sha256) {
            throw ("{0} SHA-256 mismatch for '{1}'" -f $Label, $entry.path)
        }
    }
    $manifestCopy = Join-Path $Dir $manifestFileName
    if (-not (Test-Path -LiteralPath $manifestCopy)) {
        throw ("{0} is missing its '{1}' copy" -f $Label, $manifestFileName)
    }
    $manifestItem = Get-Item -LiteralPath $manifestCopy
    if ($manifestItem.PSIsContainer -or ($manifestItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw ("{0} manifest copy is not a regular file" -f $Label)
    }
    if ((Get-Sha256 $manifestCopy) -ne (Get-Sha256 $ManifestSourcePath)) {
        throw ("{0} holds a '{1}' that is not byte-identical to the checked-in publication manifest" -f $Label, $manifestFileName)
    }
    foreach ($extra in $AllowedExtra.Keys) {
        $path = Join-Path $Dir $extra
        if (-not (Test-Path -LiteralPath $path)) {
            throw ("{0} is missing required committed placeholder '{1}'" -f $Label, $extra)
        }
        $extraItem = Get-Item -LiteralPath $path
        if ($extraItem.PSIsContainer -or ($extraItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
            $extraItem.Length -ne [int64]$AllowedExtra[$extra].byteLength -or
            (Get-Sha256 $path) -ne $AllowedExtra[$extra].sha256) {
            throw ("{0} holds a '{1}' that does not match the pinned committed placeholder; restore the committed file (git checkout -- it) and rerun." -f $Label, $extra)
        }
    }
    $unexpected = Get-UnexpectedFiles -Dir $Dir -ManagedRelative (Get-ManagedRelativePaths $Entries $AllowedExtra)
    if ($unexpected.Count -gt 0) {
        throw ("{0} contains unexpected unmanifested file(s): {1}. Delete the listed stale file(s) from the previous bundle and rerun; arbitrary prior-bundle files are never copied into the signed package." -f $Label, ($unexpected -join ', '))
    }
}

function Assert-AllowedExtraFiles([string]$Dir, [hashtable]$AllowedExtra, [string]$Label) {
    foreach ($extra in $AllowedExtra.Keys) {
        $path = Join-Path $Dir $extra
        if (-not (Test-Path -LiteralPath $path)) {
            throw ("{0} is missing required committed placeholder '{1}'" -f $Label, $extra)
        }
        $item = Get-Item -LiteralPath $path
        if ($item.PSIsContainer -or ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
            $item.Length -ne [int64]$AllowedExtra[$extra].byteLength -or
            (Get-Sha256 $path) -ne $AllowedExtra[$extra].sha256) {
            throw ("{0} holds a '{1}' that does not match the pinned committed placeholder" -f $Label, $extra)
        }
    }
}

function Assert-PriorBundle([string]$Dir, [object[]]$Entries, [string]$ManifestSourcePath, [hashtable]$AllowedExtra, [string]$Label) {
    $item = Get-Item -LiteralPath $Dir
    if (-not $item.PSIsContainer -or ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw ("{0} is not a regular directory" -f $Label)
    }
    $managedPresent = @($Entries | Where-Object {
            Test-Path -LiteralPath (Join-Path $Dir ($_.path.Replace('/', '\')))
        }).Count -gt 0
    $manifestPresent = Test-Path -LiteralPath (Join-Path $Dir $manifestFileName)
    if ($managedPresent -or $manifestPresent) {
        Assert-PackageIntegrity -Dir $Dir -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label $Label
        return 'package'
    }
    Assert-AllowedExtraFiles -Dir $Dir -AllowedExtra $AllowedExtra -Label $Label
    $unexpected = Get-UnexpectedFiles -Dir $Dir -ManagedRelative (Get-ManagedRelativePaths @() $AllowedExtra)
    if ($unexpected.Count -gt 0) {
        throw ("{0} contains unexpected unmanifested file(s): {1}" -f $Label, ($unexpected -join ', '))
    }
    return 'placeholder'
}

function Restore-CrashedPublication([string]$RetrievalDir, [string]$BundleDir, [object[]]$Entries, [string]$ManifestSourcePath, [hashtable]$AllowedExtra) {
    # Runs BEFORE stale-dir cleanup so the only recoverable backup survives.
    $bundleExists = Test-Path -LiteralPath $BundleDir
    $backups = @(Get-ChildItem -LiteralPath $RetrievalDir -Force -Filter ".bundle-backup-*" -ErrorAction SilentlyContinue)
    if ($backups.Count -gt 1) {
        throw ("bundle is missing and {0} .bundle-backup-* directories exist ({1}); refusing ambiguous recovery" -f $backups.Count, (($backups | ForEach-Object { $_.Name }) -join ', '))
    }
    if ($bundleExists) {
        $bundleState = Assert-PriorBundle -Dir $BundleDir -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label 'published bundle'
        if ($backups.Count -eq 0) { return }
        $backupState = Assert-PriorBundle -Dir $backups[0].FullName -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label "recoverable backup $($backups[0].Name)"
        if ($bundleState -eq 'placeholder' -and $backupState -eq 'package') {
            Remove-Item -LiteralPath $BundleDir -Recurse -Force
            Rename-Item -LiteralPath $backups[0].FullName -NewName (Split-Path -Leaf $BundleDir)
            Assert-PriorBundle -Dir $BundleDir -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label 'recovered bundle' | Out-Null
        } else {
            Remove-Item -LiteralPath $backups[0].FullName -Recurse -Force
        }
        return
    }
    if ($backups.Count -eq 0) { return }
    $backupState = Assert-PriorBundle -Dir $backups[0].FullName -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label "recoverable backup $($backups[0].Name)"
    Rename-Item -LiteralPath $backups[0].FullName -NewName (Split-Path -Leaf $BundleDir)
    Write-Host "recovered : restored previous bundle from $($backups[0].Name)"
}

function Publish-Package([string]$RetrievalDir, [string]$BundleDir, [string]$StagingDir, [object[]]$Entries, [string]$ManifestSourcePath, [hashtable]$AllowedExtra, [switch]$ForceFinalValidationFailure, [scriptblock]$ValidatePublishedResources) {
    $backupDir = $null
    $priorState = $null
    $publishedNew = $false
    try {
        if (Test-Path -LiteralPath $BundleDir) {
            $priorState = Assert-PriorBundle -Dir $BundleDir -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label 'prior bundle'
            $backupDir = Join-Path $RetrievalDir ('.bundle-backup-' + [guid]::NewGuid().ToString('N'))
            Rename-Item -LiteralPath $BundleDir -NewName (Split-Path -Leaf $backupDir)
            Assert-PriorBundle -Dir $backupDir -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label 'verified prior bundle backup' | Out-Null
        }
        Rename-Item -LiteralPath $StagingDir -NewName 'bundle'
        $publishedNew = $true
        if ($ForceFinalValidationFailure) {
            $target = Join-Path $BundleDir ($Entries[0].path.Replace('/', '\'))
            $bytes = [System.IO.File]::ReadAllBytes($target)
            $bytes[0] = [byte]($bytes[0] -bxor 1)
            [System.IO.File]::WriteAllBytes($target, $bytes)
        }
        Assert-PackageIntegrity -Dir $BundleDir -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label 'published bundle'
        # Keep the verified backup until every final package gate passes.
        if ($ValidatePublishedResources) { & $ValidatePublishedResources }
    } catch {
        $publicationError = $_.Exception
        try {
            if ($publishedNew -and (Test-Path -LiteralPath $BundleDir)) {
                Remove-Item -LiteralPath $BundleDir -Recurse -Force
            }
            if ($backupDir -and (Test-Path -LiteralPath $backupDir)) {
                Rename-Item -LiteralPath $backupDir -NewName (Split-Path -Leaf $BundleDir)
                if ($priorState -eq 'package') {
                    Assert-PackageIntegrity -Dir $BundleDir -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label 'restored prior bundle'
                } else {
                    Assert-PriorBundle -Dir $BundleDir -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label 'restored prior bundle' | Out-Null
                }
            }
        } catch {
            throw ("publication failed and rollback failed: {0}; rollback error: {1}" -f $publicationError.Message, $_.Exception.Message)
        }
        throw $publicationError
    }
    if ($backupDir -and (Test-Path -LiteralPath $backupDir)) {
        Remove-Item -LiteralPath $backupDir -Recurse -Force
    }
}

function Invoke-SelfTest {
    # Offline proof of the 1.R2 behaviors using only temp directories and
    # self-defined fixtures, never the real network or model files.
    $root = Join-Path ([System.IO.Path]::GetTempPath()) ("stage-selftest-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    try {
        # Fixture authority: a seed "checked-in" manifest, two seed artifacts,
        # and a seed README whose bytes define the self-test placeholder pin.
        Set-Content -LiteralPath (Join-Path $root "authority.manifest.json") -Value '{"bundleId":"selftest"}'
        foreach ($seed in @("seeds\models\e.onnx", "seeds\tokenizers\t.json")) {
            $p = Join-Path $root $seed
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $p) | Out-Null
            Set-Content -LiteralPath $p -Value ("bytes-of-" + (Split-Path -Leaf $p))
        }
        Set-Content -LiteralPath (Join-Path $root ".pin-seed") -Value "placeholder"
        $authority = Join-Path $root "authority.manifest.json"
        $entries = @(
            [pscustomobject]@{ path = "models/e.onnx"; byteLength = (Get-Item (Join-Path $root "seeds\models\e.onnx")).Length; sha256 = (Get-Sha256 (Join-Path $root "seeds\models\e.onnx")) },
            [pscustomobject]@{ path = "tokenizers/t.json"; byteLength = (Get-Item (Join-Path $root "seeds\tokenizers\t.json")).Length; sha256 = (Get-Sha256 (Join-Path $root "seeds\tokenizers\t.json")) }
        )
        $extras = @{
            "README.md" = @{
                byteLength = (Get-Item (Join-Path $root ".pin-seed")).Length
                sha256     = (Get-Sha256 (Join-Path $root ".pin-seed"))
            }
        }

        $externalManifest = Join-Path $root 'external-manifest.json'
        Set-Content -LiteralPath $externalManifest -Value '{"bundleId":"external-mutant"}'
        $probeCache = Join-Path $root 'external-cache'
        $probeErrorAction = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $probeOutput = (& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $scriptPath -ManifestPath $externalManifest -CacheRoot $probeCache 2>&1 | Out-String)
            $probeExitCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $probeErrorAction
        }
        if ($probeExitCode -eq 0 -or $probeOutput -notmatch '(?i)unsupported argument') {
            throw 'selftest: external manifest override was not rejected before packaging'
        }
        if (Test-Path -LiteralPath $probeCache) {
            throw 'selftest: rejected external manifest invocation touched the cache'
        }
        Write-Host 'selftest ok : external manifest override rejected before packaging'

        # Filesystem-backed regression fixtures execute the real Tauri iterator,
        # including junction aliases, map suffixes and crash-state expansion.
        $helperManifest = Join-Path $srcTauriDir '..\..\tools\retrieval-package-contract\Cargo.toml'
        & cargo test --quiet --offline --locked --manifest-path $helperManifest
        if ($LASTEXITCODE -ne 0) { throw 'selftest: Tauri resource expansion regressions failed' }
        Write-Host 'selftest ok : authoritative Tauri expansion regressions passed'

        function New-SelfTestPackage([string]$Dir) {
            foreach ($entry in $entries) {
                $target = Join-Path $Dir ($entry.path.Replace('/', '\'))
                New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
                Copy-Item -LiteralPath (Join-Path $root ("seeds\" + ($entry.path.Replace('/', '\')))) -Destination $target
            }
            Copy-Item -LiteralPath $authority -Destination (Join-Path $Dir $manifestFileName)
            foreach ($extra in $extras.Keys) {
                Copy-Item -LiteralPath (Join-Path $root ".pin-seed") -Destination (Join-Path $Dir $extra)
            }
        }

        # Restoring must fail closed on $What and preserve the sole backup.
        function Assert-RecoveryRejected([string]$RetrievalDir, [string]$Backup, [string]$Pattern, [string]$What) {
            $failed = $false
            try {
                Restore-CrashedPublication -RetrievalDir $RetrievalDir -BundleDir (Join-Path $RetrievalDir "bundle") -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras
            } catch {
                $failed = ($_.Exception.Message -like $Pattern)
            }
            if (-not $failed) { throw "selftest: $What did not fail closed" }
            if (-not (Test-Path -LiteralPath $Backup)) {
                throw "selftest: recoverable backup was deleted during $What"
            }
        }

        $ret0 = Join-Path $root 'ret0'
        $placeholder0 = Join-Path $ret0 'bundle'
        New-Item -ItemType Directory -Force -Path $placeholder0 | Out-Null
        Copy-Item -LiteralPath (Join-Path $root '.pin-seed') -Destination (Join-Path $placeholder0 'README.md')
        if ((Assert-PriorBundle -Dir $placeholder0 -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label 'placeholder bundle') -ne 'placeholder') {
            throw 'selftest: committed placeholder was not recognized as a safe prior state'
        }
        Write-Host 'selftest ok : committed placeholder prior state accepted'

        # 1. A sole intact backup with no bundle directory is fully verified
        #    (artifacts, manifest-copy identity, README pin) then restored.
        $ret1 = Join-Path $root "ret1"
        New-SelfTestPackage (Join-Path $ret1 ".bundle-backup-abc")
        Restore-CrashedPublication -RetrievalDir $ret1 -BundleDir (Join-Path $ret1 "bundle") -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras
        foreach ($rel in @("models\e.onnx", $manifestFileName, "README.md")) {
            if (-not (Test-Path -LiteralPath (Join-Path $ret1 "bundle\$rel"))) {
                throw "selftest: sole backup was not restored ($rel missing)"
            }
        }
        if (@(Get-ChildItem $ret1 -Directory -Filter ".bundle-backup-*").Count -ne 0) {
            throw "selftest: backup was not consumed by recovery"
        }
        Write-Host "selftest ok : sole-backup crash recovery restores the previous bundle"

        # 2. Unexpected content in the recoverable backup fails closed.
        $ret2 = Join-Path $root "ret2"
        $backup2 = Join-Path $ret2 ".bundle-backup-def"
        New-SelfTestPackage $backup2
        Set-Content -LiteralPath (Join-Path $backup2 "foreign.bin") -Value "x"
        Assert-RecoveryRejected $ret2 $backup2 "*unexpected unmanifested file*" "backup with unexpected content"
        Write-Host "selftest ok : unexpected backup content rejected; backup preserved"

        # 3. A backup missing a manifest-managed artifact fails closed.
        $ret3 = Join-Path $root "ret3"
        $backup3 = Join-Path $ret3 ".bundle-backup-missing"
        New-SelfTestPackage $backup3
        Remove-Item -LiteralPath (Join-Path $backup3 "tokenizers\t.json")
        Assert-RecoveryRejected $ret3 $backup3 "*missing manifest-managed artifact*" "backup missing a managed artifact"
        Write-Host "selftest ok : backup missing managed artifact rejected; backup preserved"

        # 4. A backup with a corrupted managed artifact (same length, altered
        #    bytes) fails closed on its hash.
        $ret4 = Join-Path $root "ret4"
        $backup4 = Join-Path $ret4 ".bundle-backup-corrupt"
        New-SelfTestPackage $backup4
        Set-Content -LiteralPath (Join-Path $backup4 "models\e.onnx") -Value "bytes-of-X.onnx"
        Assert-RecoveryRejected $ret4 $backup4 "*SHA-256 mismatch*" "backup with corrupted managed artifact"
        Write-Host "selftest ok : corrupted managed artifact rejected; backup preserved"

        # 5. The committed README is required package content, not optional:
        #    a backup without it fails closed.
        $ret5 = Join-Path $root "ret5"
        $backup5 = Join-Path $ret5 ".bundle-backup-noreadme"
        New-SelfTestPackage $backup5
        Remove-Item -LiteralPath (Join-Path $backup5 "README.md")
        Assert-RecoveryRejected $ret5 $backup5 "*missing required committed placeholder*" "backup without README"
        Write-Host "selftest ok : backup without required README rejected; backup preserved"

        # 6. Multiple backups without a bundle refuse ambiguous recovery.
        $ret6 = Join-Path $root "ret6"
        foreach ($name in @(".bundle-backup-a", ".bundle-backup-b")) {
            $p = Join-Path $root "ret6\$name"
            New-Item -ItemType Directory -Force -Path $p | Out-Null
            Set-Content -LiteralPath (Join-Path $p $manifestFileName) -Value "{}"
        }
        $failed = $false
        try {
            Restore-CrashedPublication -RetrievalDir $ret6 -BundleDir (Join-Path $ret6 "bundle") -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras
        } catch {
            $failed = ($_.Exception.Message -like "*refusing ambiguous recovery*")
        }
        if (-not $failed) { throw "selftest: ambiguous backups did not fail closed" }
        Write-Host "selftest ok : ambiguous backups refused"

        # 7. Staging control: the complete clean package passes full integrity.
        $stage = Join-Path $root "stage"
        New-SelfTestPackage $stage
        Assert-PackageIntegrity -Dir $stage -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label "staged package"

        $reparseTarget = Join-Path $root 'reparse-target'
        $reparsePath = Join-Path $stage 'reparse-dir'
        New-Item -ItemType Directory -Force -Path $reparseTarget | Out-Null
        Set-Content -LiteralPath (Join-Path $reparseTarget 'outside.txt') -Value 'outside'
        try {
            New-Item -ItemType Junction -Path $reparsePath -Target $reparseTarget -ErrorAction Stop | Out-Null
            $reparseItem = Get-Item -LiteralPath $reparsePath
            if (($reparseItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq 0) {
                throw 'junction was not a reparse point'
            }
        } catch {
            throw 'selftest setup: Windows junction could not be created for reparse coverage'
        }
        $failed = $false
        try {
            Assert-PackageIntegrity -Dir $stage -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label 'staged package'
        } catch {
            $failed = ($_.Exception.Message -like '*reparse-dir*')
        }
        & cmd.exe /c "rmdir /s /q `"$reparsePath`"" | Out-Null
        if (Test-Path -LiteralPath $reparsePath) { throw 'selftest: reparse directory cleanup failed' }
        if (-not $failed) { throw 'selftest: reparse directory was not rejected' }
        Write-Host 'selftest ok : reparse directory rejected before publication'

        # 8. An unmanifested staged file is rejected...
        Set-Content -LiteralPath (Join-Path $stage "stale-extra.onnx") -Value "x"
        $failed = $false
        try {
            Assert-PackageIntegrity -Dir $stage -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label "staged package"
        } catch {
            $failed = ($_.Exception.Message -like "*stale-extra.onnx*")
        }
        if (-not $failed) { throw "selftest: unmanifested staged file was not rejected" }

        Remove-Item -LiteralPath (Join-Path $stage "stale-extra.onnx")
        New-Item -ItemType Directory -Force -Path (Join-Path $stage "stale-dir") | Out-Null
        $failed = $false
        try {
            Assert-PackageIntegrity -Dir $stage -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label "staged package"
        } catch {
            $failed = ($_.Exception.Message -like "*stale-dir*")
        }
        if (-not $failed) { throw "selftest: unmanifested staged directory was not rejected" }

        Remove-Item -LiteralPath (Join-Path $stage "stale-dir") -Recurse
        Set-Content -LiteralPath (Join-Path $stage "README.md") -Value "tampered!!"
        $failed = $false
        try {
            Assert-PackageIntegrity -Dir $stage -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label "staged package"
        } catch {
            $failed = ($_.Exception.Message -like "*pinned committed placeholder*")
        }
        if (-not $failed) { throw "selftest: tampered placeholder was not rejected" }

        Set-Content -LiteralPath (Join-Path $stage "README.md") -Value "placeholder"
        Set-Content -LiteralPath (Join-Path $stage $manifestFileName) -Value '{"bundleId":"other"}'
        $failed = $false
        try {
            Assert-PackageIntegrity -Dir $stage -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label "staged package"
        } catch {
            $failed = ($_.Exception.Message -like "*byte-identical to the checked-in publication manifest*")
        }
        if (-not $failed) { throw "selftest: divergent manifest copy was not rejected" }
        foreach ($unsafe in @("models/../e.onnx", "models//e.onnx", "./models/e.onnx", "models\e.onnx", "/models/e.onnx")) {
            $failed = $false
            try {
                Assert-ManifestEntry ([pscustomobject]@{ path = $unsafe; byteLength = 1; sha256 = ("0" * 64) })
            } catch {
                $failed = ($_.Exception.Message -like "*unsafe artifact path*")
            }
            if (-not $failed) { throw "selftest: unsafe manifest path was not rejected ($unsafe)" }
        }
        $failed = $false
        try {
            Assert-ManifestEntries @(
                [pscustomobject]@{ path = "models/e.onnx"; byteLength = 1; sha256 = ("0" * 64) },
                [pscustomobject]@{ path = "models/E.onnx"; byteLength = 1; sha256 = ("0" * 64) }
            )
        } catch {
            $failed = ($_.Exception.Message -like "*duplicate artifact path*")
        }
        if (-not $failed) { throw "selftest: duplicate manifest path was not rejected" }

        $ret7 = Join-Path $root 'ret7'
        $prior7 = Join-Path $ret7 'bundle'
        $staging7 = Join-Path $ret7 '.staging-next'
        New-SelfTestPackage $prior7
        New-SelfTestPackage $staging7
        $failed = $false
        try {
            Publish-Package -RetrievalDir $ret7 -BundleDir $prior7 -StagingDir $staging7 -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -ForceFinalValidationFailure
        } catch {
            $failed = ($_.Exception.Message -like '*SHA-256 mismatch*')
        }
        if (-not $failed) { throw 'selftest: final publication validation failure was not surfaced' }
        Assert-PackageIntegrity -Dir $prior7 -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label 'restored prior bundle'
        if (@(Get-ChildItem $ret7 -Force -Filter '.bundle-backup-*').Count -ne 0) {
            throw 'selftest: rollback backup was deleted or left ambiguous'
        }
        Write-Host 'selftest ok : failed final validation restored the verified prior bundle'
        $ret8 = Join-Path $root 'ret8'
        $prior8 = Join-Path $ret8 'bundle'
        $staging8 = Join-Path $ret8 '.staging-next'
        New-SelfTestPackage $prior8
        New-SelfTestPackage $staging8
        $failed = $false
        try {
            Publish-Package -RetrievalDir $ret8 -BundleDir $prior8 -StagingDir $staging8 -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -ValidatePublishedResources { throw 'selftest final resource mapping failure' }
        } catch {
            $failed = ($_.Exception.Message -eq 'selftest final resource mapping failure')
        }
        if (-not $failed) { throw 'selftest: final resource mapping failure was not surfaced' }
        Assert-PackageIntegrity -Dir $prior8 -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label 'mapping-failure restored prior bundle'
        if (@(Get-ChildItem -LiteralPath $ret8 -Directory -Filter '.bundle-backup-*').Count -ne 0) {
            throw 'selftest: final resource mapping failure left a stranded backup'
        }
        Write-Host 'selftest ok : failed final resource mapping restored the verified prior bundle'
        Write-Host "selftest ok : unmanifested/tampered/divergent/unsafe/duplicate content rejected; clean package accepted"
        Write-Host "SELFTEST PASS"
    } finally {
        Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if ($SelfTest) {
    Invoke-SelfTest
    exit 0
}

if (-not (Test-Path -LiteralPath $ManifestPath)) { throw "manifest not found: $ManifestPath" }
$manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
if ($manifest.manifestVersion -ne 1) { throw "unsupported manifestVersion '$($manifest.manifestVersion)' (expected 1)" }
if (-not $manifest.bundleId) { throw "bundleId missing from manifest" }

$entries = @(
    $manifest.embeddingModel.artifacts +
    $manifest.embeddingModel.tokenizer.artifacts +
    $manifest.rerankerModel.artifacts +
    $manifest.rerankerModel.tokenizer.artifacts +
    $manifest.licenses
)
Assert-ManifestEntries -Entries $entries
Assert-TauriResourceContract -StaticOnly

# Recover a crashed publication first; only then drop stale leftovers.
# Recovery runs the same package-integrity gate as staging/publish, so a
# missing, corrupt, or foreign backup can never be renamed into `bundle`.
Restore-CrashedPublication -RetrievalDir $retDir -BundleDir $finalDir -Entries $entries -ManifestSourcePath $ManifestPath -AllowedExtra $allowedExtraFiles
Assert-TauriResourceContract

# Leftovers from a crashed staging run are inert but large; single-owner script.
Get-ChildItem $retDir -Directory -Filter ".staging-*" -ErrorAction SilentlyContinue |
    Remove-Item -Recurse -Force

# Fetch into the build cache; never download into final packaged resources.
$cacheDir = Join-Path $CacheRoot $manifest.bundleId
foreach ($entry in $entries) {
    $dest = Join-Path $cacheDir ($entry.path.Replace('/', '\'))
    if ((Test-Path -LiteralPath $dest) `
            -and ((Get-Item -LiteralPath $dest).Length -eq [int64]$entry.byteLength) `
            -and ((Get-Sha256 $dest) -eq $entry.sha256)) {
        Write-Host "cache hit : $($entry.path)"
        continue
    }
    if (-not $entry.source -or -not $entry.source.url) {
        throw "no valid cache entry and no pinned source URL for '$($entry.path)'"
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dest) | Out-Null
    # Licenses have a checked-in exact source; prefer it over the network.
    $checkedIn = Join-Path $retDir ($entry.path.Replace('/', '\'))
    if (($entry.path -like 'licenses/*') -and (Test-Path -LiteralPath $checkedIn) `
            -and ((Get-Item -LiteralPath $checkedIn).Length -eq [int64]$entry.byteLength) `
            -and ((Get-Sha256 $checkedIn) -eq $entry.sha256)) {
        Copy-Item -LiteralPath $checkedIn -Destination $dest
        Write-Host "checked-in: $($entry.path)"
        continue
    }
    $temp = "$dest.download"
    Write-Host "fetching  : $($entry.source.url)"
    & curl.exe -sSL --fail --retry 3 -o $temp $entry.source.url
    if ($LASTEXITCODE -ne 0) {
        Remove-Item $temp -ErrorAction SilentlyContinue
        throw "download failed: $($entry.source.url)"
    }
    if (((Get-Item -LiteralPath $temp).Length -ne [int64]$entry.byteLength) -or ((Get-Sha256 $temp) -ne $entry.sha256)) {
        Remove-Item $temp -Force
        throw "downloaded file fails verification: $($entry.source.url)"
    }
    Move-Item -LiteralPath $temp -Destination $dest -Force
}

# Stage the complete package outside final resources, verify as one unit.
$stagingDir = Join-Path $retDir (".staging-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $stagingDir | Out-Null

try {
    foreach ($entry in $entries) {
        $target = Join-Path $stagingDir ($entry.path.Replace('/', '\'))
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
        Copy-Item -LiteralPath (Join-Path $cacheDir ($entry.path.Replace('/', '\'))) -Destination $target
    }
    Copy-Item -LiteralPath $ManifestPath -Destination (Join-Path $stagingDir $manifestFileName)

    # Retain ONLY the pinned committed placeholder from the prior bundle, and
    # only after its bytes match the pin; every other prior-bundle file is
    # stale or foreign and must be removed by hand (the content check below
    # names them), never silently repackaged.
    foreach ($extra in $allowedExtraFiles.Keys) {
        $source = Join-Path $finalDir $extra
        if (Test-Path -LiteralPath $source) {
            $sourceItem = Get-Item -LiteralPath $source
            if ($sourceItem.PSIsContainer -or ($sourceItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0 -or
                $sourceItem.Length -ne [int64]$allowedExtraFiles[$extra].byteLength -or
                (Get-Sha256 $source) -ne $allowedExtraFiles[$extra].sha256) {
                throw ("prior bundle holds a '{0}' that does not match the pinned committed placeholder; restore the committed file (git checkout -- it), delete any stale copy, and rerun." -f $extra)
            }
            Copy-Item -LiteralPath $source -Destination (Join-Path $stagingDir $extra)
        }
    }

    # Verify the complete staged package with the shared integrity gate.
    Assert-PackageIntegrity -Dir $stagingDir -Entries $entries -ManifestSourcePath $ManifestPath -AllowedExtra $allowedExtraFiles -Label "staged package"

    Publish-Package -RetrievalDir $retDir -BundleDir $finalDir -StagingDir $stagingDir -Entries $entries -ManifestSourcePath $ManifestPath -AllowedExtra $allowedExtraFiles -ValidatePublishedResources { Assert-TauriResourceContract }

    $totalBytes = ($entries | Measure-Object -Property byteLength -Sum).Sum
    Write-Host ("published : {0} ({1} artifacts, {2} MiB) -> resources/retrieval/bundle" -f $manifest.bundleId, $entries.Count, [Math]::Round($totalBytes / 1MB, 1))
} finally {
    if (Test-Path -LiteralPath $stagingDir) {
        Remove-Item -LiteralPath $stagingDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
