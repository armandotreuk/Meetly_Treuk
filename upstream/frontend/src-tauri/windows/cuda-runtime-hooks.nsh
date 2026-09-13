; Tauri places the `resources/cuda-runtime` mapping below its one installer
; resource root, so the staged files live at $INSTDIR\resources\cuda-runtime.
; Copy only those redistributable runtime DLLs alongside the executable because
; Windows resolves CUDA import libraries before the Tauri resource directory.
!macro NSIS_HOOK_POSTINSTALL
  CopyFiles /SILENT "$INSTDIR\resources\cuda-runtime\*.dll" "$INSTDIR"
!macroend
