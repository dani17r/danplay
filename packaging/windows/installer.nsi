; Instalador de DanPlay para Windows.
;
; Se compila con makensis DESDE LINUX (scripts/build-windows-cross.sh
; --instalador), sobre la misma carpeta que produce la version portatil. Es
; decir: portable e instalador salen de lo mismo y no pueden desincronizarse.
;
; Instala PARA EL USUARIO, en %LOCALAPPDATA%\Programs\DanPlay:
;
;   - No pide administrador, asi que no sale el aviso de UAC.
;   - No toca a los demas usuarios del equipo.
;   - Todo el registro va a HKCU.
;
; Sobre «reproductor predeterminado»: el instalador deja a DanPlay REGISTRADO
; (sale en «Abrir con» y en la lista de aplicaciones predeterminadas de
; Windows), pero NO puede elegirlo por ti. Desde Windows 8 esa eleccion vive en
; una clave protegida con un hash y el sistema deshace cualquier intento de
; escribirla. Por eso al terminar se ofrece abrir la pagina de Ajustes, que es
; donde das el ultimo clic. Es lo mismo que hacen VLC, Spotify y foobar2000.
;
; Parametros (se pasan con -D):
;   VERSION  el numero de version
;   ORIGEN   la carpeta con todo lo que hay que meter dentro
;   SALIDA   el .exe que se genera
;   ICONO    el .ico de la aplicacion

Unicode true
ManifestDPIAwareness PerMonitorV2

!define NOMBRE "DanPlay"
!define EJECUTABLE "danplay-app.exe"
!define EDITOR "dani17r"
!define CLAVE "Software\DanPlay"
!define CAPACIDADES "Software\DanPlay\Capabilities"
!define DESINSTALAR "Software\Microsoft\Windows\CurrentVersion\Uninstall\DanPlay"

Name "${NOMBRE} ${VERSION}"
OutFile "${SALIDA}"
InstallDir "$LOCALAPPDATA\Programs\${NOMBRE}"
InstallDirRegKey HKCU "${CLAVE}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1034 "ProductName" "${NOMBRE}"
VIAddVersionKey /LANG=1034 "FileDescription" "Instalador de ${NOMBRE}"
VIAddVersionKey /LANG=1034 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1034 "LegalCopyright" "${EDITOR}"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!define MUI_ICON "${ICONO}"
!define MUI_UNICON "${ICONO}"
!define MUI_ABORTWARNING

!define MUI_FINISHPAGE_RUN "$INSTDIR\${EJECUTABLE}"
!define MUI_FINISHPAGE_RUN_TEXT "Abrir DanPlay"
; La segunda casilla de la ultima pagina: lleva a Ajustes de Windows, que es
; donde de verdad se elige el reproductor predeterminado.
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Elegir DanPlay como reproductor predeterminado"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION AbrirAjustes
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
!macro Asociar EXT DESCRIPCION
  WriteRegStr HKCU "Software\Classes\DanPlay.${EXT}" "" "${DESCRIPCION}"
  WriteRegStr HKCU "Software\Classes\DanPlay.${EXT}\DefaultIcon" "" '"$INSTDIR\${EJECUTABLE}",0'
  WriteRegStr HKCU "Software\Classes\DanPlay.${EXT}\shell\open" "" "Abrir con ${NOMBRE}"
  WriteRegStr HKCU "Software\Classes\DanPlay.${EXT}\shell\open\command" "" '"$INSTDIR\${EJECUTABLE}" "%1"'
  WriteRegStr HKCU "Software\Classes\.${EXT}\OpenWithProgids" "DanPlay.${EXT}" ""
  WriteRegStr HKCU "${CAPACIDADES}\FileAssociations" ".${EXT}" "DanPlay.${EXT}"
!macroend

!macro Desasociar EXT
  DeleteRegKey HKCU "Software\Classes\DanPlay.${EXT}"
  DeleteRegValue HKCU "Software\Classes\.${EXT}\OpenWithProgids" "DanPlay.${EXT}"
!macroend

; ------------------------------------------------------------------- ayudas

; Windows no deja sobrescribir un .exe que se esta ejecutando, y el error que
; sale por su cuenta no dice que hacer. Esto lo dice.
Function ComprobarSiEstaAbierta
  ${IfNot} ${FileExists} "$INSTDIR\${EJECUTABLE}"
    Return
  ${EndIf}
  otra_vez:
    ClearErrors
    ; Renombrarlo es la unica forma fiable de saber si esta en uso sin
    ; plugins de terceros: si esta abierto, Windows no deja.
    Rename "$INSTDIR\${EJECUTABLE}" "$INSTDIR\${EJECUTABLE}.viejo"
    ${If} ${Errors}
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION \
        "DanPlay esta abierto.$\n$\nCierralo del todo (icono de la bandeja, boton derecho, Salir) y vuelve a intentarlo." \
        IDRETRY otra_vez
      Abort "DanPlay estaba abierto."
    ${EndIf}
    Delete "$INSTDIR\${EJECUTABLE}.viejo"
FunctionEnd

; Ajustes > Aplicaciones predeterminadas, ya filtrado por DanPlay.
Function AbrirAjustes
  ExecShell "open" "ms-settings:defaultapps?registeredAppName=DanPlay"
FunctionEnd

; Que el explorador se entere de los iconos y el «Abrir con» nuevos sin
; reiniciar sesion. SHCNE_ASSOCCHANGED | SHCNF_IDLIST.
!macro RefrescarExplorador
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

; ------------------------------------------------------------------ secciones

Section "DanPlay" SeccionApp
  SectionIn RO
  Call ComprobarSiEstaAbierta

  SetOutPath "$INSTDIR"
  ; Todo lo que produce la version portatil: la aplicacion, el nucleo, el
  ; Python que lo mueve y las herramientas externas si estan.
  File /r "${ORIGEN}\*.*"

  WriteRegStr HKCU "${CLAVE}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${CLAVE}" "Version" "${VERSION}"
  WriteUninstaller "$INSTDIR\desinstalar.exe"

  ; La ficha de «Aplicaciones instaladas»
  WriteRegStr HKCU "${DESINSTALAR}" "DisplayName" "${NOMBRE}"
  WriteRegStr HKCU "${DESINSTALAR}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${DESINSTALAR}" "DisplayIcon" '"$INSTDIR\${EJECUTABLE}",0'
  WriteRegStr HKCU "${DESINSTALAR}" "Publisher" "${EDITOR}"
  WriteRegStr HKCU "${DESINSTALAR}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${DESINSTALAR}" "UninstallString" '"$INSTDIR\desinstalar.exe"'
  WriteRegStr HKCU "${DESINSTALAR}" "QuietUninstallString" '"$INSTDIR\desinstalar.exe" /S'
  WriteRegDWORD HKCU "${DESINSTALAR}" "NoModify" 1
  WriteRegDWORD HKCU "${DESINSTALAR}" "NoRepair" 1
  ; El tamaño que enseña Windows, en KB. Sin esto pone «desconocido».
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${DESINSTALAR}" "EstimatedSize" "$0"

  CreateShortcut "$SMPROGRAMS\${NOMBRE}.lnk" "$INSTDIR\${EJECUTABLE}"
SectionEnd

Section "Acceso directo en el escritorio" SeccionEscritorio
  CreateShortcut "$DESKTOP\${NOMBRE}.lnk" "$INSTDIR\${EJECUTABLE}"
SectionEnd

Section "Abrir canciones con DanPlay" SeccionAsociar
  ; Los ocho formatos que DanPlay sabe leer. Tienen que ser los mismos que
  ; declara `bundle.fileAssociations` en tauri.conf.json; hay una prueba que
  ; lo comprueba (tests/test_core.py).
  !insertmacro Asociar "mp3"  "Canción MP3"
  !insertmacro Asociar "flac" "Canción FLAC"
  !insertmacro Asociar "wav"  "Sonido WAV"
  !insertmacro Asociar "m4a"  "Canción M4A"
  !insertmacro Asociar "ogg"  "Canción OGG"
  !insertmacro Asociar "opus" "Canción Opus"
  !insertmacro Asociar "aac"  "Canción AAC"
  !insertmacro Asociar "wma"  "Canción WMA"

  ; Para que DanPlay salga en Ajustes > Aplicaciones predeterminadas
  WriteRegStr HKCU "${CAPACIDADES}" "ApplicationName" "${NOMBRE}"
  WriteRegStr HKCU "${CAPACIDADES}" "ApplicationDescription" "Gestor de biblioteca musical"
  WriteRegStr HKCU "Software\RegisteredApplications" "${NOMBRE}" "${CAPACIDADES}"
  !insertmacro RefrescarExplorador
SectionEnd

LangString DESC_App        ${LANG_SPANISH} "La aplicación, el núcleo y todo lo que necesita para funcionar."
LangString DESC_Escritorio ${LANG_SPANISH} "Deja un icono de DanPlay en el escritorio."
LangString DESC_Asociar    ${LANG_SPANISH} "Hace que DanPlay aparezca en «Abrir con» para tus canciones y en la lista de reproductores de Windows."

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SeccionApp}        $(DESC_App)
  !insertmacro MUI_DESCRIPTION_TEXT ${SeccionEscritorio} $(DESC_Escritorio)
  !insertmacro MUI_DESCRIPTION_TEXT ${SeccionAsociar}    $(DESC_Asociar)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; -------------------------------------------------------------- desinstalar

Section "Uninstall"
  ; Nunca se borra una carpeta a ciegas: si ahi no esta el ejecutable, esto no
  ; es una instalacion de DanPlay y no se toca nada. `RMDir /r` sobre una
  ; carpeta equivocada se lleva por delante lo que haya dentro.
  ${IfNot} ${FileExists} "$INSTDIR\${EJECUTABLE}"
    MessageBox MB_ICONSTOP "No encuentro DanPlay en $INSTDIR. No se ha borrado nada."
    Abort
  ${EndIf}

  Delete "$SMPROGRAMS\${NOMBRE}.lnk"
  Delete "$DESKTOP\${NOMBRE}.lnk"

  !insertmacro Desasociar "mp3"
  !insertmacro Desasociar "flac"
  !insertmacro Desasociar "wav"
  !insertmacro Desasociar "m4a"
  !insertmacro Desasociar "ogg"
  !insertmacro Desasociar "opus"
  !insertmacro Desasociar "aac"
  !insertmacro Desasociar "wma"
  DeleteRegValue HKCU "Software\RegisteredApplications" "${NOMBRE}"
  DeleteRegKey HKCU "${CLAVE}"
  DeleteRegKey HKCU "${DESINSTALAR}"
  !insertmacro RefrescarExplorador

  RMDir /r "$INSTDIR"

  ; La biblioteca y los ajustes NO se borran: estan en %LOCALAPPDATA%\danplay
  ; y %APPDATA%\danplay, y reinstalar tiene que devolverte tu musica tal como
  ; la dejaste. Quien quiera empezar de cero puede borrar esas dos carpetas.
SectionEnd
