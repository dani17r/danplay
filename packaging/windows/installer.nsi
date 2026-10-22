; Instalador de DanPlay para Windows.
;
; Se compila con makensis DESDE LINUX (scripts/build-windows-cross.sh
; --installer), sobre la misma carpeta que produce la version portatil. Es
; decir: portable e instalador salen de lo mismo y no pueden desincronizarse.
;
; Instala PARA EL USUARIO, en %LOCALAPPDATA%\Programs\DanPlay:
;
;   - No pide administrador, asi que no sale el aviso de UAC.
;   - No toca a los demas usuarios del equipo.
;   - Todo el registro va a HKCU.
;
; Actualizar es volver a ejecutar el instalador: va a la misma carpeta (la
; recuerda del registro) y a la misma ficha de «Aplicaciones instaladas», quita
; lo que dejo la version anterior y copia la nueva. Nunca hay dos DanPlay. La
; biblioteca y los ajustes viven fuera (%LOCALAPPDATA%\danplay, %APPDATA%\danplay)
; y no se tocan ni al actualizar ni al desinstalar.
;
; Sobre «reproductor predeterminado»: el instalador deja a DanPlay REGISTRADO
; (sale en «Abrir con» y en la lista de aplicaciones predeterminadas de
; Windows), pero NO puede elegirlo por ti. Desde Windows 8 esa eleccion vive en
; una clave protegida con un hash y el sistema deshace cualquier intento de
; escribirla. Por eso al terminar se ofrece abrir la pagina de Ajustes, que es
; donde das el ultimo clic. Es lo mismo que hacen VLC, Spotify y foobar2000.
;
; Parametros (se pasan con -D):
;   VERSION     el numero de version
;   SOURCE_DIR  la carpeta con todo lo que hay que meter dentro
;   OUT_FILE    el .exe que se genera
;   APP_ICON    el .ico de la aplicacion

Unicode true
ManifestDPIAwareness PerMonitorV2

!define APP_NAME "DanPlay"
!define APP_EXE "danplay-app.exe"
!define PUBLISHER "dani17r"
!define REG_KEY "Software\DanPlay"
!define CAPABILITIES_KEY "Software\DanPlay\Capabilities"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\DanPlay"

Name "${APP_NAME} ${VERSION}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "${REG_KEY}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1034 "ProductName" "${APP_NAME}"
VIAddVersionKey /LANG=1034 "FileDescription" "Instalador de ${APP_NAME}"
VIAddVersionKey /LANG=1034 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1034 "LegalCopyright" "${PUBLISHER}"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"
!define MUI_ABORTWARNING

!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Abrir DanPlay"
; La segunda casilla de la ultima pagina: lleva a Ajustes de Windows, que es
; donde de verdad se elige el reproductor predeterminado.
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Elegir DanPlay como reproductor predeterminado"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION OpenDefaultAppsSettings
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "Spanish"

; ------------------------------------------------------------- asociaciones

; Un tipo de archivo: la ficha propia, el «Abrir con» y la capacidad que mira
; Windows para listarnos entre los reproductores.
;
; OJO con `OpenWithProgids`: AÑADE a DanPlay a la lista de «Abrir con» sin
; quitar a nadie. Escribir en `Software\Classes\.mp3` el valor por defecto si
; que pisaria al programa que estuviera puesto, y eso no se hace: la eleccion
; es del usuario.
!macro Associate EXT DESCRIPTION
  WriteRegStr HKCU "Software\Classes\DanPlay.${EXT}" "" "${DESCRIPTION}"
  WriteRegStr HKCU "Software\Classes\DanPlay.${EXT}\DefaultIcon" "" '"$INSTDIR\${APP_EXE}",0'
  WriteRegStr HKCU "Software\Classes\DanPlay.${EXT}\shell\open" "" "Abrir con ${APP_NAME}"
  WriteRegStr HKCU "Software\Classes\DanPlay.${EXT}\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
  WriteRegStr HKCU "Software\Classes\.${EXT}\OpenWithProgids" "DanPlay.${EXT}" ""
  WriteRegStr HKCU "${CAPABILITIES_KEY}\FileAssociations" ".${EXT}" "DanPlay.${EXT}"
!macroend

!macro Dissociate EXT
  DeleteRegKey HKCU "Software\Classes\DanPlay.${EXT}"
  DeleteRegValue HKCU "Software\Classes\.${EXT}\OpenWithProgids" "DanPlay.${EXT}"
!macroend

; ------------------------------------------------------------------- ayudas

; Windows no deja sobrescribir un .exe que se esta ejecutando, y el error que
; sale por su cuenta no dice que hacer. Esto lo dice.
Function CheckNotRunning
  ${IfNot} ${FileExists} "$INSTDIR\${APP_EXE}"
    Return
  ${EndIf}
  retry:
    ClearErrors
    ; Renombrarlo es la unica forma fiable de saber si esta en uso sin
    ; plugins de terceros: si esta abierto, Windows no deja.
    Rename "$INSTDIR\${APP_EXE}" "$INSTDIR\${APP_EXE}.old"
    ${If} ${Errors}
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION \
        "DanPlay esta abierto.$\n$\nCierralo del todo (icono de la bandeja, boton derecho, Salir) y vuelve a intentarlo." \
        IDRETRY retry
      Abort "DanPlay estaba abierto."
    ${EndIf}
    Delete "$INSTDIR\${APP_EXE}.old"
FunctionEnd

; Ajustes > Aplicaciones predeterminadas, ya filtrado por DanPlay.
Function OpenDefaultAppsSettings
  ExecShell "open" "ms-settings:defaultapps?registeredAppName=DanPlay"
FunctionEnd

; Que el explorador se entere de los iconos y el «Abrir con» nuevos sin
; reiniciar sesion. SHCNE_ASSOCCHANGED | SHCNF_IDLIST.
!macro RefreshExplorer
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

; ------------------------------------------------------------------ secciones

; Al actualizar, fuera lo que dejo la instalacion anterior ANTES de copiar la
; nueva: copiar encima deja huerfano lo que la version nueva ya no trae (un
; paquete de Python, una herramienta) y eso acaba dando fallos raros. Se borra
; solo lo nuestro, pieza a pieza; nunca la carpeta entera a ciegas, que el
; usuario pudo elegir una que ya tuviera cosas suyas.
Function RemovePreviousInstall
  ${IfNot} ${FileExists} "$INSTDIR\${APP_EXE}"
    Return
  ${EndIf}
  DetailPrint "Quitando la version anterior de $INSTDIR"
  RMDir /r "$INSTDIR\python"
  RMDir /r "$INSTDIR\tools"
  RMDir /r "$INSTDIR\danplay-core"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\danplay-core.exe"
  Delete "$INSTDIR\WebView2Loader.dll"
  Delete "$INSTDIR\LEEME.txt"
  Delete "$INSTDIR\desinstalar.exe"
FunctionEnd

Section "DanPlay" SectionApp
  SectionIn RO
  Call CheckNotRunning
  Call RemovePreviousInstall

  SetOutPath "$INSTDIR"
  ; Todo lo que produce la version portatil: la aplicacion, el nucleo, el
  ; Python que lo mueve y las herramientas externas si estan.
  File /r "${SOURCE_DIR}\*.*"

  WriteRegStr HKCU "${REG_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${REG_KEY}" "Version" "${VERSION}"
  WriteUninstaller "$INSTDIR\desinstalar.exe"

  ; La ficha de «Aplicaciones instaladas»
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\${APP_EXE}",0'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\desinstalar.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\desinstalar.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ; El tamaño que enseña Windows, en KB. Sin esto pone «desconocido».
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"

  CreateShortcut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
SectionEnd

Section "Acceso directo en el escritorio" SectionDesktop
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
SectionEnd

Section "Abrir canciones con DanPlay" SectionAssociate
  ; Los ocho formatos que DanPlay sabe leer. Tienen que ser los mismos que
  ; declara `bundle.fileAssociations` en tauri.conf.json; hay una prueba que
  ; lo comprueba (tests/test_core.py).
  !insertmacro Associate "mp3"  "Canción MP3"
  !insertmacro Associate "flac" "Canción FLAC"
  !insertmacro Associate "wav"  "Sonido WAV"
  !insertmacro Associate "m4a"  "Canción M4A"
  !insertmacro Associate "ogg"  "Canción OGG"
  !insertmacro Associate "opus" "Canción Opus"
  !insertmacro Associate "aac"  "Canción AAC"
  !insertmacro Associate "wma"  "Canción WMA"

  ; Para que DanPlay salga en Ajustes > Aplicaciones predeterminadas
  WriteRegStr HKCU "${CAPABILITIES_KEY}" "ApplicationName" "${APP_NAME}"
  WriteRegStr HKCU "${CAPABILITIES_KEY}" "ApplicationDescription" "Gestor de biblioteca musical"
  WriteRegStr HKCU "Software\RegisteredApplications" "${APP_NAME}" "${CAPABILITIES_KEY}"
  !insertmacro RefreshExplorer
SectionEnd

LangString DESC_App       ${LANG_SPANISH} "La aplicación, el núcleo y todo lo que necesita para funcionar."
LangString DESC_Desktop   ${LANG_SPANISH} "Deja un icono de DanPlay en el escritorio."
LangString DESC_Associate ${LANG_SPANISH} "Hace que DanPlay aparezca en «Abrir con» para tus canciones y en la lista de reproductores de Windows."

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SectionApp}       $(DESC_App)
  !insertmacro MUI_DESCRIPTION_TEXT ${SectionDesktop}   $(DESC_Desktop)
  !insertmacro MUI_DESCRIPTION_TEXT ${SectionAssociate} $(DESC_Associate)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; -------------------------------------------------------------- desinstalar

Section "Uninstall"
  ; Nunca se borra una carpeta a ciegas: si ahi no esta el ejecutable, esto no
  ; es una instalacion de DanPlay y no se toca nada. `RMDir /r` sobre una
  ; carpeta equivocada se lleva por delante lo que haya dentro.
  ${IfNot} ${FileExists} "$INSTDIR\${APP_EXE}"
    MessageBox MB_ICONSTOP "No encuentro DanPlay en $INSTDIR. No se ha borrado nada."
    Abort
  ${EndIf}

  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$DESKTOP\${APP_NAME}.lnk"

  !insertmacro Dissociate "mp3"
  !insertmacro Dissociate "flac"
  !insertmacro Dissociate "wav"
  !insertmacro Dissociate "m4a"
  !insertmacro Dissociate "ogg"
  !insertmacro Dissociate "opus"
  !insertmacro Dissociate "aac"
  !insertmacro Dissociate "wma"
  DeleteRegValue HKCU "Software\RegisteredApplications" "${APP_NAME}"
  DeleteRegKey HKCU "${REG_KEY}"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  !insertmacro RefreshExplorer

  RMDir /r "$INSTDIR"

  ; La biblioteca y los ajustes NO se borran: estan en %LOCALAPPDATA%\danplay
  ; y %APPDATA%\danplay, y reinstalar tiene que devolverte tu musica tal como
  ; la dejaste. Quien quiera empezar de cero puede borrar esas dos carpetas.
SectionEnd
