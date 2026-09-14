# Windows x64 packages must not inherit the GitHub runner's CPU instruction set.
#
# whisper-rs-sys builds its vendored ggml/whisper.cpp sources as a native CMake
# project.  Without this override, GGML_NATIVE probes the runner and can enable
# AVX-512.  Many current client CPUs, including Intel Core Ultra mobile parts,
# do not expose AVX-512; such a package terminates with 0xc000001d as soon as a
# Whisper context is created.  Keep the portable AVX/AVX2/FMA defaults while
# explicitly excluding runner-specific AVX-512 and AMX variants.
set(GGML_NATIVE OFF CACHE BOOL "Build portable Windows x64 GGML binaries" FORCE)
set(GGML_AVX512 OFF CACHE BOOL "Do not require AVX-512 in distributed binaries" FORCE)
set(GGML_AVX512_VBMI OFF CACHE BOOL "Do not require AVX-512 VBMI" FORCE)
set(GGML_AVX512_VNNI OFF CACHE BOOL "Do not require AVX-512 VNNI" FORCE)
set(GGML_AVX512_BF16 OFF CACHE BOOL "Do not require AVX-512 BF16" FORCE)
set(GGML_AMX_TILE OFF CACHE BOOL "Do not require AMX tile instructions" FORCE)
set(GGML_AMX_INT8 OFF CACHE BOOL "Do not require AMX INT8 instructions" FORCE)
set(GGML_AMX_BF16 OFF CACHE BOOL "Do not require AMX BF16 instructions" FORCE)
