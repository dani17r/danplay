"""Limpieza y normalizacion de nombres.

Reglas acordadas con el usuario:
  - sin acentos, pero la ñ se conserva
  - las palabras en MAYUSCULA pasan a Capitalizado
  - formato final: "Artista - Titulo (feat. X).mp3"
  - fuera el ruido de descargas (VIDEO OFICIAL, LETRA, y2mate.com, ...)
  - duplicados: sufijo " - r", " - r2", ...

Lo hace el crate de Rust (library/src/names.rs, regla a regla lo que
antes estaba aqui); esto es la puerta de Python, con los mismos nombres.
"""

import re

import danplay_core as _rust


def _txt(s) -> str:
    """Un texto que se pueda pasar a Rust. Un nombre de archivo que no es
    UTF-8 llega con «surrogates» (`surrogateescape`) y Rust no los admite:
    se cambian por «�» antes de que un nombre raro pare un escaneo."""
    s = str(s)
    try:
        s.encode("utf-8")
    except UnicodeEncodeError:
        return s.encode("utf-8", "surrogateescape").decode("utf-8", "replace")
    return s


# las reglas del ruido de las descargas, tambien para quien las combina con
# las suyas (chat/resolve.py)
NOISE = list(_rust.names_noise())
# Lo que `strip_controls` quita (los mismos que en names.rs): los de control
# ASCII y C1 y los invisibles de direccion o anchura cero. Para quien
# compruebe un nombre (las pruebas).
CONTROL_CHARS = re.compile("[\x00-\x08\x0e-\x1f\x7f-\x9f\u200b-\u200f\u202a-\u202e\u2066-\u2069]")
# Nombres que Windows no deja usar como archivo ni carpeta (`sanitize` les
# pone «_» detras)
WINDOWS_RESERVED = {
    "CON",
    "PRN",
    "AUX",
    "NUL",
    *(f"COM{i}" for i in range(1, 10)),
    *(f"LPT{i}" for i in range(1, 10)),
}


def strip_controls(s: str) -> str:
    """El texto sin caracteres de control (los saltos, como espacios)."""
    return _rust.names_strip_controls(_txt(s))


def strip_accents(s: str) -> str:
    """Quita tildes y diacriticos pero respeta la ñ."""
    return _rust.names_strip_accents(_txt(s))


def search_form(text) -> str:
    """La forma en que se compara un nombre al buscar: minusculas, sin tildes
    PERO CON LA ñ, y sin nada que no sea letra, digito o espacio
    («*3.-De Gloria en Gloria* ✅🙏🏻» → «3 de gloria en gloria»). Se aplica a
    LOS DOS LADOS de toda comparacion."""
    return _rust.names_search_form(_txt(text or ""))


def search_tokens(text) -> list[str]:
    """Las palabras de `search_form(text)`."""
    return search_form(text).split()


def fold_enye(form: str) -> str:
    """La ñ como n. Solo para la segunda pasada, tolerante, de una busqueda
    («senor» tiene que encontrar «señor»): la forma normal CONSERVA la ñ, y
    «mañana» no es «manana»."""
    return form.replace("ñ", "n")


def capitalize_words(s: str) -> str:
    """Palabras enteras en MAYUSCULA -> Capitalizadas. Respeta CamelCase."""
    return _rust.names_capitalize_words(_txt(s))


def split_glued(s: str) -> str:
    """VERSIONButterflyFull -> VERSION Butterfly Full."""
    return _rust.names_split_glued(_txt(s))


def strip_noise(s: str) -> str:
    return _rust.names_strip_noise(_txt(s))


def clean(s: str) -> str:
    """Pipeline completo sobre un texto sin extension."""
    return _rust.names_clean(_txt(s))


def sanitize(s: str) -> str:
    """Nombre valido de archivo o carpeta en cualquier sistema: sin caracteres
    prohibidos ni de control, sin puntos y espacios finales, y con «_» en los
    nombres que Windows reserva («Con», «Aux»...)."""
    return _rust.names_sanitize(_txt(s))


def extract_feat(text: str) -> tuple[str, str]:
    """Saca el 'feat. X' del texto. Devuelve (texto_sin_feat, invitados)."""
    return _rust.names_extract_feat(_txt(text))


def final_name(artist: str, title: str, feat: str = "", extra: str = "", ext: str = ".mp3") -> str:
    """Construye 'Artista - Titulo (feat. X) (extra).ext'."""
    return _rust.names_final_name(_txt(artist), _txt(title), _txt(feat), _txt(extra), _txt(ext))


def match_key(s: str) -> str:
    """Clave normalizada para comparar canciones (deteccion de duplicados)."""
    return _rust.names_match_key(_txt(s))


def free_name(folder: str, name: str) -> str:
    """Si ya existe, aplica el sufijo ' - r', ' - r2', ... (regla de duplicados)."""
    return _rust.names_free_name(str(folder), _txt(name))


# ---------------------------------------------------------------- vocabulario


def _flat(s: str) -> str:
    return _rust.names_flat(_txt(s))


def vocabulary(artists_dir) -> dict:
    """{clave_plana: NombreRealDeCarpeta} a partir de las carpetas existentes."""
    return dict(_rust.names_vocabulary(str(artists_dir)))


def detect_artist(name: str, vocab: dict) -> dict:
    """Intenta resolver artista/titulo usando los artistas ya conocidos.

    Devuelve {'artist','title','feat','confidence'}. La confianza dice si el
    resultado es fiable (>=0.8) o si conviene preguntarle a la IA.
    """
    return _rust.names_detect_artist(_txt(name), list(vocab.items()))


def split_artist_title(file_name: str) -> dict | None:
    """«Artista - Titulo.mp3» → {artist, title, feat}; None si no hay guion.
    Con varios guiones, lo de despues del segundo va entre parentesis:
    «Palisades - Personal - Live» → «Personal (Live)»."""
    return _rust.names_split_artist_title(_txt(file_name))


# ------------------------------------------------- nombre desde YouTube


def channel_as_artist(channel: str) -> str:
    """El canal como artista: «BarakVEVO» → «Barak», «Ish Melton - Topic» → «Ish Melton»."""
    return _rust.names_channel_as_artist(_txt(channel or ""))


def _tail_to_parens(title: str) -> str:
    """«Que Se Abra El Cielo - Drum Cam» → «Que Se Abra El Cielo (Drum Cam)»."""
    return _rust.names_tail_to_parens(_txt(title))


def from_video(
    title: str, channel: str, vocab: dict | None = None, yt_artist: str = "", yt_track: str = ""
) -> dict:
    """Artista y titulo de una descarga, a partir de lo que dice YouTube: los
    datos de YouTube Music si vienen; un artista que ya tienes y aparece en
    el titulo; el canal, si aparece en el titulo; «Artista - Titulo»; y si no,
    el titulo entero con el canal como artista. Lo que se baja se llama como
    en YouTube, limpio, y no se le cuelga a otro artista."""
    return _rust.names_from_video(
        _txt(title or ""),
        _txt(channel or ""),
        list((vocab or {}).items()),
        _txt(yt_artist or ""),
        _txt(yt_track or ""),
    )


def video_kind(title: str) -> dict | None:
    """Si el titulo dice que no es la cancion sin mas: {"category", "what"}.

    `category` es la de `ingest.CATEGORY_FOLDER` (track, tutorial,
    sequence); `what`, como se nombra en el aviso («una Drum Cam»). Las
    descargas no se archivan por esto: la app solo lo sugiere.
    """
    return _rust.names_video_kind(_txt(title or ""))
