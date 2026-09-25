"""La biblioteca se vigila sola: lo que cambia en el disco llega al indice.

Antes el indice solo se ponia al dia al pulsar «Analizar e indexar todo».
Mover la carpeta de musica, o una cancion desde el gestor de archivos, dejaba
la app enseñando canciones que ya no estaban, y con la ultima que sono puesta
en el reproductor.

Todo corre en un hilo propio que arranca con el nucleo (`api.serve`):

- **Al arrancar**, antes de que la interfaz pregunte nada, las carpetas que
  ya no estan apartan sus canciones (`prepare`: solo mira si cada carpeta
  existe, es instantaneo). Despues, en segundo plano, un escaneo incremental
  pone al dia el resto: lo que se movio, se borro o llego con la app cerrada.
- **Mientras corre**, `watchdog` avisa de cada cambio dentro de las carpetas
  (el gestor de archivos, un editor de etiquetas, una copia). Cuando deja de
  haber movimiento un par de segundos, se escanea: con los archivos que no
  han cambiado es casi instantaneo, y uno que se movio conserva su id.
- **Cada pocos segundos** se mira si alguna carpeta aparecio o desaparecio
  (un disco que se monta o se desmonta, una carpeta que se mueve entera) y,
  cada diez minutos, un escaneo completo por si algun aviso se perdio por el
  camino (unidades de red, un desbordamiento de inotify).

El escaneo solo avisa a la interfaz si algo cambio (`library._touch`), y de
ahi en adelante es el camino de siempre: Rust ve subir `revision` en
`/api/status` y las ventanas se refrescan solas.

`DANPLAY_WATCH=0` lo apaga entero; `DANPLAY_RESCAN_MINUTES` cambia el repaso
periodico (0 = sin repaso).
"""

import logging
import os
import threading
import time
from pathlib import Path
from typing import Any, cast

from . import config, library

log = logging.getLogger(__name__)

# Segundos sin movimiento antes de escanear: copiar una carpeta son cientos de
# avisos seguidos, y se escanea una vez al final, no cien.
QUIET = 2.0
# Con cambios sin parar (una copia larga), se escanea igualmente cada tanto
# para que lo copiado vaya apareciendo.
MAX_WAIT = 30.0
# Cada cuanto se mira si las carpetas siguen donde estaban.
CHECK_EVERY = 5.0
# Lo que se espera a `prepare` antes de empezar a contestar. Mirar si existe
# una carpeta es instantaneo, salvo en una unidad de red que no responde: ahi
# se sigue sin esperar y se termina en segundo plano.
PREPARE_TIMEOUT = 3.0

# Lo que watchdog cuenta y aqui no importa: abrir y leer un archivo (el propio
# reproductor lo hace con cada cancion) no cambia nada.
_IGNORED = {"opened", "closed_no_write"}


def enabled() -> bool:
    return config.env("DANPLAY_WATCH", "1") not in ("0", "false", "no")


def _rescan_every() -> float:
    try:
        return max(0.0, float(config.env("DANPLAY_RESCAN_MINUTES", "10") or 0)) * 60
    except ValueError:
        return 600.0


class _Events:
    """Lo que watchdog llama con cada aviso (le basta con `dispatch`)."""

    def __init__(self, watcher: "Watcher"):
        self.watcher = watcher

    def dispatch(self, event) -> None:
        kind = event.event_type
        if kind in _IGNORED:
            return
        # Una carpeta «modificada» es que algo cambio dentro, y eso ya avisa
        # por su cuenta; si era un .txt o una caratula, no importa.
        if event.is_directory and kind == "modified":
            return
        for path in (event.src_path, getattr(event, "dest_path", "") or ""):
            if path:
                self.watcher.notify(os.fsdecode(path), event.is_directory)


class Watcher:
    def __init__(self, quiet=QUIET, check_every=CHECK_EVERY, rescan_every=None, observe=True):
        self.quiet = quiet
        self.check_every = check_every
        self.rescan_every = _rescan_every() if rescan_every is None else rescan_every
        self.observe = observe
        self._lock = threading.Lock()
        self._pending_since: float | None = None  # primer aviso aun sin escanear
        self._last_event = 0.0
        self._wake = threading.Event()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._observer = None
        self._watches: dict = {}  # carpeta -> ObservedWatch
        self._present: dict[str, bool] = {}  # carpeta -> existia la ultima vez
        self._known = False  # ya se miraron las carpetas una vez
        self._roots: list[str] = []
        self._exclusions: list = []
        self._last_scan_took = 0.0
        self._recheck = threading.Event()  # mirar las carpetas ya, sin esperar

    # ------------------------------------------------------------ vida
    def start(self) -> None:
        if self._thread is None:
            self._thread = threading.Thread(target=self._run, name="danplay-watcher", daemon=True)
            self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        self._wake.set()
        if self._observer is not None:
            try:
                self._observer.stop()
            except Exception:  # noqa: BLE001
                pass

    def _run(self) -> None:
        self.refresh_roots()
        self.scan("al arrancar")
        last_check = last_full = time.monotonic()
        while not self._stop.is_set():
            self._wake.wait(timeout=1.0)
            self._wake.clear()
            if self._stop.is_set():
                break
            now = time.monotonic()
            if self._recheck.is_set() or now - last_check >= self.check_every:
                self._recheck.clear()
                last_check = now
                if self.refresh_roots():
                    self.mark()  # una carpeta se fue o volvio
            if self.due(time.monotonic()):
                self.scan("cambios en el disco")
                last_full = time.monotonic()
            elif self.rescan_every and now - last_full >= self.rescan_every:
                self.scan("repaso")
                last_full = time.monotonic()

    # ------------------------------------------------------------ escanear
    def scan(self, why: str = "") -> dict | None:
        started = time.monotonic()
        try:
            r = library.scan()
        except Exception:
            log.warning("el escaneo automatico (%s) fallo", why, exc_info=True)
            return None
        finally:
            self._last_scan_took = time.monotonic() - started
        if r.get("changed"):
            log.info(
                "biblioteca al dia (%s): %d nuevas, %d vuelven, %d apartadas, %d releidas",
                why,
                r["added_count"],
                r["back"],
                r["removed"],
                r["updated"],
            )
        return r

    def folders_changed(self) -> None:
        """Se añadio, quito o movio una carpeta desde la app: se vigila ya,
        sin esperar al siguiente repaso (en ese rato un cambio se perdia)."""
        self._recheck.set()
        self._wake.set()

    def mark(self) -> None:
        """Apunta que hay algo que escanear."""
        now = time.monotonic()
        with self._lock:
            if self._pending_since is None:
                self._pending_since = now
            self._last_event = now
        self._wake.set()

    def due(self, now: float) -> bool:
        """Si toca escanear lo apuntado. Se espera a que haya calma; cuanto
        mas tardo el ultimo escaneo, mas calma se pide (una biblioteca enorme
        en un disco lento no debe escanearse sin parar mientras copias)."""
        with self._lock:
            if self._pending_since is None:
                return False
            calm = max(self.quiet, min(self._last_scan_took, MAX_WAIT))
            limit = max(MAX_WAIT, 2 * self._last_scan_took)
            if now - self._last_event >= calm or now - self._pending_since >= limit:
                self._pending_since = None
                return True
            return False

    # ------------------------------------------------------------ avisos
    def notify(self, path: str, is_dir: bool = False) -> None:
        """Un cambio en esa ruta. Solo cuenta si es audio (o una carpeta) de
        una carpeta gestionada y no esta excluido."""
        root = self._root_of(path)
        if root is None:
            return
        if not is_dir and path != root and Path(path).suffix.lower() not in config.EXTENSIONS:
            return
        if path != root and self._excluded(path, root, is_dir):
            return
        self.mark()

    def _root_of(self, path: str) -> str | None:
        for root in self._roots:  # la mas larga primero
            if path == root or path.startswith(root.rstrip(os.sep) + os.sep):
                return root
        return None

    def _excluded(self, path: str, root: str, is_dir: bool) -> bool:
        parts = Path(os.path.relpath(path, root)).parts
        folder = root
        for name in parts if is_dir else parts[:-1]:
            if library._excluded(folder, name, self._exclusions):
                return True
            folder = os.path.join(folder, name)
        return False

    # ------------------------------------------------------------ carpetas
    def refresh_roots(self) -> bool:
        """Relee las carpetas gestionadas y mira si existen. Devuelve True si
        alguna aparecio o desaparecio desde la ultima vez, o si hay una nueva
        (añadida en Ajustes: se indexa sola, sin pulsar «Analizar»)."""
        try:
            roots = library.roots()
            self._exclusions = library.list_exclusions()
        except Exception:
            log.warning("no pude leer las carpetas gestionadas", exc_info=True)
            return False
        present = {r: os.path.isdir(r) for r in roots}
        changed = any(r in self._present and self._present[r] != ok for r, ok in present.items())
        if self._known:
            changed = changed or any(ok and r not in self._present for r, ok in present.items())
        self._known = True
        self._present = present
        self._roots = sorted(roots, key=len, reverse=True)
        if self.observe:
            self._arm()
        return changed

    def _arm(self) -> None:
        """Vigila las carpetas que existen y deja de vigilar las demas."""
        if self._observer is None:
            try:
                from watchdog.observers import Observer
            except ImportError:
                log.warning(
                    "sin watchdog: la biblioteca solo se repasa cada %d min",
                    self.rescan_every // 60,
                )
                self.observe = False
                return
            self._observer = Observer()
            self._observer.daemon = True
            self._observer.start()
        wanted = {r for r, ok in self._present.items() if ok}
        for root in [r for r in self._watches if r not in wanted]:
            try:
                self._observer.unschedule(self._watches.pop(root))
            except Exception:  # noqa: BLE001
                pass  # se habia ido con su carpeta
        for root in wanted - set(self._watches):
            try:
                # `_Events` hace de manejador (le basta con `dispatch`) sin
                # importar watchdog al cargar este modulo
                handler = cast(Any, _Events(self))
                self._watches[root] = self._observer.schedule(handler, root, recursive=True)
            except OSError as e:
                # sin vigilancia en esa carpeta (el tope de inotify, un
                # sistema de archivos que no avisa): queda el repaso periodico
                log.warning(
                    "no puedo vigilar %s (%s): se repasara cada %d min",
                    root,
                    e,
                    self.rescan_every // 60,
                )


_WATCHER: Watcher | None = None


def prepare(timeout: float = PREPARE_TIMEOUT) -> None:
    """Aparta ya las canciones de las carpetas que no estan, con tope."""

    def hide():
        try:
            n = library.hide_missing_roots()
            if n:
                log.info("%d canciones apartadas: su carpeta ya no esta", n)
        except Exception:
            log.warning("no pude mirar las carpetas al arrancar", exc_info=True)

    t = threading.Thread(target=hide, name="danplay-prepare", daemon=True)
    t.start()
    t.join(timeout)


def folders_changed() -> None:
    """Avisa al vigilante (si esta en marcha) de que las carpetas cambiaron."""
    if _WATCHER is not None:
        _WATCHER.folders_changed()


def start() -> Watcher | None:
    """Arranca la vigilancia (una sola vez por proceso)."""
    global _WATCHER
    if not enabled():
        return None
    if _WATCHER is None:
        _WATCHER = Watcher()
        _WATCHER.start()
    return _WATCHER


def stop() -> None:
    """Para la vigilancia (al apagar el nucleo). Se puede volver a arrancar."""
    global _WATCHER
    if _WATCHER is not None:
        _WATCHER.stop()
        _WATCHER = None
