/* danplay-core.exe: arranca el nucleo Python que viaja a su lado.
 *
 * Por que existe: la aplicacion busca un ejecutable llamado `danplay-core`
 * (el sidecar de Tauri). Cuando el nucleo se empaqueta con PyInstaller, ese
 * ejecutable ES el nucleo. Cuando se empaqueta con el Python embebido de
 * Windows —que es la unica forma de construirlo desde Linux, porque
 * PyInstaller no compila para otro sistema— lo que hay es un `python.exe` y
 * una carpeta de dependencias, asi que hace falta esta pieza de tres lineas
 * que lanza uno con el otro.
 *
 * Le pasa sus mismos argumentos, hereda la entrada y la salida, y devuelve el
 * mismo codigo de salida: para quien lo llama es indistinguible del nucleo.
 *
 * ESPERA a que Python termine en vez de sustituirse por el. En Windows no
 * existe exec de verdad —la biblioteca de C crea un proceso nuevo y termina el
 * actual—, asi que si nos fueramos, la aplicacion veria morir a su hijo al
 * instante, creeria que el nucleo se ha caido y lo levantaria otra vez, sin
 * parar. Quedandose, el proceso que la aplicacion vigila vive lo que viva
 * Python, y el Job Object se lleva a los dos al cerrar.
 */
#include <windows.h>
#include <process.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int wmain(int argc, wchar_t **argv) {
    wchar_t exe[MAX_PATH];
    if (!GetModuleFileNameW(NULL, exe, MAX_PATH)) {
        fwprintf(stderr, L"danplay-core: no se donde estoy\n");
        return 1;
    }
    /* la carpeta donde vivo */
    wchar_t *slash = wcsrchr(exe, L'\\');
    if (slash) *slash = L'\0';

    wchar_t python[MAX_PATH];
    _snwprintf(python, MAX_PATH, L"%s\\python\\python.exe", exe);
    if (GetFileAttributesW(python) == INVALID_FILE_ATTRIBUTES) {
        fwprintf(stderr, L"danplay-core: falta %s\n", python);
        return 1;
    }

    /* python.exe -m danplay.cli serve <lo que me hayan pasado> */
    wchar_t **args = (wchar_t **)calloc(argc + 5, sizeof(wchar_t *));
    if (!args) return 1;
    int n = 0;
    args[n++] = python;
    args[n++] = L"-m";
    args[n++] = L"danplay.cli";
    args[n++] = L"serve";
    for (int i = 1; i < argc; i++) args[n++] = argv[i];
    args[n] = NULL;

    intptr_t status = _wspawnv(_P_WAIT, python, (const wchar_t *const *)args);
    free(args);
    if (status == -1) {
        fwprintf(stderr, L"danplay-core: no pude arrancar %s\n", python);
        return 1;
    }
    return (int)status;
}
