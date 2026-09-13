# Static, local-first protection for the R13 Windows package policy. This
# validates the exact CI contract that previously failed only after a full CUDA
# package build. It is deliberately text-based: it can run before a compiler,
# CUDA toolkit, or GitHub runner are available.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$workflowPath = Join-Path $repoRoot '.github/workflows/build-windows.yml'
$workflow = Get-Content -LiteralPath $workflowPath -Raw

function Get-WorkflowSection([string]$StartMarker, [string]$EndMarker) {
    $start = $workflow.IndexOf($StartMarker, [StringComparison]::Ordinal)
    if ($start -lt 0) { throw "Missing Windows CI contract section: $StartMarker" }
    $end = $workflow.IndexOf($EndMarker, $start, [StringComparison]::Ordinal)
    if ($end -lt 0) { throw "Missing Windows CI contract boundary after: $StartMarker" }
    return $workflow.Substring($start, $end - $start)
}

$identity = Get-WorkflowSection '      - name: Verify R13 validation artifact identity' '      - name: Rename build profile dir for upload'
if ($identity -match 'must not import nvcuda\.dll') {
    throw 'R13 CUDA executable imports of the system NVIDIA driver must not be rejected.'
}
if ($identity -notmatch 'Join-Path \$installRoot ''nvcuda\.dll''') {
    throw 'R13 package verification must reject a bundled nvcuda.dll.'
}
if ($identity -notmatch 'system-provided NVIDIA driver API') {
    throw 'R13 package verification must document the system driver boundary.'
}
$nvcudaBranchMarker = '          if ($dependents -match ''(?im)^\s*nvcuda\.dll\s*$'') {'
$nvcudaBranchStart = $identity.IndexOf($nvcudaBranchMarker, [StringComparison]::Ordinal)
if ($nvcudaBranchStart -lt 0) {
    throw 'R13 package verification must identify the executable nvcuda.dll import branch.'
}
$nvcudaBranchEnd = $identity.IndexOf("`n          }", $nvcudaBranchStart, [StringComparison]::Ordinal)
if ($nvcudaBranchEnd -lt 0) {
    throw 'R13 nvcuda.dll import branch is not structurally complete.'
}
$nvcudaBranch = $identity.Substring($nvcudaBranchStart, $nvcudaBranchEnd - $nvcudaBranchStart)
if ($nvcudaBranch -notmatch '\bWrite-Host\b') {
    throw 'R13 nvcuda.dll executable import branch must explicitly permit and record the system driver dependency.'
}
if ($nvcudaBranch -match '(?im)\b(?:throw|exit)\b') {
    throw 'R13 nvcuda.dll executable import branch must not reject the system driver dependency.'
}

$gateStart = $workflow.IndexOf('  gate-installed-package-smoke:', [StringComparison]::Ordinal)
if ($gateStart -lt 0) { throw 'Missing R13 terminal gate.' }
$gate = $workflow.Substring($gateStart)
if ($gate -notmatch "if: needs\.build-windows\.outputs\.r13-validation != 'true'") {
    throw 'R13 terminal gate must conditionally skip normal smoke evidence.'
}
foreach ($stepName in @('Download MSI smoke evidence', 'Download NSIS smoke evidence', 'Restore gate evidence')) {
    $escapedName = [regex]::Escape($stepName)
    $pattern = "(?ms)^      - name: $escapedName\r?\n        if: needs\.build-windows\.outputs\.r13-validation != 'true'"
    if (-not [regex]::IsMatch($gate, $pattern)) {
        throw "R13 terminal gate must skip '$stepName' when normal smokes are intentionally absent."
    }
}

Write-Host 'windows-ci-contract: passed'
