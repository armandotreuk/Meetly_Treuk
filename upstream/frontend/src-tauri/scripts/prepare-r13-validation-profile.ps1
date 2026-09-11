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

function Test-DirectoryEntryExists {
    param([Parameter(Mandatory)][string]$Path)

    $parent = Split-Path -Parent $Path
    $leaf = Split-Path -Leaf $Path
    if (-not (Test-Path -LiteralPath $parent)) {
        return $false
    }

    # Enumerating the parent catches a dangling junction, for which Test-Path
    # alone can return false even though the entry still occupies the name.
    $entries = @(Get-ChildItem -LiteralPath $parent -Force -ErrorAction Stop |
        Where-Object { $_.Name -ieq $leaf } |
        Select-Object -First 1)
    return $entries.Count -gt 0
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

$profileRoots = @()
foreach ($variableName in @('APPDATA', 'LOCALAPPDATA')) {
    $base = [Environment]::GetEnvironmentVariable($variableName)
    if ([string]::IsNullOrWhiteSpace($base)) {
        throw "$variableName is not available in this Windows session."
    }

    $link = Join-Path $base $ApplicationIdentifier
    $directoryName = if ($variableName -eq 'APPDATA') { 'roaming-app-data' } else { 'local-app-data' }
    $target = Join-Path $root $directoryName

    if (Test-DirectoryEntryExists $link) {
        throw "Refusing to replace existing validation path '$link'. Inspect or remove it manually only after preserving required evidence."
    }
    if (Test-DirectoryEntryExists $target) {
        throw "Refusing to reuse existing validation target '$target'. Choose a fresh D: validation root."
    }

    $profileRoots += [PSCustomObject]@{
        Link = $link
        Target = $target
        LinkCreated = $false
        TargetCreated = $false
    }
}

try {
    foreach ($profileRoot in $profileRoots) {
        if (-not $PSCmdlet.ShouldProcess("$($profileRoot.Link) -> $($profileRoot.Target)", 'create validation-only directory junction')) {
            continue
        }

        New-Item -ItemType Directory -Force -Path $profileRoot.Target | Out-Null
        $profileRoot.TargetCreated = $true
        New-Item -ItemType Junction -Path $profileRoot.Link -Target $profileRoot.Target | Out-Null
        $profileRoot.LinkCreated = $true

        $linkItem = Get-Item -LiteralPath $profileRoot.Link
        $targetProperty = $linkItem.PSObject.Properties['Target']
        if ($null -eq $targetProperty -or [string]::IsNullOrWhiteSpace([string]$targetProperty.Value)) {
            throw "Validation junction '$($profileRoot.Link)' was created, but its target could not be verified."
        }
        $resolved = if ($targetProperty.Value -is [array]) { $targetProperty.Value[0] } else { [string]$targetProperty.Value }
        if ((Normalize-PathForComparison $resolved) -ne (Normalize-PathForComparison $profileRoot.Target)) {
            throw "Validation junction '$($profileRoot.Link)' does not resolve to the requested D: target."
        }
        Write-Host "Prepared validation-only profile root: $($profileRoot.Link) -> $($profileRoot.Target)"
    }
} catch {
    foreach ($profileRoot in @($profileRoots | Where-Object LinkCreated)) {
        try {
            # The link was created by this invocation after a clean preflight;
            # remove the entry itself even if its target metadata is malformed.
            Remove-Item -LiteralPath $profileRoot.Link -Force
            if (Test-DirectoryEntryExists $profileRoot.Link) {
                throw 'junction entry still exists after removal'
            }
        } catch {
            Write-Warning "Could not remove newly created validation junction '$($profileRoot.Link)': $($_.Exception.Message)"
        }
    }
    foreach ($profileRoot in @($profileRoots | Where-Object TargetCreated)) {
        try {
            if ((Get-ChildItem -LiteralPath $profileRoot.Target -Force).Count -eq 0) {
                Remove-Item -LiteralPath $profileRoot.Target -Force
            }
        } catch {
            Write-Warning "Could not remove newly created validation target '$($profileRoot.Target)': $($_.Exception.Message)"
        }
    }
    throw
}
