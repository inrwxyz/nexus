!macro NSIS_HOOK_PREINSTALL
  nsExec::Exec 'taskkill /F /T /IM nexus.exe'
  nsExec::Exec 'taskkill /F /T /IM adb.exe'
  nsExec::Exec '"$SYSDIR\cmd.exe" /c taskkill /F /T /IM adb.exe'
  nsExec::Exec '"$SYSDIR\cmd.exe" /c taskkill /F /T /IM nexus.exe'
  Sleep 1200
  Delete "$INSTDIR\_up_\assets\adb.exe"
  Delete "$INSTDIR\_up_\assets\AdbWinApi.dll"
  Delete "$INSTDIR\_up_\assets\AdbWinUsbApi.dll"
  Delete "$INSTDIR\${MAINBINARYNAME}.exe"
  Sleep 500
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'taskkill /F /T /IM nexus.exe'
  nsExec::Exec 'taskkill /F /T /IM adb.exe'
  nsExec::Exec '"$SYSDIR\cmd.exe" /c taskkill /F /T /IM adb.exe'
  nsExec::Exec '"$SYSDIR\cmd.exe" /c taskkill /F /T /IM nexus.exe'
  Sleep 1000
  RMDir /r "$LOCALAPPDATA\Nexus"
!macroend
