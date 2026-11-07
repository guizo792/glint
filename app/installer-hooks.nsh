; Uninstalling also removes the start-with-Windows entry that Glint itself creates.
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Glint"
!macroend
