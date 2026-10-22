"""El emparejador con la biblioteca (`danplay.chat.resolve`): «¿esta cancion ya la tenemos?».

Todo sobre una biblioteca SINTETICA (artistas inventados y titulos de canciones
publicas) en una base temporal. Las filas se insertan a mano en `songs`, sin
generar audio: son miles de filas en un instante, y el indice FTS5 las sigue
por sus disparadores como siempre.

Lo que se prueba, en el orden del archivo:

  - los estados (found / ambiguous / probable / missing) y el orden de las
    candidatas, con la lista tal como la pega WhatsApp (numeracion, negritas,
    emojis, @menciones, notas entre parentesis, ruido de video);
  - la ñ: Mañana no es manana en la forma normal, y es lo mismo en la segunda
    pasada, con descuento (`why="enye~n"`);
  - apostrofos, erratas, partes de un titulo, orden cambiado;
  - los que NO hay: nada puede salir «found» y casi nada «probable»;
  - los enlaces y el historial de descargas (con ids reutilizados);
  - el lote: eleccion recordada, desempates, un item que revienta, el
    presupuesto de tiempo, una lista de 60;
  - entradas hostiles con limite de tiempo, en memoria y con el prefiltro FTS5;
  - el indice (una vez por revision, bajo cerrojo) y la escala (50 000 filas,
    marcada como lenta).
"""

import random
import threading
import time
import warnings
from types import SimpleNamespace

import pytest
from conftest import time_limit
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

from danplay import config, library, names
from danplay.chat import resolve

# Las pruebas lentas llevan la marca `slow` (`pytest -m "not slow"` las salta). La
# marca no esta registrada en pyproject.toml, y el aviso de «marca desconocida»
# saldria al importar este archivo: se apaga solo aqui.
with warnings.catch_warnings():
    warnings.simplefilter("ignore")
    slow = pytest.mark.slow

SONG_KEYS = {"id", "artist", "title", "album", "duration", "key", "bpm", "stars", "favorite"}
INSERT_SONG = (
    "INSERT INTO songs (id, path, root, folder, file, artist, title, album, feat, "
    "match_key, duration, stars, favorite) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)"
)


# ----------------------------------------------------------------- ayudas


class Lib:
    """Una biblioteca sintetica en la base temporal."""

    def __init__(self) -> None:
        self.next_id = 1

    def add(
        self,
        artist: str,
        title: str,
        *,
        file: str | None = None,
        folder: str | None = None,
        album: str = "",
        feat: str = "",
        stars: int = 0,
        favorite: int = 0,
        sid: int | None = None,
    ) -> int:
        if sid is None:
            sid = self.next_id
        self.next_id = max(self.next_id, sid + 1)
        if file is None:
            file = f"{artist} - {title}.mp3" if artist else f"{title}.mp3"
        if folder is None:
            folder = f"Artistas/{artist}" if artist else ""
        row = (sid, f"/musica/{folder}/{file}", "/musica", folder, file, artist, title, album)
        with library.connect() as conn:
            conn.execute(INSERT_SONG, (*row, feat, "", 200.0, stars, favorite))
        resolve.invalidate()
        return sid

    def path_of(self, sid: int) -> str:
        with library.connect() as conn:
            return conn.execute("SELECT path FROM songs WHERE id=?", (sid,)).fetchone()[0]

    def remove(self, sid: int) -> None:
        with library.connect() as conn:
            conn.execute("DELETE FROM songs WHERE id=?", (sid,))
        resolve.invalidate()


@pytest.fixture
def db(tmp_path, monkeypatch):
    """Una base vacia y temporal, y el emparejador sin nada recordado."""
    monkeypatch.setattr(config, "DATABASE", tmp_path / "danplay.db")
    resolve.invalidate()
    yield Lib()
    resolve.invalidate()


@pytest.fixture
def base(db):
    """La biblioteca de las pruebas (devuelve los ids por nombre corto): una
    cancion en tres versiones y un medley que la lleva, canciones con ñ y con
    apostrofo, copias « - r», una etiqueta sucia y otra cruzada, y titulos
    hechos solo de palabras vacias."""
    rows = {
        "dda_vivo": ("Coro Aurora", "Digno De Adorar (En Vivo)"),
        "dda": ("Banda Horizonte", "Digno De Adorar"),
        "dda_acu": ("Lucero D'Alba", "Digno De Adorar (Acustico)"),
        "dda_medley": ("Trio Faro", "Toma Tu Lugar + Dios + Digno De Adorar (Espontaneo)"),
        "sublime": ("Coro Aurora", "Sublime Gracia"),
        "castillo": ("Banda Horizonte", "Castillo Fuerte Es Nuestro Dios"),
        "cuan": ("Lucero D'Alba", "Cuan Grande Es Dios"),
        "santo3": ("Trio Faro", "Santo Santo Santo"),
        "cena": ("Coro Aurora", "Cena Del Señor"),
        "manana_n": ("Banda Horizonte", "Mañana"),
        "manana": ("Lucero D'Alba", "Manana"),
        "ano": ("Trio Faro", "Año Nuevo"),
        "rey": ("Lucero D'Alba", "Rey Infinito"),
        "gloria": ("Coro Aurora", "De Gloria En Gloria"),
        "big": ("Banda Horizonte", "Grande Y Fuerte Es Nuestro Dios"),
        "personal": ("Trio Faro", "Personal"),
        "alabanza": ("Altura Worship", "Alabanza"),
        "tu": ("Coro Aurora", "Tú"),
        "es_el": ("Trio Faro", "Es El"),
        "dont": ("Lucero D'Alba", "Don't Stop Believin'"),
        "hosanna": ("Banda Horizonte", "Hosanna En Las Alturas"),
        "sub": ("Banda Horizonte", "Fuego Santo (Cae El Muro)"),
    }
    ids = SimpleNamespace(**{key: db.add(*row) for key, row in rows.items()})
    # la etiqueta de titulo trae al artista y ruido de YouTube; el archivo esta limpio
    ids.dirty_tags = db.add(
        "Coro Aurora",
        "Coro Aurora - Fuego En El Altar (Video Oficial)",
        file="Coro Aurora - Fuego En El Altar.mp3",
    )
    # etiquetas cruzadas: el «artista» es el titulo y el «titulo», el artista
    ids.crossed = db.add("Rio De Vida", "Pedro Arango", file="Rio De Vida - Pedro Arango.mp3")
    # copias de la casa
    ids.babel = db.add("Coro Aurora", "Babel", file="Coro Aurora - Babel.mp3")
    ids.babel_r = db.add("Coro Aurora", "Babel - r", file="Coro Aurora - Babel - r.mp3")
    ids.babel_r2 = db.add("Coro Aurora", "Babel - r2", file="Coro Aurora - Babel - r2.mp3")
    return ids


def song_ids(res: resolve.Resolution) -> list[int]:
    return [c.song["id"] for c in res.candidates]


def chosen(res: resolve.Resolution) -> dict:
    """La cancion elegida (la prueba falla si no hay)."""
    assert res.song is not None, f"no se eligio ninguna ({res.status})"
    return res.song


def pick(res: resolve.Resolution) -> int:
    return chosen(res)["id"]


def within(seconds: float, fn, *args, **kwargs):
    """Llama a `fn` con un tope de tiempo: si se cuelga, la prueba falla (no se queda esperando)."""
    out: dict = {}

    def run() -> None:
        try:
            out["value"] = fn(*args, **kwargs)
        except BaseException as exc:  # noqa: BLE001  (se vuelve a lanzar abajo)
            out["error"] = exc

    t = threading.Thread(target=run, daemon=True)
    t.start()
    t.join(seconds)
    assert not t.is_alive(), f"se quedo mas de {seconds} s"
    if "error" in out:
        raise out["error"]
    return out["value"]


# ------------------------------------------------- estados y orden


def test_exact_title_is_found(base):
    r = resolve.match_song("Rey Infinito")
    assert r.status == "found" and r.via == "titulo" and not r.other_version
    assert r.song is not None and r.song["id"] == base.rey
    assert set(r.song) == SONG_KEYS, "la fila ligera de siempre (la de `_song_brief`)"
    assert r.candidates[0].song == r.song
    assert (r.candidates[0].why, r.candidates[0].score) == ("titulo exacto", 1.0)
    assert r.query["title"] == "Rey Infinito" and r.index == 0


def test_three_versions_is_ambiguous_with_one_chosen(base):
    """«Digno De Adorar» esta en estudio, en vivo y acustica (y dentro de un medley):
    se elige una y las demas van de alternativas, con el medley la ultima."""
    r = resolve.match_song("Digno De Adorar")
    assert r.status == "ambiguous" and r.via == "titulo"
    versiones = {base.dda, base.dda_vivo, base.dda_acu}
    assert r.song is not None and r.song["id"] in versiones
    assert song_ids(r)[0] == r.song["id"], "la elegida va primero"
    assert set(song_ids(r)[:3]) == versiones, "las otras dos versiones, de alternativas"
    assert song_ids(r)[3] == base.dda_medley and r.candidates[3].score < r.candidates[0].score
    assert "2 versiones más" in r.note
    # sin nada que decida, gana el titulo mas corto (la version de estudio)
    assert r.song["id"] == base.dda


def test_the_requested_version_is_no_longer_ambiguous(base):
    live = resolve.match_song("Digno De Adorar (En Vivo)")
    assert (live.status, pick(live)) == ("found", base.dda_vivo)
    acoustic = resolve.match_song("digno de adorar acustico")
    assert (acoustic.status, pick(acoustic)) == ("found", base.dda_acu)
    # la nota de la lista pide la version igual que el parentesis
    with_note = resolve.match_song("Digno De Adorar", hints=["(En Vivo)"])
    assert (with_note.status, pick(with_note)) == ("found", base.dda_vivo)
    # y el artista que la canta
    by_banda = resolve.match_song("Digno De Adorar", "Banda Horizonte")
    assert (by_banda.status, pick(by_banda)) == ("found", base.dda)
    by_lucero = resolve.match_song("Digno De Adorar", "Lucero D Alba")
    assert (by_lucero.status, pick(by_lucero)) == ("found", base.dda_acu)


@pytest.mark.parametrize(
    "text",
    [
        "*1.- Digno De Adorar* @Ana Lopez @Beto Ruiz (En Su Presencia) Voz",
        "1) Digno De Adorar @Ana Lopez Voz",
        "*Digno De Adorar*❤️🙏🏻",
        "• _Digno De Adorar_ 🎶 (tono G)",
        "  DIGNO DE ADORAR  [Letra] HD ",
    ],
)
def test_a_noisy_query_finds_the_same_song(base, text):
    clean_match = resolve.match_song("Digno De Adorar")
    dirty_match = resolve.match_song(text)
    assert (dirty_match.status, song_ids(dirty_match)) == (
        clean_match.status,
        song_ids(clean_match),
    )


@pytest.mark.parametrize(
    "text",
    [
        "Rey Infinito",
        "REY INFINITO",
        "rey infinito",
        "Rey Infinito!!",
        "«Rey Infinito»",
        '"Rey Infinito"',
        "‘Rey Infinito’",
        "*3.-Rey Infinito* ✅🙏🏻",
        "3.-Rey Infinito",
        "(3) Rey Infinito",
        "3º Rey Infinito",
        "03 - Rey Infinito",
        "~Rey Infinito~ 🥁",
        "Réy Infínito",
        "Rey Infinito (Video Oficial) HD",
        "Rey Infinito [Letra]",
        "Rey Infinito (con coros) (final)",
        "Rey Infinito @Ana Lopez @Beto Ruiz",
        "Rey Infinito @Ana Lopez Voz de apoyo (En Su Presencia)",
        "Rey Infinito https://youtu.be/AbCdEfGhIjK?feature=share",
        "Rey_Infinito",
        "Rey   Infinito\n",
    ],
)
def test_whatsapp_decorations_do_not_get_in_the_way(base, text):
    r = resolve.match_song(text)
    assert r.status in ("found", "ambiguous") and pick(r) == base.rey, text


def test_a_wrong_artist_or_a_mention_does_not_hide_the_song(base):
    without = resolve.match_song("Rey Infinito")
    for artist in ("Un Artista Cualquiera", "Trio Faro", "Ana Lopez", "Coro Aurora"):
        r = resolve.match_song("Rey Infinito", artist)
        assert (r.status, pick(r)) == (without.status, pick(without)), artist
    # la @mencion no es el artista: aunque coincida con uno de la biblioteca, no decide
    plain = resolve.match_song("Digno De Adorar")
    with_mention = resolve.match_song("Digno De Adorar @Lucero D'Alba")
    assert (with_mention.status, pick(with_mention)) == (plain.status, pick(plain))
    # lo mismo con la pista suelta de una nota
    with_note = resolve.match_song("Digno De Adorar", hints=["Lucero D'Alba", "Voz"])
    assert pick(with_note) == pick(plain)


def test_the_right_artist_only_adds(base):
    r = resolve.match_song("Rey Infinito", "Lucero D'Alba")
    assert r.status == "found" and pick(r) == base.rey
    for shape in (
        "Lucero DAlba",
        "Lucero D’Alba",
        "lucero d'alba",
        "LUCERO D ALBA",
        "Lucero Dalba",
    ):
        r = resolve.match_song("Digno De Adorar", shape)
        assert (r.status, pick(r)) == ("found", base.dda_acu), shape


def test_the_artist_attached_or_first(base):
    for text in (
        "Rey Infinito Lucero D'Alba",
        "Lucero D'Alba - Rey Infinito",
        "Lucero D'Alba Rey Infinito",
    ):
        r = resolve.match_song(text)
        assert r.status in ("found", "ambiguous") and pick(r) == base.rey, text
    r = resolve.match_song("Banda Horizonte - Digno De Adorar")
    assert (r.status, pick(r)) == ("found", base.dda)


def test_dirty_tags_are_searched_by_file_and_by_tag(base):
    for text in ("Fuego En El Altar", "Coro Aurora - Fuego En El Altar (Video Oficial)"):
        r = resolve.match_song(text)
        assert r.status == "found" and pick(r) == base.dirty_tags, text


def test_crossed_tags_are_found_with_the_artist(base):
    """Una cancion con el artista en el titulo y al reves: se encuentra si se pide con
    su artista; sin el, nada dice que «Rio De Vida» sea un titulo."""
    r = resolve.match_song("Rio De Vida", "Pedro Arango")
    assert r.status == "found" and pick(r) == base.crossed
    assert pick(resolve.match_song("Pedro Arango", "Rio De Vida")) == base.crossed
    assert resolve.match_song("Rio De Vida").status == "missing"


def test_in_house_copies_are_the_same_song(base):
    """«Babel», «Babel - r» y «Babel - r2» son tres archivos de la misma version:
    no hay nada que elegir."""
    for text in ("Babel", "Babel - r2", "babel - r"):
        r = resolve.match_song(text)
        assert r.status == "found", text
        assert set(song_ids(r)) <= {base.babel, base.babel_r, base.babel_r2}
        assert pick(r) == base.babel


def test_titles_made_of_stop_words(base):
    assert pick(resolve.match_song("Tú")) == base.tu
    assert resolve.match_song("tu").status == "found"
    assert pick(resolve.match_song("Es El")) == base.es_el
    # un orden distinto de solo palabras vacias no es una pista fuerte
    assert resolve.match_song("El Es").status in ("probable", "missing")
    # y una sola letra suelta no es nada
    assert resolve.match_song("a").status == "missing"
    assert resolve.match_song("x").status == "missing"


# --------------------------------------------------------------- la ñ


def test_enye_is_not_n_in_the_normal_form(base):
    """Mañana y Manana son dos canciones: cada una se encuentra por su forma, y la
    otra solo aparece como alternativa de la segunda pasada, con descuento."""
    with_enye = resolve.match_song("Mañana")
    assert pick(with_enye) == base.manana_n and with_enye.candidates[0].why == "titulo exacto"
    other = next(c for c in with_enye.candidates if c.song["id"] == base.manana)
    assert other.why == "enye~n" and other.score < with_enye.candidates[0].score

    without_enye = resolve.match_song("Manana")
    assert pick(without_enye) == base.manana and without_enye.candidates[0].why == "titulo exacto"
    other = next(c for c in without_enye.candidates if c.song["id"] == base.manana_n)
    assert other.why == "enye~n" and other.score < without_enye.candidates[0].score

    for variant in ("MAÑANA", "mañana", "*2.-Mañana* ✅"):
        assert pick(resolve.match_song(variant)) == base.manana_n, variant


def test_the_second_pass_forgives_enye_at_a_discount(base):
    """Quien escribe «Senor» sin ñ encuentra «Señor», marcado como de menor confianza."""
    r = resolve.match_song("Cena Del Senor")
    assert r.status == "found" and pick(r) == base.cena
    assert r.candidates[0].why == "enye~n" and 0.9 <= r.candidates[0].score < 1.0
    assert "ñ" in r.note
    exact = resolve.match_song("Cena Del Señor")
    assert exact.candidates[0].why == "titulo exacto" and exact.candidates[0].score == 1.0
    assert exact.note == ""
    # tambien a la inversa: la ñ en lo pedido y la n en la biblioteca
    new = resolve.match_song("Año Nuevo")
    assert new.candidates[0].why == "titulo exacto"
    without = resolve.match_song("Ano Nuevo")
    assert pick(without) == base.ano and without.candidates[0].why == "enye~n"
    # la ñ cuesta: con otra imperfeccion encima ya no es found
    assert resolve.match_song("Ano Nuevoo").status == "probable"


def test_enye_and_n_do_not_count_as_a_typo():
    """La errata de una letra no puede colar la n por la ñ en la forma normal."""
    assert not resolve._near("mañana", "manana")
    assert not resolve._near("senor", "señor")
    assert resolve._near("hosanna", "hosnna") and resolve._near("hosanna", "hosanan")
    assert resolve._near("hosanna", "hosannaa") and resolve._near("grita", "grit")
    assert not resolve._near("rey", "rei"), "las palabras cortas no se tocan"
    assert not resolve._near("hosanna", "hosanna")


# ------------------------------------------------------ apostrofos


def test_apostrophes_in_titles_and_artists(base):
    for text in (
        "Don't Stop Believin'",
        "Dont Stop Believin",
        "Don’t Stop Believin’",
        "DONT STOP BELIEVIN",
    ):
        r = resolve.match_song(text)
        assert r.status == "found" and pick(r) == base.dont, text
    assert resolve._parse("Lucero D'Alba").core == resolve._parse("Lucero DAlba").core
    assert resolve.title_key("Rey Infinito", "Lucero D'Alba") == resolve.title_key(
        "Rey Infinito", "Lucero DAlba"
    )


def test_the_artist_apostrophe_with_a_gap_too(db):
    sid = db.add("Christine D' Clario", "Fe", file="Christine D'Clario - Fe.mp3")
    other = db.add("Otro Artista", "Fe")
    for shape in (
        "Christine D'Clario",
        "Christine DClario",
        "Christine D Clario",
        "Christine D’Clario",
    ):
        r = resolve.match_song("Fe", shape)
        assert pick(r) == sid and r.status == "found", shape
    assert pick(resolve.match_song("Fe", "Otro Artista")) == other


# ------------------------------------------------ erratas, partes, orden


@pytest.mark.parametrize(
    "text",
    [
        "Hosznna En Las Alturas",  # una letra cambiada
        "Hosnna En Las Alturas",  # una de menos
        "Hosannna En Las Alturas",  # una de mas
        "Hosanan En Las Alturas",  # dos seguidas cambiadas de sitio
        "Hosanna En Las Altruas",
        "hosanna en las alturaz",
    ],
)
def test_a_typo_is_probable_and_finds_the_song(base, text):
    r = resolve.match_song(text)
    assert r.status == "probable" and r.via == "difuso" and pick(r) == base.hosanna, text
    assert r.candidates[0].why == "difuso"
    assert resolve.T_MISSING <= r.candidates[0].score < resolve.T_FOUND
    assert r.note


def test_a_typo_with_the_right_artist_is_found(base):
    r = resolve.match_song("Hosznna En Las Alturas", "Banda Horizonte")
    assert r.status == "found" and pick(r) == base.hosanna


def test_a_typo_in_a_short_word_only_shows_when_joined(base):
    """La errata de una letra se tolera en palabras de 4 o mas (una de 5 a la que
    le falta una queda en 4). En una corta solo se ve al mirar el titulo entero."""
    assert resolve.match_song("Rei Infinito").status == "probable", "entero: «reiinfinito»"
    assert resolve.match_song("Gracia Sublim").status == "probable"
    assert resolve.match_song("Sublimme").status == "missing", "una sola palabra suelta no basta"


def test_the_start_of_a_title_is_probable(base):
    r = resolve.match_song("Grande Y")
    assert r.status == "probable" and pick(r) == base.big
    assert r.candidates[0].why == "frase en titulo" and r.note
    assert pick(resolve.match_song("Castillo Fuerte")) == base.castillo
    # pero una sola palabra, o el final, o el medio, no bastan
    assert resolve.match_song("Grande").status == "missing"
    assert resolve.match_song("Sublime").status == "missing"
    assert resolve.match_song("Nuestro Dios").status == "missing"


def test_the_same_words_in_another_order_is_probable(base):
    for text in ("Gracia Sublime", "Alturas Las En Hosanna", "Infinito Rey"):
        r = resolve.match_song(text)
        assert r.status == "probable" and r.candidates[0].why == "palabras", text
    assert pick(resolve.match_song("Gracia Sublime")) == base.sublime


def test_words_joined_or_split_differently(base):
    """«ReyInfinito» o «CenaDelSeñor» son el mismo titulo con los espacios en otro sitio."""
    for text in ("ReyInfinito", "Rey Infini to", "CenaDelSeñor"):
        r = resolve.match_song(text)
        assert r.status == "found" and r.song is not None, text
    assert pick(resolve.match_song("ReyInfinito")) == base.rey
    assert pick(resolve.match_song("CenaDelSeñor")) == base.cena


def test_a_subtitle_and_part_of_a_medley(base):
    sub = resolve.match_song("Cae El Muro")
    assert sub.status == "probable" and pick(sub) == base.sub
    full = resolve.match_song("Fuego Santo Cae El Muro")
    assert full.status == "found" and pick(full) == base.sub
    medley = resolve.match_song("Toma Tu Lugar")
    assert pick(medley) == base.dda_medley
    assert resolve.T_MISSING <= medley.candidates[0].score < 1.0, (
        "una parte vale menos que un titulo"
    )


# --------------------------------------------------- lo que NO esta


@pytest.mark.parametrize(
    "text",
    [
        # canciones publicas que no estan
        "Oceans Where Feet May Fail",
        "Way Maker",
        "Rey de Reyes",
        "*5.-Océanos (Donde Mis Pies Pueden Fallar)* ✅🙏🏻",
        # casi: una palabra cambiada
        "Cuán Grande Es Él",
        "*2.- Cuán Grande Es Él* 🙏🏻",
        "Santo Es El Señor",
        "Digno Es El Cordero",
        "Castillo Debil Es Nuestro Dios",
        # casi: el titulo de otra con algo de mas
        "Personal Jesus",
        "Sublime Gracia Maravillosa Eterna",
        # una palabra suelta del artista, del album, de un parentesis
        "Altura",
        "Altura Worship",
        "Horizonte",
        "Aurora",
        "Heaven",
        # nada que ver
        "Despacito",
        "Bohemian Rhapsody",
        "Gloria Aleluya Santo",
    ],
)
def test_what_is_missing_is_not_matched(base, text):
    r = resolve.match_song(text)
    assert r.status == "missing" and r.song is None and r.via == "", f"{text!r} -> {r.status}"
    assert resolve.rank(text) == []


def test_what_is_missing_with_its_artist_is_missing_too(base):
    for title, artist in (
        ("Good Good Father", "Chris Tomlin"),
        ("Rey de Reyes", "Hillsong en Español"),
        ("Alabanza Eterna", "Altura Worship"),
        ("Cuán Grande Es Él", "Lucero D'Alba"),
    ):
        assert resolve.match_song(title, artist).status == "missing", (title, artist)


def test_an_extra_word_is_never_found(base):
    """Quien pide «Rey Infinito Poderoso» no pide «Rey Infinito»: como mucho, probable."""
    for text in ("Rey Infinito Poderoso", "Hosanna En Las Alturas Eterno", "Cena Del Señor Santa"):
        r = resolve.match_song(text)
        assert r.status in ("probable", "missing"), (text, r.status)


def test_a_changed_word_is_never_found(base):
    """Ningun titulo que se parece a uno de la biblioteca (una palabra cambiada) se cuela
    como `found` ni `ambiguous`."""
    for a in ("Rey", "Gloria", "Sublime", "Cena", "Santo", "Hosanna"):
        for b in ("Eterno", "Poderoso", "Nuevo", "Perfecto"):
            text = f"{a} {b}"
            assert resolve.match_song(text).status not in ("found", "ambiguous"), text


# ------------------------------------------------- enlaces e historial

VIDEO = "AbCdEfGhIjK"
URL = f"https://www.youtube.com/watch?v={VIDEO}"
OTHER_URL = "https://youtu.be/Q1w2E3r4T5y"


def record_download(
    db, url, song_id, *, target=None, ok=True, already=False, at=None, title=""
) -> None:
    """Apunta una descarga en el historial, como `youtube._log`."""
    library.log_download(
        {
            "at": at,
            "source": "assistant",
            "query": url,
            "title": title,
            "url": url,
            "ok": ok,
            "already": already,
            "song_id": song_id,
            "target": db.path_of(song_id) if target is None else target,
        }
    )


def test_an_already_downloaded_link_beats_the_title(base, db):
    record_download(db, URL, base.dda_acu)
    r = resolve.match_song("cualquier cosa que no se parece", url=URL)
    assert (r.status, r.via, r.other_version) == ("found", "enlace", False)
    assert pick(r) == base.dda_acu
    assert [(c.why, c.score) for c in r.candidates] == [("enlace", 1.0)]
    # aunque el titulo apunte a otra version del mismo nombre
    other = resolve.match_song("Digno De Adorar", "Banda Horizonte", url=URL)
    assert (other.via, pick(other)) == ("enlace", base.dda_acu)
    # y sin titulo
    without = resolve.match_song("", url=URL)
    assert (without.via, pick(without)) == ("enlace", base.dda_acu)


@pytest.mark.parametrize(
    "url",
    [
        f"https://www.youtube.com/watch?v={VIDEO}&list=PLxxxxxxxx&t=30s&feature=share",
        f"https://youtu.be/{VIDEO}?feature=share",
        f"youtu.be/{VIDEO}",
        f"https://music.youtube.com/watch?v={VIDEO}",
        f"https://m.youtube.com/watch?v={VIDEO}",
        f"https://www.youtube.com/shorts/{VIDEO}",
    ],
)
def test_the_link_is_the_same_video_however_it_is_written(base, db, url):
    record_download(db, URL, base.rey)
    assert resolve._video_id(url) == VIDEO
    r = resolve.match_song("", url=url)
    assert (r.via, pick(r)) == ("enlace", base.rey)


@pytest.mark.parametrize(
    "url",
    [
        f"https://youtube.com.otro.example/watch?v={VIDEO}",
        f"https://youtube.com@otro.example/watch?v={VIDEO}",
        f"https://otro.example/?u=https://youtube.com/watch?v={VIDEO}",
        f"http://127.0.0.1/watch?v={VIDEO}",
        f"javascript:alert('youtube.com/watch?v={VIDEO}')",
        f"file:///youtube.com/watch?v={VIDEO}",
        f"https://www.youtube.com/playlist?list={VIDEO}",
        "https://www.youtube.com/watch?v=corto",
        "https://www.youtube.com/",
        "",
    ],
)
def test_what_is_not_a_youtube_video_is_not_a_link(base, db, url):
    record_download(db, URL, base.rey)
    assert resolve._video_id(url) == ""
    r = resolve.match_song("Cena Del Señor", url=url)
    assert r.via != "enlace" and pick(r) == base.cena and not r.other_version


def test_a_reused_id_does_not_count(base, db):
    """Los ids de cancion se reutilizan al borrar la ultima fila: la fila del
    historial solo vale si el archivo de esa cancion sigue siendo el que se bajo."""
    last = db.add("Banda Horizonte", "Cancion Temporal")
    record_download(db, URL, last)
    db.remove(last)  # sale de la biblioteca y el id queda libre…
    reused = db.add("Otro Artista", "Otro Titulo Distinto", sid=last)  # …y otra cancion lo toma
    assert reused == last
    r = resolve.match_song("Otro Titulo Distinto", url=URL)
    assert r.via != "enlace", "el historial apuntaba a otra cancion"
    assert pick(r) == reused and r.other_version, (
        "se encuentra por el titulo; el enlace queda por bajar"
    )
    # y sin titulo el enlace no lleva a nada
    without = resolve.match_song("", url=URL)
    assert without.status == "missing" and without.via == ""


def test_history_is_walked_from_newest_to_oldest(base, db):
    """El caso real: un enlace con tres filas; la mas nueva apunta a una cancion que
    ya no es esa (id reutilizado) y la anterior sigue valiendo."""
    now = time.time()
    record_download(db, URL, base.dda_vivo, at=now - 300)  # vale
    record_download(db, URL, base.dda_vivo, at=now - 200)  # vale (repetida)
    record_download(
        db, URL, base.rey, target="/musica/Artistas/Otro/Otro Archivo.mp3", at=now - 100
    )  # no vale
    r = resolve.match_song("", url=URL)
    assert (r.via, pick(r)) == ("enlace", base.dda_vivo)
    # si la mas nueva SI vale, gana ella
    record_download(db, URL, base.sublime, at=now)
    assert pick(resolve.match_song("", url=URL)) == base.sublime
    # solo valen las que salieron bien (o las que ya estaban)
    record_download(db, "https://youtu.be/ZyXwVuTsRqP", base.rey, ok=False)
    assert resolve.match_song("", url="https://youtu.be/ZyXwVuTsRqP").via != "enlace"


def test_the_same_file_name_counts_even_if_the_folder_moved(base, db):
    """Si se movio la biblioteca de sitio, el archivo sigue llamandose igual."""
    name = db.path_of(base.cena).rsplit("/", 1)[-1]
    record_download(
        db, URL, base.cena, target=f"C:\\Users\\alguien\\Musica\\Artistas\\Coro Aurora\\{name}"
    )
    assert resolve.match_song("", url=URL).via == "enlace"


def test_already_there_is_checked_by_the_name_key(base, db):
    """Una descarga «ya la tenias» no deja archivo nuevo: se comprueba el nombre."""
    with library.connect() as conn:
        conn.execute(
            "UPDATE songs SET match_key=? WHERE id=?",
            (names.match_key(names.clean("Cena Del Señor")), base.cena),
        )
    resolve.invalidate()
    record_download(
        db,
        URL,
        base.cena,
        target="",
        ok=False,
        already=True,
        title="CENA DEL SEÑOR (Video Oficial)",
    )
    r = resolve.match_song("", url=URL)
    assert (r.via, pick(r)) == ("enlace", base.cena)
    # con otro nombre de video, esa fila no demuestra nada
    record_download(
        db,
        "https://youtu.be/QwErTyUiOpA",
        base.cena,
        target="",
        ok=False,
        already=True,
        title="Algo Que No Es",
    )
    assert resolve.match_song("", url="https://youtu.be/QwErTyUiOpA").via != "enlace"


def test_other_version_and_strict(base, db):
    """El enlace no esta en el historial pero la biblioteca tiene el titulo: se usa
    la de la biblioteca (`other_version`) o, con `strict`, no se sustituye."""
    r = resolve.match_song("Cena Del Señor", url=OTHER_URL)
    assert (r.status, pick(r), r.other_version, r.via) == ("found", base.cena, True, "titulo")
    assert "historial" in r.note
    strict_match = resolve.match_song("Cena Del Señor", url=OTHER_URL, strict=True)
    assert (strict_match.status, strict_match.song, strict_match.other_version) == (
        "missing",
        None,
        True,
    )
    assert strict_match.candidates[0].song["id"] == base.cena, "la otra version sigue a la vista"
    # el enlace de verdad ya bajado no es «otra version» ni siquiera en modo estricto
    record_download(db, OTHER_URL, base.cena)
    ok = resolve.match_song("Cena Del Señor", url=OTHER_URL, strict=True)
    assert (ok.status, ok.via, ok.other_version) == ("found", "enlace", False)
    # sin enlace no hay otra version
    assert not resolve.match_song("Cena Del Señor", strict=True).other_version
    # lo que no esta sigue sin estar, con o sin enlace
    assert not resolve.match_song("Way Maker", url=OTHER_URL).other_version


def test_the_video_title(base):
    r = resolve.match_song(
        "", url=OTHER_URL, video_title="Banda Horizonte - Cena Del Señor (Video Oficial) HD"
    )
    assert r.status == "found" and pick(r) == base.cena and r.other_version
    # el canal o el artista del video pueden ser otros: solo suman
    r = resolve.match_song(
        "", url=OTHER_URL, video_title="Alguien Que No Es - Rey Infinito (Letra)"
    )
    assert pick(r) == base.rey
    # si el titulo escrito no encuentra nada, se prueba con el del video
    r = resolve.match_song("Lo Que Dijo La Lista", url=OTHER_URL, video_title="Rey Infinito")
    assert pick(r) == base.rey
    # sin titulo ni video no hay nada que mirar
    empty = resolve.match_song("", url=OTHER_URL)
    assert empty.status == "missing" and empty.note
    # y con strict no se sustituye
    assert (
        resolve.match_song("", url=OTHER_URL, video_title="Rey Infinito", strict=True).status
        == "missing"
    )


def test_a_link_pasted_in_the_title_is_not_the_title(base, db):
    record_download(db, URL, base.sublime)
    r = resolve.match_song(f"*3.-Sublime Gracia* {URL} @Ana Lopez")
    assert (r.via, pick(r)) == ("enlace", base.sublime)
    new = resolve.match_song("*3.-Rey Infinito* https://youtu.be/Q1w2E3r4T5y?feature=share")
    assert pick(new) == base.rey and new.other_version


# ------------------------------------------------------------ el lote


def test_resolve_items_in_order_with_each_status(base):
    items = [
        "*1.- Rey Infinito*",
        {"title": "Digno De Adorar", "artist": "Un Equivocado", "n": 7},
        {"title": "Hosznna En Las Alturas", "note": "(En Su Presencia) Voz"},
        {"title": "Despacito"},
        {"title": "Cena Del Senor", "url": OTHER_URL},
    ]
    out = resolve.resolve_items(items)
    assert [r.index for r in out] == [0, 1, 2, 3, 4]
    assert [r.status for r in out] == ["found", "ambiguous", "probable", "missing", "found"]
    assert [r.query["n"] for r in out] == [1, 7, 3, 4, 5], "el n pedido, o la posicion"
    assert out[1].query["artist"] == "Un Equivocado" and out[2].query["note"].startswith("(En Su")
    assert out[4].other_version and not out[0].other_version
    assert not any(r.failed for r in out)


def test_overrides_and_explicit_id(base):
    out = resolve.resolve_items(
        [
            {"title": "Digno De Adorar"},
            {"title": "Digno De Adorar"},
            {"title": "Digno De Adorar", "n": 9},
            {"title": "Rey Infinito", "id": base.dda_acu},
        ],
        overrides=[{"n": 2, "id": base.dda_vivo}, {"n": 9, "id": str(base.dda_acu)}],
    )
    assert out[0].status == "ambiguous"
    assert (out[1].status, pick(out[1])) == ("found", base.dda_vivo)
    assert out[1].candidates[0].why == "elegida" and out[1].candidates[0].score == 1.0
    assert (out[2].status, pick(out[2])) == ("found", base.dda_acu), "el n pedido, no la posicion"
    assert (out[3].status, pick(out[3])) == ("found", base.dda_acu), "el id manda sobre el titulo"
    # un id que no existe cae al titulo y lo dice
    odd = resolve.resolve_items([{"title": "Rey Infinito", "id": 99999}])[0]
    assert pick(odd) == base.rey and "id" in odd.note
    bad = resolve.resolve_items(
        [{"title": "Rey Infinito"}],
        overrides=[{"n": 1, "id": 99999}, {"nada": 1}, {"n": "x", "id": 1}],
    )
    assert pick(bad[0]) == base.rey


def test_the_remembered_choice_wins_ties(base):
    """«Digno De Adorar» tiene tres versiones: la que se eligio la semana pasada
    (mismo `title_key`) gana a igualdad, y deja de dar la duda de siempre."""
    remembered = {resolve.title_key("*1.- Digno De Adorar* @Ana Lopez"): base.dda_acu}
    key = resolve.title_key("digno de adorar (En Su Presencia)")
    assert key == resolve.title_key("DIGNO DE ADORAR") == "digno de adorar"
    r = resolve.match_song("Digno De Adorar", prefer_ids=[remembered[key]])
    assert r.status == "ambiguous", "sigue habiendo tres; solo cambia la elegida"
    assert pick(r) == base.dda_acu and song_ids(r)[0] == base.dda_acu
    batch = resolve.resolve_items(["Digno De Adorar"], prefer_ids=[base.dda_vivo])
    assert pick(batch[0]) == base.dda_vivo
    # una eleccion que no esta entre las empatadas no cambia nada
    assert pick(resolve.match_song("Digno De Adorar", prefer_ids=[base.rey])) == base.dda
    assert pick(resolve.match_song("Digno De Adorar", prefer_ids=["no", None, 99999])) == base.dda
    # y lo que pide la persona vale mas que lo recordado
    live = resolve.match_song("Digno De Adorar (En Vivo)", prefer_ids=[base.dda_acu])
    assert pick(live) == base.dda_vivo


def test_title_key_is_stable_and_tells_apart():
    same_titles = [
        "Digno De Adorar",
        "*1.- Digno De Adorar* ✅🙏🏻",
        "digno de adorar @Ana Lopez @Beto Ruiz (En Su Presencia) Voz",
        "DIGNO  DE ADORAR  [HD]",
        "Digno De Adorar https://youtu.be/AbCdEfGhIjK",
    ]
    assert len({resolve.title_key(t) for t in same_titles}) == 1
    assert resolve.title_key("Digno De Adorar") != resolve.title_key("Digno De Adorar Ya")
    assert resolve.title_key("Mañana") != resolve.title_key("Manana"), (
        "la ñ se conserva, como en toda comparacion"
    )
    with_artist = resolve.title_key("Digno De Adorar", "Banda Horizonte")
    assert (
        with_artist
        == resolve.title_key("*1.-Digno De Adorar*", "banda  horizonte")
        != resolve.title_key("Digno De Adorar")
    )
    assert resolve.title_key("") == "" and resolve.title_key(None) == ""


def test_tiebreaks_choice_favorite_stars_and_short_title(db):
    length = db.add("Artista Uno", "Canto Nuevo Largo (En Vivo)")
    short = db.add("Artista Dos", "Canto Nuevo", stars=2)
    four = db.add("Artista Tres", "Canto Nuevo (Acustico)", stars=4)
    favorite_song = db.add("Artista Cuatro", "Canto Nuevo (Pista)", favorite=1)
    assert pick(resolve.match_song("Canto Nuevo")) == favorite_song, (
        "la favorita gana a las estrellas"
    )
    with library.connect() as conn:
        conn.execute("UPDATE songs SET favorite=0")
    resolve.invalidate()
    assert pick(resolve.match_song("Canto Nuevo")) == four, "mas estrellas"
    with library.connect() as conn:
        conn.execute("UPDATE songs SET stars=0")
    resolve.invalidate()
    assert pick(resolve.match_song("Canto Nuevo")) == short, "el titulo mas corto"
    assert pick(resolve.match_song("Canto Nuevo", prefer_ids=[four])) == four, (
        "la recordada, la primera"
    )
    # el que solo empieza igual no es un empate: no se elige por mas estrellas ni por recordado
    assert pick(resolve.match_song("Canto Nuevo", prefer_ids=[length])) == short
    assert {length, short, four, favorite_song} == set(
        song_ids(resolve.match_song("Canto Nuevo", limit=10))
    )


def test_an_item_that_crashes_does_not_sink_the_batch(base, monkeypatch):
    # la puntuacion esta en Rust: revienta el emparejado de ese item
    original = resolve._match

    def blow_up(snap, title, *args, **kwargs):
        if "explota" in str(title).lower():
            raise ValueError("un titulo raro")
        return original(snap, title, *args, **kwargs)

    monkeypatch.setattr(resolve, "_match", blow_up)
    out = resolve.resolve_items(["Rey Infinito", "Rey Explota", {"title": "Cena Del Señor"}])
    assert [r.status for r in out] == ["found", "missing", "found"]
    assert [r.failed for r in out] == [False, True, False]
    assert out[1].note and out[1].query["title"] == "Rey Explota"


def test_the_time_budget_and_the_item_cap(base, monkeypatch):
    out = resolve.resolve_items(["Rey Infinito", "Cena Del Señor"], budget=-1)
    assert [r.failed for r in out] == [True, True] and all(r.status == "missing" for r in out)
    assert all("tiempo" in r.note for r in out)
    monkeypatch.setattr(resolve, "MAX_ITEMS", 2)
    out = resolve.resolve_items(["Rey Infinito", "Cena Del Señor", "Sublime Gracia"])
    assert [r.failed for r in out] == [False, False, True]


def test_a_failing_db_does_not_sink_the_batch(base, monkeypatch):
    def broken(*_a, **_k):
        raise RuntimeError("la base no se puede abrir")

    monkeypatch.setattr(resolve.INDEX, "snapshot", broken)
    out = resolve.resolve_items(["Rey Infinito", "Cena Del Señor"])
    assert [(r.status, r.failed) for r in out] == [("missing", True), ("missing", True)]


def test_a_setlist_of_60(db):
    first_words = [
        "Cielo",
        "Mar",
        "Luz",
        "Monte",
        "Rio",
        "Sol",
        "Viento",
        "Fuego",
        "Tierra",
        "Estrella",
    ]
    second_words = ["Nuevo", "Eterno", "Santo", "Abierto", "Grande", "Fiel"]
    titles = [f"{a} {b}" for a in first_words for b in second_words]
    ids = {t: db.add(f"Artista {i % 7}", t) for i, t in enumerate(titles)}
    decorations = [
        "*{n}.-{t}* ✅🙏🏻",
        "{n}) _{t}_ 🎶 @Ana Lopez (En Su Presencia) Voz",
        "• *{t}*❤️",
        "{t} (final) HD",
        "{n}. {t} @Beto Ruiz",
    ]
    items = [
        decorations[i % len(decorations)].format(n=i + 1, t=t) for i, t in enumerate(titles[:45])
    ]
    items += [
        f"{a} {b}"
        for a in ("Zorro", "Quimera", "Pandora", "Nebulosa", "Ocaso")
        for b in ("Lejano", "Perdido", "Frio")
    ]
    assert len(items) == 60
    t0 = time.perf_counter()
    out = within(30, resolve.resolve_items, items)
    assert time.perf_counter() - t0 < time_limit(5.0), "60 items con 60 canciones"
    assert len(out) == 60 and [r.index for r in out] == list(range(60))
    for i, t in enumerate(titles[:45]):
        assert out[i].status == "found" and out[i].song["id"] == ids[t], t
    assert all(r.status == "missing" and not r.failed for r in out[45:])


def test_rank_is_the_measure_search_adapter(base):
    assert resolve.rank("Rey Infinito") == [{"id": base.rey, "score": 1.0, "status": "found"}]
    amb = resolve.rank("Digno De Adorar")
    assert amb[0]["status"] == "ambiguous" and {r["status"] for r in amb[1:]} == {"alt"}
    assert amb[0]["id"] == base.dda and len(amb) >= 3
    assert all(0 < r["score"] <= 1 for r in amb)
    assert resolve.rank("Despacito") == [] and resolve.rank("") == []
    assert resolve.rank("Hosznna En Las Alturas")[0]["status"] == "probable"


def test_limit_and_the_copies_returned(base):
    assert len(resolve.match_song("Digno De Adorar", limit=2).candidates) == 2
    assert len(resolve.match_song("Digno De Adorar", limit=1).candidates) == 1
    r = resolve.match_song("Rey Infinito")
    chosen(r)["title"] = "MUTADO"
    r.candidates[0].song["artist"] = "MUTADO"
    other = resolve.match_song("Rey Infinito")
    assert chosen(other)["title"] == "Rey Infinito" and chosen(other)["artist"] == "Lucero D'Alba"


# ------------------------------------------------------ entradas hostiles

HOSTILES = {
    "200 KB de palabras": "gloria dios señor " * 12_000,
    "200 KB de asteriscos": "*" * 200_000,
    "200 KB de parentesis sin cerrar": "(" * 200_000,
    "200 KB de corchetes": "[" * 200_000 + "x" * 1000,
    "200 KB de (a)": "(a)" * 70_000,
    "parentesis anidados": "(" * 5000 + "Rey Infinito" + ")" * 5000,
    "50 000 arrobas": "@" * 50_000,
    "enlaces repetidos": "https://youtu.be/" * 12_000,
    "20 000 lineas de numeracion": "1.- Titulo\n" * 20_000,
    "solo caracteres raros": "★☆✿❀♪♫🙏🏻🎶" * 50 + "\u202e\u200b\x00\x07",
    "una letra": "x",
    "un punto": ".",
    "vacio": "",
    "solo espacios": " " * 100_000,
    "una palabra gigante": "a" * 200_000,
    "Tú": "Tú",
    "Él": "Él",
    "sintaxis FTS con comillas": '"gloria" OR NEAR(dios senor, 3) AND * ( ) :: title:gloria -x ^y "',
    "sintaxis FTS con asterisco": "gloria* OR dios*",
    "palabras reservadas de FTS": "OR AND NOT NEAR",
    "comillas sueltas": '"' * 2000,
    "apostrofes sueltos": "'" * 2000,
    "dos puntos de filtro": "artista:barak tono:Bb bpm>100 title:x",
    "NUL y controles": "gloria\x00dios\x07\x1b[31m",
    "texto bidi": "\u202egloria\u202c dios",
}


@pytest.fixture(params=["en memoria", "con prefiltro FTS5"])
def mode(request, base, monkeypatch):
    """Los mismos casos con el indice entero en memoria y con el prefiltro FTS5."""
    if request.param == "con prefiltro FTS5":
        monkeypatch.setattr(resolve, "FULL_LIMIT", 3)
        resolve.invalidate()
        assert resolve.INDEX.snapshot().large
    else:
        assert not resolve.INDEX.snapshot().large
    return request.param


@pytest.mark.parametrize("name", sorted(HOSTILES))
def test_hostile_inputs_within_a_time_limit(mode, name):
    text = HOSTILES[name]
    r = within(5, resolve.match_song, text, text[:40], hints=[text], url=text[:300])
    assert r.status in ("found", "ambiguous", "probable", "missing")
    assert (r.song is None) == (r.status == "missing")
    assert len(r.query["title"]) <= 300 and len(r.query["url"]) <= 300


def test_a_whole_hostile_batch_does_not_fail(mode):
    items = [{"title": t, "artist": t[:30], "url": t[:300], "note": t} for t in HOSTILES.values()]
    out = within(30, resolve.resolve_items, items)
    assert len(out) == len(items) and not any(r.failed for r in out)
    assert [r.index for r in out] == list(range(len(items)))


def test_fts_syntax_does_not_change_the_search(mode):
    """Comillas, parentesis, OR, NEAR y * dentro del titulo son texto: no se interpretan."""
    for text in (
        '"Rey Infinito"',
        "Rey* OR Cena*",
        "NEAR(rey infinito, 2)",
        "title:rey",
        "Rey AND (Infinito OR Cena)",
    ):
        r = within(5, resolve.match_song, text)
        assert r.status in ("found", "ambiguous", "probable", "missing")
    assert resolve.match_song('"Rey Infinito"').status == "found"
    assert resolve.match_song("Rey* OR Cena*").status != "found"


def test_a_title_of_only_stop_words_and_one_letter(mode):
    r = resolve.match_song("Tú")
    assert r.status == "found" and chosen(r)["title"] == "Tú"
    assert resolve.match_song("Él").status == "missing"
    assert resolve.match_song("y").status == "missing"
    assert resolve.match_song("x").status == "missing"


@settings(
    max_examples=120, deadline=None, suppress_health_check=[HealthCheck.function_scoped_fixture]
)
@given(st.text(max_size=300), st.text(max_size=40))
def test_any_text_gives_a_valid_answer(base, title, artist):
    r = resolve.match_song(title, artist, hints=[title[:20]], url=title[-30:])
    assert r.status in ("found", "ambiguous", "probable", "missing")
    assert (r.song is None) == (r.status == "missing")
    assert all(0.0 <= c.score <= 1.0 for c in r.candidates)
    if r.status != "missing":
        assert r.candidates and r.candidates[0].song == r.song


# ----------------------------------------- el prefiltro FTS5 y el indice


def test_the_fts_prefilter_matches_the_in_memory_index(base, monkeypatch):
    queries = [
        ("Rey Infinito", ""),
        ("*3.-Rey Infinito* ✅🙏🏻 @Ana Lopez Voz (En Su Presencia)", ""),
        ("Digno De Adorar", ""),
        ("Digno De Adorar", "Banda Horizonte"),
        ("Digno De Adorar (En Vivo)", ""),
        ("Cena Del Senor", ""),
        ("Mañana", ""),
        ("Manana", ""),
        ("Dont Stop Believin", ""),
        ("Grande Y", ""),
        ("Gracia Sublime", ""),
        ("Fuego En El Altar", ""),
        ("Babel - r2", ""),
        ("Tú", ""),
        ("Es El", ""),
        ("Cuán Grande Es Él", ""),
        ("Personal Jesus", ""),
        ("Despacito", ""),
        ("Altura Worship", ""),
        ("Hosznna En Las Alturas", ""),
    ]

    def summary():
        out = []
        for t, a in queries:
            r = resolve.match_song(t, a)
            out.append((r.status, r.song, song_ids(r)))
        return out

    in_memory = summary()
    monkeypatch.setattr(resolve, "FULL_LIMIT", 3)
    resolve.invalidate()
    assert resolve.INDEX.snapshot().large
    with_fts = summary()
    for (t, a), mem, fts in zip(queries, in_memory, with_fts, strict=True):
        assert mem == fts, (t, a)


def test_the_prefilter_finds_a_typo_with_the_tolerant_retry(base, monkeypatch):
    """Una palabra mal escrita no existe en el indice FTS5: la rescatan sus tres
    primeros caracteres (solo si nada convencio al primer intento)."""
    monkeypatch.setattr(resolve, "FULL_LIMIT", 3)
    resolve.invalidate()
    assert resolve.INDEX.snapshot().large
    r = resolve.match_song("Sublmie Gracai")
    assert r.status == "probable" and pick(r) == base.sublime
    r = resolve.match_song("Sublime Gracai")
    assert r.status == "probable" and pick(r) == base.sublime


def test_the_index_is_rebuilt_with_the_library_revision(base, db):
    assert resolve.match_song("Cancion Recien Llegada").status == "missing"
    db.add("Banda Horizonte", "Cancion Recien Llegada")  # invalida
    assert resolve.match_song("Cancion Recien Llegada").status == "found"
    # sin invalidar a mano: lo que avisa la propia biblioteca (`_touch`)
    with library.connect() as conn:
        conn.execute(
            INSERT_SONG,
            (
                500,
                "/musica/x/Otra.mp3",
                "/musica",
                "x",
                "Otra.mp3",
                "Alguien",
                "Otra Cancion Mas",
                "",
                "",
                "",
                100.0,
                0,
                0,
            ),
        )
    assert resolve.match_song("Otra Cancion Mas").status == "missing", (
        "el indice no sabe de ella aun"
    )
    library._touch()
    assert resolve.match_song("Otra Cancion Mas").status == "found"
    first = resolve.INDEX.snapshot()
    assert resolve.INDEX.snapshot() is first, "no se reconstruye si nada cambio"
    resolve.invalidate()
    assert resolve.INDEX.snapshot() is not first


def test_another_db_another_index(base, tmp_path, monkeypatch):
    assert resolve.match_song("Rey Infinito").status == "found"
    monkeypatch.setattr(config, "DATABASE", tmp_path / "otra.db")
    assert resolve.match_song("Rey Infinito").status == "missing", "la otra base esta vacia"


def test_the_index_is_built_once_under_a_lock(base, monkeypatch):
    resolve.invalidate()
    loads: list[int] = []
    original = resolve._Snapshot.load

    def counted_load(self, conn):
        loads.append(threading.get_ident())
        time.sleep(0.05)  # que los demas hilos lleguen mientras se construye
        return original(self, conn)

    monkeypatch.setattr(resolve._Snapshot, "load", counted_load)
    barrera = threading.Barrier(8)
    results: list = []

    def worker() -> None:
        barrera.wait()
        r = resolve.match_song("Digno De Adorar")
        results.append((r.status, pick(r), song_ids(r)))

    threads = [threading.Thread(target=worker) for _ in range(8)]
    for h in threads:
        h.start()
    for h in threads:
        h.join(30)
    assert len(loads) == 1, "una sola construccion para los ocho hilos"
    assert len(results) == 8 and len(set(map(repr, results))) == 1


def test_the_song_data_comes_from_the_db(db):
    sid = db.add("Alguien", "Una Cancion", album="Un Disco", stars=3, favorite=1)
    with library.connect() as conn:
        conn.execute("UPDATE songs SET key='Bb', bpm=87.4, duration=213.6 WHERE id=?", (sid,))
    resolve.invalidate()
    r = resolve.match_song("Una Cancion")
    assert r.song == {
        "id": sid,
        "artist": "Alguien",
        "title": "Una Cancion",
        "album": "Un Disco",
        "duration": 214,
        "key": "Bb",
        "bpm": 87,
        "stars": 3,
        "favorite": True,
    }


# --------------------------------------------------------------- escala


@slow
def test_50000_songs_within_a_time_limit(db):
    """El emparejador con 50 000 canciones: el prefiltro FTS5 y la puntuacion solo
    sobre los 400 mejores. Presupuesto del diseño: menos de 300 ms por item."""
    rng = random.Random(11)  # noqa: S311
    syllables = [a + b for a in "bcdfglmnprstvz" for b in "aeiou"]

    def word() -> str:
        return "".join(rng.choice(syllables) for _ in range(rng.randint(2, 4)))

    vocab = [word() for _ in range(5000)]
    common_words = [
        "gloria",
        "dios",
        "señor",
        "santo",
        "rey",
        "vida",
        "amor",
        "luz",
        "fuego",
        "cielo",
    ]
    stop_words = ["de", "la", "el", "en", "y", "que", "tu", "mi", "su", "con"]
    artists = [f"{word()} {word()}".title() for _ in range(800)]

    def title() -> str:
        ws = []
        for i in range(rng.randint(1, 5)):
            r = rng.random()
            if r < 0.25:
                ws.append(rng.choice(common_words))
            elif r < 0.45 and i:
                ws.append(rng.choice(stop_words))
            else:
                ws.append(rng.choice(vocab))
        return " ".join(w.capitalize() for w in ws)

    planted = {
        "Zafiro Del Amanecer": "Coro Zafiro",
        "Quimera Eterna": "Banda Quimera",
        "Mañana Sera Otro Dia": "Los Del Alba",
        "Pandora Y El Fuego Santo": "Trio Pandora",
        "Nebulosa": "Astro Coro",
    }

    def row(i: int, artist: str, t: str) -> tuple:
        file = f"{artist} - {t}.mp3"
        folder = f"Artistas/{artist}"
        return (i, f"/n/{i}/{file}", "/n", folder, file, artist, t, "", "", "", 200.0, 0, 0)

    rows = [row(i, rng.choice(artists), title()) for i in range(1, 50_001)]
    rows += [row(k, a, t) for k, (t, a) in enumerate(planted.items(), start=50_001)]
    t0 = time.perf_counter()
    with library.connect() as conn:
        conn.executemany(INSERT_SONG, rows)
    resolve.invalidate()
    snap = resolve.INDEX.snapshot()
    assert snap.large and snap.count == 50_005
    assert time.perf_counter() - t0 < time_limit(60), "montar la base y el indice"

    requested = [
        ("*1.- Zafiro Del Amanecer* ✅🙏🏻 @Ana Lopez Voz (En Su Presencia)", "found"),
        ("quimera eterna", "found"),
        ("QUIMERA ETERNA (Video Oficial) HD", "found"),
        ("Mañana Sera Otro Dia", "found"),
        ("Manana Sera Otro Dia", "found"),
        ("Pandora Y El Fuego Santo @Beto Ruiz", "found"),
        ("Nebulosa", "found"),
        ("Zafiro Del Amanecr", "probable"),
        ("Pandora Y El", "probable"),
        ("Tú Zafiro Sin Amanecer Ni Fin", "missing"),
        ("Cuan Grande Es Su Lejano Reino Eterno", "missing"),
        ("Oceans Where Feet May Fail", "missing"),
        ("Tú", "missing"),
        ("a", "missing"),
    ]
    items = requested * 4  # 56 items, como una lista larga
    t0 = time.perf_counter()
    out = resolve.resolve_items([t for t, _ in items])
    total = time.perf_counter() - t0
    assert total / len(items) < time_limit(0.3), f"{total / len(items) * 1000:.0f} ms por item"
    assert not any(r.failed for r in out)
    for (text, state), r in zip(items, out, strict=True):
        assert r.status == state, (text, r.status)
        if state != "missing":
            assert pick(r) > 50_000, text  # una de las plantadas

    # ninguno pasa de un segundo y medio, ni siquiera los hostiles
    for name in ("200 KB de palabras", "sintaxis FTS con comillas", "Tú", "una palabra gigante"):
        t0 = time.perf_counter()
        resolve.match_song(HOSTILES[name])
        assert time.perf_counter() - t0 < time_limit(1.5), name
