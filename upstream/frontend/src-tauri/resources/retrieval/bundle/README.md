# Retrieval model bundle (staged, not committed)

This directory is populated by `frontend/src-tauri/scripts/stage-retrieval-models.ps1`
from the pinned artifacts declared in `../model-bundle.manifest.json`. The fetched
models, tokenizers, and license copies are verified (byte length + SHA-256) and
published here atomically before `tauri build` packages them as application resources.

Everything except this README is excluded from Git; see the root `.gitignore`.
This file also keeps this directory present in Git so the Tauri resource walk
succeeds before the first staging run.
