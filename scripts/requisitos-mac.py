"""Los requisitos del nucleo para este Mac, y en que archivo estan.

    pip install -r "$(python scripts/requisitos-mac.py)"

En Apple Silicon valen los de siempre (requirements.txt). onnxruntime (el
separador de pistas y el pulso) dejo de publicarse para Mac Intel en la 1.24:
ahi va la 1.23.2, la ultima que hay (probado: los tres modelos de DanPlay
cargan con ella), con lo que ella pide y la 1.30 ya no (coloredlogs y sympy),
todo con su hash. Esa lista se escribe en requisitos-mac.txt.

Lo usan los dos trabajos de .github/workflows/macos.yml: las pruebas y el
paquete.
"""

import platform
import re
import sys
from pathlib import Path

INTEL = """\
onnxruntime==1.23.2 --hash=sha256:162f4ca894ec3de1a6fd53589e511e06ecdc3ff646849b62a9da7489dee9ce95
coloredlogs==15.0.1 --hash=sha256:612ee75c546f53e92e70049c9dbfcc18c935a2b9a53b66085ce9ef6a6e5c0934
humanfriendly==10.0 --hash=sha256:1697e1a8a8f550fd43c2865cd84542fc175a61dcb779b6fee18cf6b6ccba1477
sympy==1.14.0 --hash=sha256:e091cc3e99d2141a0ba2847328f5479b05d94a6635cb96148ccb3f34671bd8f5
mpmath==1.3.0 --hash=sha256:a0b2b9fe80bbcd81a6647ff13108738cfb482d481d826cc0e02f5b35e5c88d2c
"""


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    requirements = root / "requirements.txt"
    if sys.platform != "darwin" or platform.machine() != "x86_64":
        print(requirements)
        return
    text, n = re.subn(
        r"^onnxruntime==.*\n(?:\s+--hash=.*\n)*",
        INTEL,
        requirements.read_text(encoding="utf-8"),
        flags=re.MULTILINE,
    )
    if n != 1:
        sys.exit("requirements.txt no trae onnxruntime como se esperaba: mira requisitos-mac.py")
    mac = root / "requisitos-mac.txt"
    mac.write_text(text, encoding="utf-8")
    print("onnxruntime 1.23.2, el ultimo que hay para Mac Intel", file=sys.stderr)
    print(mac)


if __name__ == "__main__":
    main()
