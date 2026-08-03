# build_cuda.ps1 — Build Meetily with CUDA 13.3 + RTX 5060 (sm_120)
#
# Usage:
#   .\build_cuda.ps1          # Build only
#   .\build_cuda.ps1 -Run     # Build + run (tauri dev)
#   .\build_cuda.ps1 -Release # Build release lib only

param(
    [switch]$Run,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

# CUDA 13.3 paths
$env:CUDA_PATH               = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3"
$env:CUDA_PATH_V13_3          = $env:CUDA_PATH
$env:CUDNN_PATH               = $env:CUDA_PATH
$env:CMAKE_CUDA_COMPILER      = "$env:CUDA_PATH\bin\nvcc.exe"

# Architecture: RTX 5060 Laptop = sm_120
$env:CMAKE_CUDA_ARCHITECTURES = "120"

# Fix CUDA 13.3 + CCCL header compatibility
# 1. /Zc:preprocessor — CCCL headers reject the MSVC traditional preprocessor
# 2. C++17 — CCCL/CUB headers require C++17 minimum
$env:CMAKE_CUDA_FLAGS         = "-Xcompiler /Zc:preprocessor"
$env:CMAKE_CXX_STANDARD       = "17"
$env:CMAKE_CUDA_STANDARD      = "17"
$env:CMAKE_CUDA_STANDARD_REQUIRED = "ON"

# Skip CMake compiler detection (CUDA 13.3 + CMake 4.3 = nvcc internal error)
$env:CMAKE_CUDA_COMPILER_WORKS = "1"

# Use pre-generated bindings
$env:WHISPER_DONT_GENERATE_BINDINGS = "1"

# OneDrive-safe target directory
$env:CARGO_TARGET_DIR = "C:\Users\arman\cargo-target"

# Add CUDA DLLs to PATH for runtime
$env:PATH = "$env:CUDA_PATH\bin\x64;$env:CUDA_PATH\bin;$env:CUDA_PATH\lib\x64;$env:PATH"

Write-Host "=== Meetily CUDA Build ===" -ForegroundColor Cyan
Write-Host "CUDA Toolkit: $env:CUDA_PATH"
Write-Host "Architecture: sm_120 (RTX 5060)"
Write-Host "Target dir:   $env:CARGO_TARGET_DIR"
Write-Host ""

Push-Location "$PSScriptRoot\src-tauri"

if ($Release) {
    Write-Host "Building release lib with CUDA..." -ForegroundColor Yellow
    cargo build --release --features cuda --lib
} elseif ($Run) {
    Write-Host "Starting Tauri dev with CUDA..." -ForegroundColor Green
    npx tauri dev -- --features cuda
} else {
    Write-Host "Building debug with CUDA..." -ForegroundColor Yellow
    cargo build --features cuda
}

Pop-Location
