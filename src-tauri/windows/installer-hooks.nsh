; NSIS hooks for the tuldMi installer (tauri.conf.json bundle.windows.nsis.installerHooks).
;
; tuldmi-mcp.exe is started by AI assistants, not by tuldMi, so closing the app before an
; upgrade doesn't stop it, and Windows won't let the installer replace a running executable.
; tuldmi-speakers.exe normally exits with the app; stop a straggler too. An assistant simply
; starts tuldmi-mcp again on its next request.
!macro NSIS_HOOK_PREINSTALL
  nsExec::Exec 'taskkill /F /T /IM tuldmi-mcp.exe'
  Pop $0
  nsExec::Exec 'taskkill /F /T /IM tuldmi-speakers.exe'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'taskkill /F /T /IM tuldmi-mcp.exe'
  Pop $0
  nsExec::Exec 'taskkill /F /T /IM tuldmi-speakers.exe'
  Pop $0
!macroend
