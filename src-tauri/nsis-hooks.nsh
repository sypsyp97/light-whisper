!macro LIGHT_WHISPER_COMMAND arguments
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\resources\install_cli_command.ps1" -InstallRoot "$INSTDIR" ${arguments}'
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint "$1"
    SetErrorLevel 1
    Abort "Could not configure the light-whisper command. $1"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro LIGHT_WHISPER_COMMAND ""
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  !insertmacro LIGHT_WHISPER_COMMAND "-Uninstall"
!macroend
