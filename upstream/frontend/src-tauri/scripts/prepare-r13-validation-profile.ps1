<#
.SYNOPSIS
Prepares the hermetic Windows profile roots used only by the R13 validation
package.

.DESCRIPTION
The validation Tauri configuration has its own application identifier
(`com.meetily.ai.r13validation`). Windows/Tauri resolves that identifier below
the user's roaming and local application-data folders. This script creates
new directory junctions for those *validation-only* paths so their contents
are stored below the supplied D: validation root instead. It refuses to
replace an existing directory, link, or any non-validation application ID.

Run this before the first launch of the validation package. Use -WhatIf to
inspect the exact paths without changing the machine.
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)]
    [string]$ValidationRoot,

    [string]$ApplicationIdentifier = 'com.meetily.ai.r13validation'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Normalize-PathForComparison {
    param([Parameter(Mandatory)][string]$Path)

    return [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
}

if ($ApplicationIdentifier -ne 'com.meetily.ai.r13validation') {
    throw 'This helper only supports the dedicated R13 validation application identifier.'
}

$root = [IO.Path]::GetFullPath($ValidationRoot)
if (-not $root.StartsWith('D:\', [StringComparison]::OrdinalIgnoreCase)) {
    throw "ValidationRoot must be under D:\; received '$root'."
}
if ($root.TrimEnd('\') -ieq 'D:') {
    throw 'ValidationRoot must be a dedicated child directory under D:\, not the drive root.'
}

foreach ($variableName in @('APPDATA', 'LOCALAPPDATA')) {
    $base = [Environment]::GetEnvironmentVariable($variableName)
    if ([string]::IsNullOrWhiteSpace($base)) {
        throw "$variableName is not available in this Windows session."
    }

    $link = Join-Path $base $ApplicationIdentifier
    $directoryName = if ($variableName -eq 'APPDATA') { 'roaming-app-data' } else { 'local-app-data' }
    $target = Join-Path $root $directoryName

    if (Test-Path -LiteralPath $link) {
        throw "Refusing to replace existing validation path '$link'. Inspect or remove it manually only after preserving required evidence."
    }
    if (Test-Path -LiteralPath $target) {
        throw "Refusing to reuse existing validation target '$target'. Choose a fresh D: validation root."
    }

    if ($PSCmdlet.ShouldProcess("$link -> $target", 'create validation-only directory junction')) {
        New-Item -ItemType Directory -Force -Path $target | Out-Null
        New-Item -ItemType Junction -Path $link -Target $target | Out-Null

        $linkItem = Get-Item -LiteralPath $link
        $targetProperty = $linkItem.PSObject.Properties['Target']
        if ($null -eq $targetProperty -or [string]::IsNullOrWhiteSpace([string]$targetProperty.Value)) {
            throw "Validation junction '$link' was created, but its target could not be verified."
        }
        $resolved = if ($targetProperty.Value -is [array]) { $targetProperty.Value[0] } else { [string]$targetProperty.Value }
        if ((Normalize-PathForComparison $resolved) -ne (Normalize-PathForComparison $target)) {
            throw "Validation junction '$link' does not resolve to the requested D: target."
        }
        Write-Host "Prepared validation-only profile root: $link -> $target"
    }
}
