; Tauri preserves the source directory below $RESOURCE, so the staged files
; live at $INSTDIR\resources\resources\cuda-runtime. Copy only those
; redistributable runtime DLLs alongside the executable because Windows
; resolves CUDA import libraries before the Tauri resource directory.
!macro NSIS_HOOK_POSTINSTALL
  CopyFiles /SILENT "$INSTDIR\resources\resources\cuda-runtime\*.dll" "$INSTDIR"
!macroend
