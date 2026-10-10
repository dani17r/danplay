"""Listas con cerrojo: `playlists.place` y las demas escrituras de una lista.

Contra una base temporal REAL (el esquema de verdad, canciones sinteticas
metidas en `songs` sin audio), porque lo que se prueba es la transaccion: que
nadie vea la lista a medias y que dos escritores a la vez no se pisen. Los
hilos arrancan juntos con un `Barrier` y un vigilante mira la lista mientras
tanto: en cada foto las posiciones tienen que ser exactamente 0..n-1.
"""

import itertools
import os
import random
import threading

import pytest

from danplay import config, library, playlists, tags

_COUNTER = itertools.count(1)


def rng(seed):
    """Un generador al azar con semilla: las pruebas se repiten igual."""
    return random.Random(seed)  # noqa: S311


@pytest.fixture
def db(tmp_path, monkeypatch):
    """Una base nueva y vacia por prueba."""
    monkeypatch.setattr(config, "DATABASE", tmp_path / "listas.db")
    return tmp_path


@pytest.fixture
def stamps(monkeypatch):
    """Apunta cada llamada a escribir etiquetas (con los ids que lleva) y no
    escribe nada."""
    calls = []
    monkeypatch.setattr(
        playlists, "_stamp_playlists_into_files", lambda ids: calls.append(list(ids))
    )
    return calls


def make_songs(n):
    """`n` canciones sinteticas, sin audio, directamente en el indice."""
    ids = []
    with library.connect() as conn:
        for _ in range(n):
            k = next(_COUNTER)
            cur = conn.execute(
                "INSERT INTO songs (path, root, folder, file, artist, title) VALUES (?,?,?,?,?,?)",
                (
                    f"/no/existe/tema-{k}.mp3",
                    "/no/existe",
                    "/no/existe",
                    f"tema-{k}.mp3",
                    "Grupo Inventado",
                    f"Tema {k}",
                ),
            )
            ids.append(cur.lastrowid)
    return ids


def make_outside(n):
    """`n` canciones de fuera de la biblioteca: su id en las listas es NEGATIVO."""
    ids = []
    with library.connect() as conn:
        for _ in range(n):
            k = next(_COUNTER)
            cur = conn.execute(
                "INSERT INTO external_songs (path, file, title) VALUES (?,?,?)",
                (f"/no/existe/fuera-{k}.mp3", f"fuera-{k}.mp3", f"Fuera {k}"),
            )
            assert cur.lastrowid is not None
            ids.append(-cur.lastrowid)
    return ids


def song_id(path):
    song = library.by_path(path)
    assert song is not None
    return song["id"]


def new_list(name="domingo"):
    return playlists.create(name)["id"]


def stored(lid):
    """(cancion, posicion) de todas las filas, como estan en la base."""
    with library.connect() as conn:
        return [
            (r["song_id"], r["position"])
            for r in conn.execute(
                "SELECT song_id, position FROM playlist_songs WHERE playlist_id=? "
                "ORDER BY position, rowid",
                (lid,),
            )
        ]


def stamped_at(lid):
    """(cancion, cuando entro) de cada fila: cambia si se vuelve a escribir."""
    with library.connect() as conn:
        return [
            (r["song_id"], r["added"])
            for r in conn.execute(
                "SELECT song_id, added FROM playlist_songs WHERE playlist_id=? ORDER BY song_id",
                (lid,),
            )
        ]


def positions(lid):
    """Las posiciones de la lista en UNA sola consulta (una foto coherente)."""
    with library.connect() as conn:
        return [
            r[0]
            for r in conn.execute(
                "SELECT position FROM playlist_songs WHERE playlist_id=? ORDER BY position",
                (lid,),
            )
        ]


def order(lid):
    return [s["id"] for s in playlists.songs(lid)]


def assert_tidy(lid):
    pos = positions(lid)
    assert pos == list(range(len(pos))), f"posiciones rotas: {pos}"


# ------------------------------------------------------------------ place


def test_place_inserta_lo_que_falta_y_ordena_todo(db):
    a, b, c, d, e, _f = make_songs(6)
    lid = new_list()
    playlists.add(lid, [a, b, c])
    r = playlists.place(lid, [c, d, a, e])
    # lo nombrado, en su orden; detras, lo que no se nombra, como estaba
    assert r == {"added": 2, "total": 5, "order": [c, d, a, e, b]}
    assert order(lid) == r["order"]
    assert stored(lid) == [(c, 0), (d, 1), (a, 2), (e, 3), (b, 4)]


def test_place_mete_cada_nueva_justo_despues_de_su_anterior(db):
    """Lo que hace falta al completar una lista de domingo: A, (nueva), B,
    (nueva), C, con A, B y C ya en la lista."""
    a, b, c, n1, n2 = make_songs(5)
    lid = new_list()
    playlists.add(lid, [a, b, c])
    r = playlists.place(lid, [a, n1, b, n2, c])
    assert r["order"] == [a, n1, b, n2, c] and r["added"] == 2
    assert_tidy(lid)


def test_place_dos_nuevas_seguidas_y_una_al_principio_quedan_como_se_piden(db):
    """Dos faltantes seguidos entre A y D (sin invertirse) y una al principio."""
    a, d, n1, n2, first = make_songs(5)
    lid = new_list()
    playlists.add(lid, [a, d])
    r = playlists.place(lid, [first, a, n1, n2, d])
    assert r == {"added": 3, "total": 5, "order": [first, a, n1, n2, d]}
    assert stored(lid) == [(first, 0), (a, 1), (n1, 2), (n2, 3), (d, 4)]


def test_place_deja_detras_lo_no_mencionado_en_su_orden(db):
    a, b, c, d = make_songs(4)
    lid = new_list()
    playlists.add(lid, [a, b, c, d])
    r = playlists.place(lid, [c])
    assert r == {"added": 0, "total": 4, "order": [c, a, b, d]}
    assert_tidy(lid)


def test_place_en_una_lista_vacia_y_sin_nada_que_poner(db):
    lid = new_list()
    assert playlists.place(lid, []) == {"added": 0, "total": 0, "order": []}
    a, b = make_songs(2)
    assert playlists.place(lid, [b, a]) == {"added": 2, "total": 2, "order": [b, a]}
    assert playlists.place(lid, iter(())) == {"added": 0, "total": 2, "order": [b, a]}
    assert playlists.place(lid, a) == {"added": 0, "total": 2, "order": [a, b]}  # un id suelto


def test_place_respeta_las_canciones_de_fuera_de_la_biblioteca(db):
    """Los ids negativos son archivos abiertos desde fuera: ni se pierden ni
    se confunden con los de la biblioteca."""
    a, b = make_songs(2)
    x, y = make_outside(2)
    lid = new_list()
    playlists.add(lid, [a, x, b])
    # x no se nombra: se queda; y es nueva: entra; -987654 no existe: se ignora
    r = playlists.place(lid, [b, y, -987654, a])
    assert r == {"added": 1, "total": 4, "order": [b, y, a, x]}
    assert order(lid) == [b, y, a, x]
    assert [s.get("external", False) for s in playlists.songs(lid)] == [False, True, False, True]
    assert_tidy(lid)
    # y tambien se puede mover: sigue siendo suya
    assert playlists.place(lid, [x, b])["order"] == [x, b, y, a]


def test_place_ignora_lo_que_no_es_una_cancion(db):
    a, b = make_songs(2)
    lid = new_list()
    r = playlists.place(lid, [999999, "no", None, a, 0, -1234, "7x", b, 888888.0])
    assert r == {"added": 2, "total": 2, "order": [a, b]}
    assert stored(lid) == [(a, 0), (b, 1)], "ni filas huerfanas de ids inventados"
    # un id que viene como texto, si es un numero, vale
    c = make_songs(1)[0]
    assert playlists.place(lid, [str(c), a])["order"] == [c, a, b]


def test_place_no_repite_lo_repetido(db):
    a, b, c = make_songs(3)
    lid = new_list()
    r = playlists.place(lid, [a, b, a, b, c, c, a])
    assert r == {"added": 3, "total": 3, "order": [a, b, c]}
    assert stored(lid) == [(a, 0), (b, 1), (c, 2)]
    # y con lo repetido ya dentro, igual
    r = playlists.place(lid, [c, c, a, a])
    assert r == {"added": 0, "total": 3, "order": [c, a, b]}


def test_place_sella_las_etiquetas_una_sola_vez_y_solo_de_las_nuevas(db, stamps):
    a, b, c, d = make_songs(4)
    lid = new_list()
    playlists.add(lid, [a])
    stamps.clear()
    playlists.place(lid, [a, b, c, d])
    assert len(stamps) == 1, stamps
    assert sorted(stamps[0]) == sorted([b, c, d])
    # solo moverlas no cambia de que listas es cada una: nada que sellar
    stamps.clear()
    playlists.place(lid, [d, c, b, a])
    assert stamps == []
    # una nueva entre las de siempre: un sellado, solo de ella
    (e,) = make_songs(1)
    playlists.place(lid, [e, a])
    assert stamps == [[e]]


def test_place_es_idempotente(db, stamps):
    a, b, c, d = make_songs(4)
    x = make_outside(1)[0]
    lid = new_list()
    playlists.add(lid, [a, b])
    first = playlists.place(lid, [b, c, x, a, d])
    rows = stored(lid)
    added_at = stamped_at(lid)
    revision, calls = library.revision(), len(stamps)
    again = playlists.place(lid, [b, c, x, a, d])
    assert again == {**first, "added": 0}
    assert stored(lid) == rows
    assert stamped_at(lid) == added_at, "ni siquiera toca las filas que ya estan"
    assert library.revision() == revision, "nada cambio: no hay nada que avisar"
    assert len(stamps) == calls, "ni se tocan los archivos"


def test_place_avisa_de_que_la_biblioteca_cambio(db):
    a, b = make_songs(2)
    lid = new_list()
    before = library.revision()
    playlists.place(lid, [a, b])
    assert library.revision() > before
    before = library.revision()
    playlists.place(lid, [b, a])  # solo reordena
    assert library.revision() > before


def test_place_en_una_lista_que_no_existe_no_escribe_nada(db):
    a = make_songs(1)[0]
    lid = new_list()
    playlists.remove(lid)
    for gone in (lid, 424242, "no", None):
        with pytest.raises(ValueError, match="no existe"):
            playlists.place(gone, [a])
    with library.connect() as conn:
        assert conn.execute("SELECT COUNT(*) FROM playlist_songs").fetchone()[0] == 0


def test_place_deja_detras_y_sin_ensenar_la_cancion_cuyo_archivo_se_fue(db):
    """Su fila se queda (si el archivo vuelve, vuelve a la lista), pero detras
    y sin contar en lo que enseña la lista."""
    a, b, c = make_songs(3)
    lid = new_list()
    playlists.add(lid, [a, b, c])
    with library.connect() as conn:
        conn.execute("DELETE FROM songs WHERE id=?", (b,))
    (n,) = make_songs(1)
    r = playlists.place(lid, [c, n])
    assert r == {"added": 1, "total": 3, "order": [c, n, a]}
    assert [song for song, _ in stored(lid)] == [c, n, a, b]
    assert_tidy(lid)
    assert order(lid) == r["order"]
    # un id que ya no es una cancion no se puede poner
    assert playlists.place(lid, [b, a])["order"] == [a, c, n]
    # y si el archivo vuelve, reaparece en la lista
    with library.connect() as conn:
        conn.execute(
            "INSERT INTO songs (id, path, root, folder, file, artist, title) VALUES (?,?,?,?,?,?,?)",
            (b, "/no/existe/vuelve.mp3", "/no/existe", "/no/existe", "vuelve.mp3", "G", "Vuelve"),
        )
    assert b in order(lid)


def test_place_repara_posiciones_repetidas_o_con_huecos(db):
    a, b, c = make_songs(3)
    lid = new_list()
    with library.connect() as conn:
        conn.executemany(
            "INSERT INTO playlist_songs (playlist_id, song_id, position, added) VALUES (?,?,?,?)",
            [(lid, a, 0, 1.0), (lid, b, 0, 2.0), (lid, c, 5, 3.0)],
        )
    r = playlists.place(lid, [])
    assert r["order"] == [a, b, c], "al empatar, la fila mas vieja va antes"
    assert stored(lid) == [(a, 0), (b, 1), (c, 2)]


def test_place_con_mas_ids_que_variables_admite_sqlite(db):
    """Las consultas van por trozos: ni 999 ni 32766 variables son un limite."""
    with library.connect() as conn:
        conn.executemany(
            "INSERT INTO songs (path, root, folder, file) VALUES (?,?,?,?)",
            [(f"/no/existe/masa-{i}.mp3", "/", "/", f"masa-{i}.mp3") for i in range(1100)],
        )
        ids = [r[0] for r in conn.execute("SELECT id FROM songs ORDER BY id")]
    lid = new_list()
    wanted = list(reversed(ids)) + [10**7 + i for i in range(100)]
    assert playlists.existing_ids(wanted) == list(reversed(ids))
    r = playlists.place(lid, wanted)
    assert r["added"] == 1100 and r["order"] == list(reversed(ids))
    assert_tidy(lid)


def test_place_escribe_el_nombre_de_la_lista_dentro_del_archivo_y_solo_cuando_hace_falta(
    configured_library,
):
    lib, songs = configured_library
    library.add_folder(lib)
    library.scan()
    gozo, tierra = song_id(songs["gozo"]), song_id(songs["tierra"])
    lid = new_list("Domingo")
    playlists.place(lid, [tierra, gozo])
    assert tags.read_all(songs["gozo"])["playlists"] == ["Domingo"]
    assert tags.read_all(songs["tierra"])["playlists"] == ["Domingo"]
    assert tags.read_all(songs["shekinah"])["playlists"] == []
    stat = os.stat(songs["gozo"]).st_mtime_ns
    playlists.place(lid, [gozo, tierra])  # solo las reordena: los archivos ni se tocan
    assert os.stat(songs["gozo"]).st_mtime_ns == stat
    assert order(lid) == [gozo, tierra]


# ---------------------------------------------- lo que ya hacian, sin cambios


def test_add_pone_al_final_y_cuenta_solo_las_que_entran(db):
    a, b, c = make_songs(3)
    lid = new_list()
    assert playlists.add(lid, [a, b]) == 2
    assert playlists.add(lid, [b, c, c, 99999]) == 1
    assert playlists.add(lid, c) == 0
    assert playlists.add(lid, [99999]) == 0
    assert order(lid) == [a, b, c]
    assert_tidy(lid)


def test_add_sella_las_etiquetas_de_lo_que_se_pide(db, stamps):
    a, b = make_songs(2)
    lid = new_list()
    playlists.add(lid, [a, b, 99999])
    assert stamps == [[a, b]]


def test_reorder_nombra_solo_algunas_y_las_demas_se_quedan_detras(db):
    a, b, c, d = make_songs(4)
    lid = new_list()
    playlists.add(lid, [a, b, c, d])
    playlists.reorder(lid, [c, a])
    assert order(lid) == [c, a, b, d]
    assert_tidy(lid)  # antes dejaba posiciones repetidas
    playlists.reorder(lid, [d, 99999, b, d])
    assert order(lid) == [d, b, c, a]
    assert_tidy(lid)


def test_reorder_no_mete_lo_que_la_lista_no_tiene(db):
    a, b, c = make_songs(3)
    lid = new_list()
    playlists.add(lid, [a, b])
    playlists.reorder(lid, [c, b, a])
    assert order(lid) == [b, a]


def test_remove_song_y_remove_apuntan_los_archivos(db, stamps):
    a, b = make_songs(2)
    lid = new_list()
    playlists.add(lid, [a, b])
    stamps.clear()
    playlists.remove_song(lid, a)
    assert order(lid) == [b] and stamps == [[a]]
    stamps.clear()
    playlists.remove(lid)
    assert stamps == [[b]]
    assert playlists.by_id(lid) is None


def test_renombrar_apunta_las_canciones_una_vez(db, stamps):
    a, b = make_songs(2)
    lid = new_list()
    playlists.add(lid, [a, b])
    stamps.clear()
    renamed = playlists.edit(lid, name="Ensayo")
    assert renamed and renamed["name"] == "Ensayo"
    assert stamps == [[a, b]]
    stamps.clear()
    playlists.edit(lid, note="solo la nota")
    assert stamps == []


# ---------------------------------------------------------------- set_songs


def test_set_songs_deja_la_lista_exactamente_asi(db):
    a, b, c, d = make_songs(4)
    lid = new_list()
    playlists.add(lid, [a, b])
    r = playlists.set_songs(lid, [c, d, b])
    assert r == {"removed": 1, "added": 2, "total": 3}
    assert order(lid) == [c, d, b]
    assert stored(lid) == [(c, 0), (d, 1), (b, 2)]
    # sin cambios: nada que quitar ni que añadir, y el mismo orden
    assert playlists.set_songs(lid, [c, d, b]) == {"removed": 0, "added": 0, "total": 3}
    # con ids inventados, esos no cuentan
    assert playlists.set_songs(lid, [b, 999999, c]) == {"removed": 1, "added": 0, "total": 2}
    assert order(lid) == [b, c]


def test_set_songs_con_las_de_fuera(db):
    a, b = make_songs(2)
    x, y = make_outside(2)
    lid = new_list()
    playlists.add(lid, [a, x, y])
    r = playlists.set_songs(lid, [y, b, x])
    assert r == {"removed": 1, "added": 1, "total": 3}
    assert order(lid) == [y, b, x]
    assert playlists.set_songs(lid, [b]) == {"removed": 2, "added": 0, "total": 1}
    assert order(lid) == [b]


def test_set_songs_sella_las_etiquetas_una_sola_vez(db, stamps):
    a, b, c = make_songs(3)
    lid = new_list()
    playlists.add(lid, [a, b])
    stamps.clear()
    playlists.set_songs(lid, [b, c])
    assert len(stamps) == 1 and sorted(stamps[0]) == sorted([a, c])
    stamps.clear()
    playlists.set_songs(lid, [c, b])  # solo el orden
    assert stamps == []


def test_set_songs_no_toca_las_filas_de_canciones_cuyo_archivo_se_fue(db):
    a, b, c = make_songs(3)
    lid = new_list()
    playlists.add(lid, [a, b, c])
    with library.connect() as conn:
        conn.execute("DELETE FROM songs WHERE id=?", (b,))
    assert playlists.set_songs(lid, [c, a]) == {"removed": 0, "added": 0, "total": 2}
    assert order(lid) == [c, a]
    assert [song for song, _ in stored(lid)] == [c, a, b], "b sigue ahi, detras, por si vuelve"
    assert_tidy(lid)


def test_set_songs_se_comporta_como_el_modelo_con_listas_al_azar(db):
    """Contra una version de papel de lo que siempre ha hecho (quitar lo que
    sobra, añadir lo que falta, dejar el orden)."""
    rnd = rng(20260117)
    universe = make_songs(12) + make_outside(3)
    junk = [999999, -999999, "x", None]
    for n in range(40):
        lid = new_list(f"lista {n}")
        before = rnd.sample(universe, rnd.randint(0, len(universe)))
        playlists.add(lid, before)
        raw = rnd.sample(universe + junk, rnd.randint(0, 10))
        raw += rnd.sample(raw, rnd.randint(0, min(3, len(raw))))  # y algunos repetidos
        wanted = [i for i in dict.fromkeys(raw) if i in universe]
        r = playlists.set_songs(lid, raw)
        assert order(lid) == wanted, (before, raw)
        assert r == {
            "removed": len([i for i in before if i not in wanted]),
            "added": len([i for i in wanted if i not in before]),
            "total": len(wanted),
        }
        assert_tidy(lid)


# ------------------------------------------------------------------ carreras


def run_together(workers, timeout=60):
    """Arranca todos los hilos a la vez (Barrier), espera y devuelve los fallos."""
    barrier = threading.Barrier(len(workers))
    errors = []

    def wrap(work):
        def go():
            try:
                barrier.wait(10)
                work()
            except BaseException as e:  # noqa: BLE001
                errors.append(repr(e))

        return go

    threads = [threading.Thread(target=wrap(w), daemon=True) for w in workers]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout)
    assert not any(t.is_alive() for t in threads), "algun hilo se quedo colgado"
    return errors


class Watcher:
    """Mira la lista sin parar mientras otros la escriben: en CADA foto las
    posiciones tienen que ser 0..n-1. Si una operacion fuese varias
    transacciones, alguna foto caeria en medio."""

    def __init__(self, lid):
        self.lid, self.bad, self.looks = lid, [], 0
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def _run(self):
        try:
            while not self._stop.is_set():
                pos = positions(self.lid)
                self.looks += 1
                if pos != list(range(len(pos))):
                    self.bad.append(pos)
                    return
        except Exception as e:  # noqa: BLE001  (si el vigilante falla, la prueba tambien)
            self.bad.append(repr(e))

    def __enter__(self):
        self._thread.start()
        return self

    def __exit__(self, *exc):
        self._stop.set()
        self._thread.join(30)
        assert not self._thread.is_alive()


def test_place_add_y_reorder_a_la_vez_no_pierden_ni_repiten_nada(db):
    universe = make_songs(70)
    base, rest = universe[:10], universe[10:]
    lid = new_list()
    playlists.add(lid, base)
    mine = {w: rest[w::6] for w in range(6)}  # cada hilo, SUS canciones

    def placer(w):
        def work():
            rnd = rng(w)
            for k in range(0, len(mine[w]), 2):
                playlists.place(lid, [*rnd.sample(base, 3), *mine[w][k : k + 2]])

        return work

    def adder(w):
        def work():
            for song in mine[w]:
                playlists.add(lid, [song])

        return work

    def reorderer():
        rnd = rng(99)
        for _ in range(25):
            now = order(lid)
            rnd.shuffle(now)
            playlists.reorder(lid, now)

    def normalizer():
        for _ in range(25):
            playlists.place(lid, [])

    with Watcher(lid) as watcher:
        errors = run_together(
            [placer(0), placer(1), placer(2), adder(3), adder(4), adder(5), reorderer, normalizer]
        )
    assert errors == []
    assert watcher.bad == [], f"alguien vio la lista a medias: {watcher.bad}"
    assert watcher.looks > 0
    final = order(lid)
    assert len(final) == len(set(final)), "canciones repetidas"
    assert set(final) == set(universe), "se perdio alguna"
    assert_tidy(lid)


def test_set_songs_en_medio_de_todo_lo_demas_deja_siempre_una_lista_sana(db):
    universe = make_songs(40)
    lid = new_list()
    playlists.add(lid, universe[:8])

    def placer():
        rnd = rng(1)
        for _ in range(25):
            playlists.place(lid, rnd.sample(universe, 6))

    def adder():
        rnd = rng(2)
        for _ in range(25):
            playlists.add(lid, rnd.sample(universe, 4))

    def setter():
        rnd = rng(3)
        for _ in range(25):
            playlists.set_songs(lid, rnd.sample(universe, rnd.randint(0, 12)))

    def reorderer():
        rnd = rng(4)
        for _ in range(25):
            now = order(lid)
            rnd.shuffle(now)
            playlists.reorder(lid, now)

    with Watcher(lid) as watcher:
        errors = run_together([placer, adder, setter, setter, reorderer])
    assert errors == []
    assert watcher.bad == [], f"alguien vio la lista a medias: {watcher.bad}"
    final = order(lid)
    assert len(final) == len(set(final)) and set(final) <= set(universe)
    assert_tidy(lid)
    # y despues de todo eso, dejarla exacta sigue funcionando
    playlists.set_songs(lid, universe[:5])
    assert order(lid) == universe[:5]
    assert_tidy(lid)


def placing(lid, wanted):
    return lambda: playlists.place(lid, wanted)


def test_dos_place_a_la_vez_no_se_pisan_las_posiciones(db):
    """La carrera de siempre: los dos leen la lista [A], cada uno inserta SU
    nueva en la posicion 1 y quedan dos con la misma."""
    for round_ in range(25):
        a, n1, n2 = make_songs(3)
        lid = new_list(f"ronda {round_}")
        playlists.add(lid, [a])
        errors = run_together([placing(lid, [a, n1]), placing(lid, [a, n2])])
        assert errors == []
        assert sorted(order(lid)) == sorted([a, n1, n2]), f"ronda {round_}"
        assert_tidy(lid)


def test_leer_y_poner_bajo_el_cerrojo_no_pierde_lo_de_los_demas(db):
    """El patron de quien completa una lista: leer como esta, calcular el orden
    final y ponerlo, todo bajo `LOCK`. Cada hilo mete una nueva al final de lo
    que haya VISTO, y al acabar estan todas."""
    base = make_songs(4)
    mine = make_songs(8)
    lid = new_list()
    playlists.add(lid, base)

    def adding(song):
        def work():
            with playlists.LOCK:
                now = order(lid)
                playlists.place(lid, [*now, song])

        return work

    with Watcher(lid) as watcher:
        errors = run_together([adding(s) for s in mine])
    assert errors == [] and watcher.bad == []
    final = order(lid)
    assert final[:4] == base and sorted(final[4:]) == sorted(mine)
    assert_tidy(lid)


def test_crear_la_misma_lista_a_la_vez_da_una_sola(db):
    results = []

    def work():
        results.append(playlists.create("domingo"))

    errors = run_together([work] * 8)
    assert errors == []
    assert len({r["id"] for r in results}) == 1
    assert sum(1 for r in results if r["created"]) == 1
    assert len(playlists.list_all()) == 1


def test_quien_envuelve_todo_en_el_cerrojo_puede_llamar_a_las_demas(db):
    """Es un RLock: leer, calcular y escribir sin que nadie se cuele."""
    a, b, c = make_songs(3)
    lid = new_list()
    with playlists.LOCK:
        playlists.add(lid, [a, b])
        with playlists.LOCK:
            now = order(lid)
            r = playlists.place(lid, [c, *now])
    assert r["order"] == [c, a, b]


OPS = {
    "add": lambda lid, ids: playlists.add(lid, [ids[1]]),
    "remove_song": lambda lid, ids: playlists.remove_song(lid, [ids[0]]),
    "reorder": lambda lid, ids: playlists.reorder(lid, [ids[0]]),
    "set_songs": lambda lid, ids: playlists.set_songs(lid, [ids[1]]),
    "place": lambda lid, ids: playlists.place(lid, [ids[1], ids[0]]),
    "edit": lambda lid, ids: playlists.edit(lid, name="otro nombre"),
    "remove": lambda lid, ids: playlists.remove(lid),
    "create": lambda lid, ids: playlists.create("otra lista"),
    "rename_folder": lambda lid, ids: playlists.rename_folder(lid, "carpeta"),
}


@pytest.mark.parametrize("op", sorted(OPS))
def test_cada_escritura_espera_al_cerrojo(db, op):
    ids = make_songs(2)
    lid = new_list()
    playlists.add(lid, [ids[0]])
    started, finished = threading.Event(), threading.Event()

    def work():
        started.set()
        OPS[op](lid, ids)
        finished.set()

    with playlists.LOCK:
        t = threading.Thread(target=work, daemon=True)
        t.start()
        assert started.wait(5)
        assert not finished.wait(0.25), f"{op} no espero al cerrojo"
    assert finished.wait(10), f"{op} no termino al soltar el cerrojo"
    t.join(5)
