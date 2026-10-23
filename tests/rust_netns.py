"""Lo que acompaña al servidor de Rust de las pruebas DENTRO de su red
propia (ver tests/api_core.py): reenvía conexiones entre esa red y la del
equipo, que no se ven, y lanza el servidor.

    python3 -I rust_netns.py [--forward ESCUCHA=DESTINO]... -- PROGRAMA ARGS...

ESCUCHA y DESTINO son `unix:RUTA` o `tcp:HOST:PUERTO`. Un socket Unix con
ruta es del disco, no de la red: se ve desde las dos. Así:

- `unix:api.sock=tcp:127.0.0.1:8730`: la API por un puerto (lo que hace
  falta para la guardia de `Host`), alcanzable desde las pruebas por el
  socket. El socket se abre cuando el puerto ya contesta: que exista es que
  el servidor escucha.
- `tcp:127.0.0.1:11434=unix:ia.sock`: un proveedor de IA de mentira de las
  pruebas, donde el servidor espera el suyo.

Solo la biblioteca estándar: se ejecuta con `-I`, fuera del proyecto. Sale
con el código del servidor; un SIGTERM se le pasa a él.
"""

import argparse
import contextlib
import os
import signal
import socket
import subprocess
import sys
import threading
import time


def _address(text: str):
    kind, _, rest = text.partition(":")
    if kind == "unix":
        return socket.AF_UNIX, rest
    if kind == "tcp":
        host, _, port = rest.rpartition(":")
        family = socket.AF_INET6 if ":" in host else socket.AF_INET
        return family, (host.strip("[]"), int(port))
    raise SystemExit(f"rust_netns: no sé qué es «{text}»")


def _pump(src: socket.socket, dst: socket.socket) -> None:
    try:
        while data := src.recv(65536):
            dst.sendall(data)
    except OSError:
        pass
    finally:
        with contextlib.suppress(OSError):
            dst.shutdown(socket.SHUT_WR)


def _serve(listener: socket.socket, target) -> None:
    family, address = target
    while True:
        try:
            conn, _ = listener.accept()
        except OSError:
            return
        out = socket.socket(family, socket.SOCK_STREAM)
        try:
            out.connect(address)
        except OSError:
            conn.close()
            out.close()
            continue
        for a, b in ((conn, out), (out, conn)):
            threading.Thread(target=_pump, args=(a, b), daemon=True).start()


def _reachable(target) -> bool:
    family, address = target
    probe = socket.socket(family, socket.SOCK_STREAM)
    try:
        probe.connect(address)
        return True
    except OSError:
        return False
    finally:
        probe.close()


def _listen(spec: str, server: subprocess.Popen | None = None) -> None:
    """Escucha en un lado y lleva cada conexión al otro. Con `server`, el
    destino es él: se espera a que conteste antes de escuchar."""
    here, _, there = spec.partition("=")
    (family, address), target = _address(here), _address(there)
    while server is not None and not _reachable(target):
        if server.poll() is not None:
            return
        time.sleep(0.01)
    listener = socket.socket(family, socket.SOCK_STREAM)
    if family != socket.AF_UNIX:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        listener.bind(address)
    except OSError:
        if family != socket.AF_INET6:
            raise
        return  # una red sin IPv6: con 127.0.0.1 basta
    listener.listen(64)
    threading.Thread(target=_serve, args=(listener, target), daemon=True).start()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--forward", action="append", default=[])
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    a = ap.parse_args()
    cmd = a.cmd[1:] if a.cmd and a.cmd[0] == "--" else a.cmd
    # hacia lo de las pruebas, antes: el servidor puede llamar al empezar
    toward_server = [f for f in a.forward if f.startswith("unix:")]
    for spec in a.forward:
        if spec not in toward_server:
            _listen(spec)
    child = subprocess.Popen(cmd)
    for spec in toward_server:
        threading.Thread(target=_listen, args=(spec, child), daemon=True).start()

    def forward(signum, _frame):
        with contextlib.suppress(ProcessLookupError):
            child.send_signal(signum)

    signal.signal(signal.SIGTERM, forward)
    signal.signal(signal.SIGINT, forward)
    code = child.wait()
    for spec in toward_server:
        with contextlib.suppress(OSError):
            os.unlink(spec.partition("=")[0].removeprefix("unix:"))
    return code


if __name__ == "__main__":
    sys.exit(main())
