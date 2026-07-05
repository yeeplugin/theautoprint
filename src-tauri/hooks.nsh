!macro NSIS_HOOK_POSTINSTALL
  CopyFiles "$INSTDIR\resources\resources\pdfium.dll" "$INSTDIR\pdfium.dll"
  CopyFiles "$INSTDIR\resources\pdfium.dll" "$INSTDIR\pdfium.dll"
!macroend
