# F5 Speaker Diarization — Model Research

> **Goal:** Pick a diarization model for Personal Meetly (shared with team, local processing, MIT-licensed derivative).
> **Requirements:** permissive weights license (redistributable), local offline inference, ONNX or convertible, reasonable CPU performance, good accuracy.
> **Date:** 2026-06-28

---

## Update (2026-06-28, post-search) — Primary changed to sherpa-onnx

A deeper search for newer/updated solutions surfaced **sherpa-onnx (k2-fsa)** as a stronger primary than diarize (FoxNoseTech):

- **Code:** Apache 2.0 — `github.com/k2-fsa/sherpa-onnx`
- **ONNX-native** — primary design goal, no Python runtime
- **Rust bindings** via `sherpa-rs` (`github.com/thewh1teagle/sherpa-rs`) — has a `diarize` example with a clean `Diarize::compute(samples, progress_cb)` API returning `(start, end, speaker)` segments
- **Tauri examples shipped in-repo** (`sherpa-onnx/tauri-examples/`) — proven desktop integration path
- **Active:** v1.13.3 (June 2026), diarization fixes in the same release
- **CPU-fast:** RTF ≈ 0.12 (NeMo TitaNet small) / 0.24 (3D-Speaker) on a 57s clip — 4–8x faster than realtime
- **3-stage pipeline:** segmentation (pyannote-segmentation-3.0, ~6MB) → embedding (NeMo TitaNet small ~23MB or 3D-Speaker ERes2Net ~30MB) → fast clustering (built-in, threshold or fixed-k)

`diarize` (FoxNoseTech) could not be reliably located in a public repository during re-verification; it is retained only as a fallback if its ONNX bundle is independently confirmed. **sherpa-onnx is the locked primary.**

---

## Key finding (corrects earlier assumption)

**DiariZen weights are CC BY-NC 4.0 (non-commercial only)** — NOT safe for a shared/team build. Eliminated.

**reverb-diarization-v1 (Reverb, shipped as a sherpa-onnx segmentation option) is also non-commercial** — we use `sherpa-onnx-pyannote-segmentation-3.0` instead, whose weights inherit PyAnnote's permissive terms (Apache 2.0 / CC-BY-4.0 with attribution — both commercial-safe). License file in the tarball must be verified at download time.

---

## Candidates compared

| Model | Code | Weights | Real-time | ONNX | DER | Size | Commercial-safe |
|---|---|---|---|---|---|---|---|
| **sherpa-onnx (k2-fsa)** ⭐ | Apache 2.0 | Apache 2.0 / CC-BY-4.0* | ❌ batch | ✅ native | ~10% | ~6+23MB | ✅ **Chosen** |
| **diarize (FoxNoseTech)** | Apache 2.0 | Apache 2.0 | ❌ batch | ✅ native | 10.8% | ~26MB | ✅ Fallback |
| **diart** | MIT | CC-BY-4.0* | ✅ 500ms streaming | ⚠️ partial | 9-11% | ~33MB | ⚠️ attribution |
| **pyannote community-1** | MIT | CC-BY-4.0* | ❌ batch | ❌ PyTorch only | ~8% | ~28MB | ⚠️ attribution |
| **NVIDIA NeMo Sortformer** | Apache 2.0 | Apache 2.0 | ✅ streaming | ⚠️ limited | competitive | ~23MB+ | ✅ |
| **WeSpeaker** | Apache 2.0 | Apache 2.0 | ❌ (embedding only) | ✅ native | N/A | ~26MB | ✅ |
| **3D-Speaker (Alibaba)** | Apache 2.0 | Apache 2.0 | ❌ batch | ✅ | N/A | ~30MB | ✅ |
| **SpeechBrain ECAPA-TDNN** | Apache 2.0 | Apache 2.0 | ❌ batch | ⚠️ manual | N/A | ~17MB | ✅ |
| **DiariZen** | Apache 2.0 | **CC BY-NC 4.0** | ❌ batch | ⚠️ community | 9-14% | 278MB | ❌ ELIMINATED |

\* CC-BY-4.0 = commercial use allowed **with attribution**; not fully MIT/permissive but redistributable with credit.

---

## Detailed analysis

### diarize (FoxNoseTech) — `github.com/FoxNoseTech/diarize`
- **v0.1.1, March 2026** — actively maintained
- Stack: Silero VAD (MIT) + WeSpeaker ResNet34-LM (Apache 2.0) + spectral clustering
- **Fully Apache 2.0 / MIT** — no attribution gymnastics, safe to redistribute
- ONNX Runtime native — runs on CPU, 7x faster than pyannote on CPU
- DER 10.8% on VoxConverse — competitive, not SOTA
- **Batch only** (streaming on roadmap)
- No HuggingFace token required — clean download
- **Best license compliance of all options**

### diart — `github.com/juanmc2005/diart`
- MIT code, but loads pyannote segmentation model (CC-BY-4.0 weights)
- **Real-time streaming** (500ms rolling buffer) — only mature streaming option
- Online incremental clustering, overlap-aware
- CPU latency 26-150ms per 500ms step — usable on modest hardware
- ONNX export partial (segmentation can export, embedding via ONNX possible)
- CC-BY-4.0 means we **must attribute pyannote** in our license notice — doable but adds a clause
- **Best for real-time** if we accept attribution requirement

### pyannote community-1 (v4.0, June 2026)
- **CC-BY-4.0 weights** — commercial use OK with attribution
- Best accuracy (~8% DER) — SOTA
- **No ONNX** — pure PyTorch, removed ONNX in 3.1
- Requires HuggingFace token + email to download (friction for team distribution)
- Batch only; slow on CPU (2-4x realtime)
- Would require Python runtime in the Tauri app — heavy dependency
- **Highest accuracy but worst deployment story** for our stack

### NVIDIA NeMo Streaming Sortformer
- Apache 2.0 everything — fully permissive
- Real-time streaming support
- **GPU-centric** — poor CPU performance, requires CUDA in practice
- Heavy framework (NeMo + PyTorch)
- ONNX export limited
- **Best streaming + license, but impractical without NVIDIA GPU**

### WeSpeaker + 3D-Speaker (embedding-only)
- Both Apache 2.0, ONNX-native, fast on CPU
- Provide speaker embeddings only — we'd write our own clustering layer (AHC/spectral)
- Could compose: WeSpeaker embeddings + our clustering = custom diarization pipeline
- **Building blocks, not a turnkey solution**

---

## Recommendation

### Primary: sherpa-onnx (k2-fsa) via `sherpa-rs`
- **Fully permissive** (Apache 2.0 code; segmentation weights Apache 2.0 / CC-BY-4.0 with attribution — commercial-safe)
- **ONNX-native** — fits the Rust/Tauri stack, no Python runtime
- **Rust bindings exist** (`sherpa-rs`) with a working `diarize` example returning `(start, end, speaker)` segments
- **Tauri examples in-repo** — proven desktop integration, lowers F5 implementation risk
- **CPU-fast** — RTF 0.12–0.24 on a 57s clip; trivial for a post-processing step on the same hardware that already runs Whisper/Parakeet
- **3-stage pipeline** (segmentation → embedding → clustering) is the same architecture diarize uses, but turnkey and battle-tested across many languages
- **PT-BR:** segmentation and clustering are language-agnostic; for embedding, prefer **3D-Speaker ERes2Net** (multilingual) over NeMo TitaNet (English-trained) — speaker identity is largely language-independent, but a multilingual embedding model generalizes better. Smoke-test on PT-BR audio before locking.

### Fallback: diarize (FoxNoseTech)
- Retained only if its ONNX bundle is independently confirmed to exist and ship Apache-2.0 weights; sherpa-onnx supersedes it on bindings + Tauri examples + maintenance.

### Future (real-time): diart
- If live speaker labels during transcription become a hard requirement, switch to diart + accept the CC-BY-4.0 attribution requirement for the pyannote segmentation weights.

### Avoid
- ❌ DiariZen — CC BY-NC 4.0, non-commercial only
- ❌ reverb-diarization-v1 (sherpa-onnx alternative segmentation) — non-commercial license
- ❌ pyannote community-1 directly — no ONNX, requires Python runtime, HF token friction
- ❌ NeMo standalone — GPU lock-in, no ONNX speaker-diarization export (issue #14733)

---

## Implementation impact on F5

Using **sherpa-onnx (k2-fsa)** via `sherpa-rs` changes the F5 scope:

- **No Python dependency** — pure ONNX, runs inside the Rust Tauri process via `sherpa-rs` (FFI over sherpa-onnx C++) or directly via the `ort` crate if we vendor the graph
- **Post-processing model** — diarization runs after transcription completes, not streaming
- **Pipeline:** audio → Whisper/Parakeet transcript segments → sherpa-onnx diarize(audio) → merge speaker labels onto segments by timestamp overlap
- **Model downloads added to onboarding:**
  - `sherpa-onnx-pyannote-segmentation-3.0` (~6MB, or 1.5MB int8)
  - `3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx` (~30MB) — preferred for PT-BR, multilingual
  - Fallback embedding: `nemo_en_titanet_small.onnx` (~23MB, fastest RTF 0.12, English-trained)
- **UI change:** show "Identifying speakers..." spinner after transcription, then reveal labels
- **Reduced complexity** from XL → **L** — turnkey pipeline + Rust bindings + Tauri examples eliminate the streaming-clustering and custom-VBx work
- **Integration point:** new `frontend/src-tauri/src/audio_v2/diarization.rs` wrapping `sherpa_rs::diarize::Diarize`, called from the post-transcription step in `transcript_pipeline.rs`

---

## Open items for F5

1. **Verify sherpa-rs maintenance status** — `github.com/thewh1teagle/sherpa-rs`; if archived, fork it or call sherpa-onnx C++ via our own FFI thin wrapper
2. **Verify segmentation weights license** in the `sherpa-onnx-pyannote-segmentation-3.0` tarball (expected Apache 2.0 or CC-BY-4.0 — both commercial-safe; document the attribution in our LICENSE notice)
3. **PT-BR smoke test** — run 3D-Speaker ERes2Net embeddings on a Portuguese meeting clip and confirm sensible speaker counts before locking the embedding model
4. **Decide on Silero VAD vs existing** — Meetily's audio pipeline may already have a VAD step; sherpa-onnx segmentation handles voice activity internally, so we may skip a separate VAD pass

_Next: lock F5 in SCOPE.md, write ARCHITECTURE.md and ROADMAP.md, then start Phase 0._
