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
    [string]$ManifestPath = "",
    [string]$CacheRoot = (Join-Path $env:LOCALAPPDATA "meetily\model-cache"),
    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$srcTauriDir = Split-Path -Parent $scriptDir
if (-not $ManifestPath) {
    $ManifestPath = Join-Path $srcTauriDir "resources\retrieval\model-bundle.manifest.json"
}
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
    if (-not $Entry.path) { throw "artifact entry without path" }
    if ($Entry.path -match '[\\:]' -or $Entry.path.StartsWith('/') -or (@($Entry.path -split '/')).Contains('..')) {
        throw "unsafe artifact path '$($Entry.path)'"
    }
    if ($Entry.sha256 -notmatch '^[0-9a-f]{64}$') { throw "malformed SHA-256 for '$($Entry.path)'" }
    if (-not $Entry.byteLength -or [int64]$Entry.byteLength -le 0) { throw "missing byteLength for '$($Entry.path)'" }
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
    foreach ($m in $ManagedRelative) { $managed[$m.ToLowerInvariant()] = $true }
    $unexpected = @()
    Get-ChildItem -LiteralPath $Dir -Recurse -File | ForEach-Object {
        $relative = $_.FullName.Substring($Dir.Length + 1)
        if (-not $managed.ContainsKey($relative.ToLowerInvariant())) { $unexpected += $relative }
    }
    return ,@($unexpected)
}

function Assert-PackageIntegrity([string]$Dir, [object[]]$Entries, [string]$ManifestSourcePath, [hashtable]$AllowedExtra, [string]$Label) {
    # Single package-integrity gate shared by recovery, staging, and
    # post-publish: nothing becomes or stays `bundle` unless all of these hold.
    foreach ($entry in $Entries) {
        $path = Join-Path $Dir ($entry.path.Replace('/', '\'))
        if (-not (Test-Path -LiteralPath $path)) {
            throw ("{0} is missing manifest-managed artifact '{1}'" -f $Label, $entry.path)
        }
        $actual = (Get-Item -LiteralPath $path).Length
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
    if ((Get-Sha256 $manifestCopy) -ne (Get-Sha256 $ManifestSourcePath)) {
        throw ("{0} holds a '{1}' that is not byte-identical to the checked-in publication manifest" -f $Label, $manifestFileName)
    }
    foreach ($extra in $AllowedExtra.Keys) {
        $path = Join-Path $Dir $extra
        if (-not (Test-Path -LiteralPath $path)) {
            throw ("{0} is missing required committed placeholder '{1}'" -f $Label, $extra)
        }
        if ((Get-Item -LiteralPath $path).Length -ne [int64]$AllowedExtra[$extra].byteLength -or
            (Get-Sha256 $path) -ne $AllowedExtra[$extra].sha256) {
            throw ("{0} holds a '{1}' that does not match the pinned committed placeholder; restore the committed file (git checkout -- it) and rerun." -f $Label, $extra)
        }
    }
    $unexpected = Get-UnexpectedFiles -Dir $Dir -ManagedRelative (Get-ManagedRelativePaths $Entries $AllowedExtra)
    if ($unexpected.Count -gt 0) {
        throw ("{0} contains unexpected unmanifested file(s): {1}. Delete the listed stale file(s) from the previous bundle and rerun; arbitrary prior-bundle files are never copied into the signed package." -f $Label, ($unexpected -join ', '))
    }
}

function Restore-CrashedPublication([string]$RetrievalDir, [string]$BundleDir, [object[]]$Entries, [string]$ManifestSourcePath, [hashtable]$AllowedExtra) {
    # Runs BEFORE stale-dir cleanup so the only recoverable backup survives.
    if (Test-Path -LiteralPath $BundleDir) { return }
    $backups = @(Get-ChildItem $RetrievalDir -Directory -Filter ".bundle-backup-*" -ErrorAction SilentlyContinue)
    if ($backups.Count -gt 1) {
        throw ("bundle is missing and {0} .bundle-backup-* directories exist ({1}); refusing ambiguous recovery" -f $backups.Count, (($backups | ForEach-Object { $_.Name }) -join ', '))
    }
    if ($backups.Count -eq 0) { return }
    Assert-PackageIntegrity -Dir $backups[0].FullName -Entries $Entries -ManifestSourcePath $ManifestSourcePath -AllowedExtra $AllowedExtra -Label "recoverable backup $($backups[0].Name)"
    Rename-Item -LiteralPath $backups[0].FullName -NewName (Split-Path -Leaf $BundleDir)
    Write-Host "recovered : restored previous bundle from $($backups[0].Name)"
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

        # 8. An unmanifested staged file is rejected...
        Set-Content -LiteralPath (Join-Path $stage "stale-extra.onnx") -Value "x"
        $failed = $false
        try {
            Assert-PackageIntegrity -Dir $stage -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label "staged package"
        } catch {
            $failed = ($_.Exception.Message -like "*stale-extra.onnx*")
        }
        if (-not $failed) { throw "selftest: unmanifested staged file was not rejected" }

        # 9. ...a tampered placeholder fails closed even though its name is on
        #    the allowed list...
        Remove-Item -LiteralPath (Join-Path $stage "stale-extra.onnx")
        Set-Content -LiteralPath (Join-Path $stage "README.md") -Value "tampered!!"
        $failed = $false
        try {
            Assert-PackageIntegrity -Dir $stage -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label "staged package"
        } catch {
            $failed = ($_.Exception.Message -like "*pinned committed placeholder*")
        }
        if (-not $failed) { throw "selftest: tampered placeholder was not rejected" }

        # 10. ...and a manifest copy diverging from the checked-in publication
        #     authority fails closed.
        Set-Content -LiteralPath (Join-Path $stage "README.md") -Value "placeholder"
        Set-Content -LiteralPath (Join-Path $stage $manifestFileName) -Value '{"bundleId":"other"}'
        $failed = $false
        try {
            Assert-PackageIntegrity -Dir $stage -Entries $entries -ManifestSourcePath $authority -AllowedExtra $extras -Label "staged package"
        } catch {
            $failed = ($_.Exception.Message -like "*byte-identical to the checked-in publication manifest*")
        }
        if (-not $failed) { throw "selftest: divergent manifest copy was not rejected" }
        Write-Host "selftest ok : unmanifested/tampered/divergent content rejected; clean package accepted"
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
if ($entries.Count -eq 0) { throw "manifest declares no artifacts" }
foreach ($entry in $entries) { Assert-ManifestEntry $entry }
$duplicates = @($entries | Group-Object path | Where-Object { $_.Count -gt 1 })
if ($duplicates.Count -gt 0) {
    throw "duplicate artifact path(s): $(($duplicates | ForEach-Object { $_.Name }) -join ', ')"
}

# Recover a crashed publication first; only then drop stale leftovers.
# Recovery runs the same package-integrity gate as staging/publish, so a
# missing, corrupt, or foreign backup can never be renamed into `bundle`.
Restore-CrashedPublication -RetrievalDir $retDir -BundleDir $finalDir -Entries $entries -ManifestSourcePath $ManifestPath -AllowedExtra $allowedExtraFiles

# Leftovers from a crashed staging run are inert but large; single-owner script.
Get-ChildItem $retDir -Directory -Filter ".staging-*" -ErrorAction SilentlyContinue |
    Remove-Item -Recurse -Force
Get-ChildItem $retDir -Directory -Filter ".bundle-backup-*" -ErrorAction SilentlyContinue |
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
            if ((Get-Item -LiteralPath $source).Length -ne [int64]$allowedExtraFiles[$extra].byteLength -or
                (Get-Sha256 $source) -ne $allowedExtraFiles[$extra].sha256) {
                throw ("prior bundle holds a '{0}' that does not match the pinned committed placeholder; restore the committed file (git checkout -- it), delete any stale copy, and rerun." -f $extra)
            }
            Copy-Item -LiteralPath $source -Destination (Join-Path $stagingDir $extra)
        }
    }

    # Verify the complete staged package with the shared integrity gate.
    Assert-PackageIntegrity -Dir $stagingDir -Entries $entries -ManifestSourcePath $ManifestPath -AllowedExtra $allowedExtraFiles -Label "staged package"

    $backupDir = $null
    if (Test-Path -LiteralPath $finalDir) {
        $backupDir = Join-Path $retDir (".bundle-backup-" + [guid]::NewGuid().ToString("N"))
        Rename-Item -LiteralPath $finalDir -NewName (Split-Path -Leaf $backupDir)
    }
    try {
        Rename-Item -LiteralPath $stagingDir -NewName "bundle"
    } catch {
        if ($backupDir) { Rename-Item -LiteralPath $backupDir -NewName "bundle" }
        throw
    }
    if ($backupDir) {
        Remove-Item -LiteralPath $backupDir -Recurse -Force
    }

    # Post-publish authority check on the final signed-package input.
    Assert-PackageIntegrity -Dir $finalDir -Entries $entries -ManifestSourcePath $ManifestPath -AllowedExtra $allowedExtraFiles -Label "published bundle"

    $totalBytes = ($entries | Measure-Object -Property byteLength -Sum).Sum
    Write-Host ("published : {0} ({1} artifacts, {2} MiB) -> {3}" -f $manifest.bundleId, $entries.Count, [Math]::Round($totalBytes / 1MB, 1), $finalDir)
} finally {
    if (Test-Path -LiteralPath $stagingDir) {
        Remove-Item -LiteralPath $stagingDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
