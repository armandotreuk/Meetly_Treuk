# Static, local-first protection for the R13 Windows package policy. This
# validates the exact CI contract that previously failed only after a full CUDA
# package build. It is deliberately text-based: it can run before a compiler,
# CUDA toolkit, or GitHub runner are available.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$workflowPath = Join-Path $repoRoot '.github/workflows/build-windows.yml'
$workflow = Get-Content -LiteralPath $workflowPath -Raw
$preflightPath = Join-Path $repoRoot '.github/workflows/windows-preflight.yml'
$preflight = Get-Content -LiteralPath $preflightPath -Raw

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
if ($identity -notmatch 'Get-ChildItem -LiteralPath \$installRoot -Filter ''nvcuda\.dll'' -File -Recurse') {
    throw 'R13 package verification must reject nvcuda.dll anywhere in the installed tree.'
}
if ($identity -notmatch 'system-provided NVIDIA driver API') {
    throw 'R13 package verification must document the system driver boundary.'
}
if ($identity -notmatch '\$installedResourceDir = Join-Path \$installRoot ''resources\\cuda-runtime''') {
    throw 'R13 package verification must inspect Tauri''s one-root CUDA runtime resource directory.'
}
if ($identity -notmatch 'did not preserve the staged .* in its resource directory') {
    throw 'R13 package verification must validate the staged CUDA runtimes inside the installed resource directory.'
}
if ($identity -notmatch 'nested resources resource directory') {
    throw 'R13 package verification must reject a nested Tauri resource root.'
}
$cudaRuntimeHookPath = Join-Path $repoRoot 'upstream/frontend/src-tauri/windows/cuda-runtime-hooks.nsh'
if (-not (Test-Path -LiteralPath $cudaRuntimeHookPath -PathType Leaf)) {
    throw 'R13 CUDA validation hook is missing.'
}
$cudaRuntimeHook = Get-Content -LiteralPath $cudaRuntimeHookPath -Raw
$expectedCudaRuntimeCopy = 'CopyFiles /SILENT "$INSTDIR\resources\cuda-runtime\*.dll" "$INSTDIR"'
if ($cudaRuntimeHook.IndexOf($expectedCudaRuntimeCopy, [StringComparison]::Ordinal) -lt 0) {
    throw 'R13 CUDA validation hook must copy the staged runtime from the single Tauri installer resource root.'
}
if ($cudaRuntimeHook.IndexOf('$INSTDIR\resources\resources\cuda-runtime', [StringComparison]::Ordinal) -ge 0) {
    throw 'R13 CUDA validation hook must not add a second resources directory.'
}
if ($identity.IndexOf('$r13InstallerArgs = "/S /D=$installRoot"', [StringComparison]::Ordinal) -lt 0) {
    throw 'R13 package verification must pass NSIS silent and destination arguments as one raw string, with /D last.'
}
if ($identity -notmatch '(?s)Start-Process\s+-FilePath\s+\$nsis\[0\]\.FullName\s+-ArgumentList\s+\$r13InstallerArgs\s+-Wait\s+-PassThru') {
    throw 'R13 package verification must wait for the NSIS installer using its raw destination argument string.'
}
if ($identity -notmatch 'completed without creating the requested isolated installation directory') {
    throw 'R13 package verification must fail clearly when NSIS ignores the isolated destination.'
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

# The staging self-test uses `cargo test --offline` for the workspace helper.
# Keep it in the Rust job, after `cargo fetch --locked`, so a clean runner has
# its git dependencies before that intentionally offline test begins.
$frontendStart = $preflight.IndexOf('  frontend-and-policy:', [StringComparison]::Ordinal)
$cargoStart = $preflight.IndexOf('  cargo-check:', [StringComparison]::Ordinal)
$preflightGateStart = $preflight.IndexOf('  gate-preflight:', [StringComparison]::Ordinal)
if ($frontendStart -lt 0 -or $cargoStart -lt 0 -or $preflightGateStart -lt 0) {
    throw 'Windows preflight must retain frontend, Cargo, and terminal-gate jobs.'
}
$frontendPreflight = $preflight.Substring($frontendStart, $cargoStart - $frontendStart)
$cargoPreflight = $preflight.Substring($cargoStart, $preflightGateStart - $cargoStart)
$preflightGate = $preflight.Substring($preflightGateStart)
$preflightGateStepsStart = $preflightGate.IndexOf('    steps:', [StringComparison]::Ordinal)
if ($preflightGateStepsStart -lt 0) {
    throw 'The Windows preflight terminal gate must define steps after its job-level condition.'
}
$preflightGatePreamble = $preflightGate.Substring(0, $preflightGateStepsStart)

# GitHub documents `!cancelled()` as the status-function alternative to
# `always()`: ordinary frontend/Cargo failures obtain a terminal verdict, but
# a canceled stale workflow cannot start its gate and hold the per-ref
# concurrency group ahead of a newer pull-request head.
$preflightGateCondition = '(?m)^ {4}if: \$\{\{ !cancelled\(\) \}\}\r?$'
if ([regex]::Matches($preflightGatePreamble, $preflightGateCondition).Count -ne 1) {
    throw 'The Windows preflight terminal gate must run after failures but skip a canceled stale workflow.'
}

function Get-NamedPreflightStep([string]$Job, [string]$JobName, [string]$StepName) {
    $stepPattern = "(?m)^ {6}- name: $([regex]::Escape($StepName))[ \t]*\r?$"
    $matches = [regex]::Matches($Job, $stepPattern)
    if ($matches.Count -ne 1) {
        throw "The $JobName preflight job must contain exactly one '$StepName' step."
    }
    $start = $matches[0].Index
    $stepStartRegex = New-Object System.Text.RegularExpressions.Regex('(?m)^ {6}- name: ')
    $next = $stepStartRegex.Match($Job, $start + 1)
    $end = if ($next.Success) { $next.Index } else { $Job.Length }
    return [pscustomobject]@{
        Start = $start
        Text = $Job.Substring($start, $end - $start)
    }
}

$selfTestName = 'Run retrieval staging/recovery self-test'
$selfTestInvocation = '(?m)^ {8}run: \./upstream/frontend/src-tauri/scripts/stage-retrieval-models\.ps1 -SelfTest[ \t]*\r?$'
if ([regex]::IsMatch($frontendPreflight, $selfTestInvocation)) {
    throw 'The offline retrieval self-test invocation must not run in the frontend-only preflight job.'
}
$fetchStep = Get-NamedPreflightStep $cargoPreflight 'Cargo' 'Fetch deps and restore whisper-rs-sys bindings'
$selfTestStep = Get-NamedPreflightStep $cargoPreflight 'Cargo' $selfTestName
if ($fetchStep.Text -notmatch '(?m)^ {10}cargo fetch --locked[ \t]*\r?$') {
    throw 'The Cargo dependency-fetch step must execute cargo fetch --locked.'
}
if ($selfTestStep.Text -notmatch $selfTestInvocation) {
    throw 'The Cargo self-test step must execute stage-retrieval-models.ps1 -SelfTest.'
}
if ($fetchStep.Start -ge $selfTestStep.Start) {
    throw 'The Cargo preflight must fetch locked dependencies before the offline retrieval self-test.'
}

Write-Host 'windows-ci-contract: passed'
