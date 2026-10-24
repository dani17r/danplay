//! El emparejador contra una biblioteca SINTÉTICA (lo del lote y lo del índice
//! por revisión van en server/src/services/chat/resolve.rs): artistas
//! inventados y títulos de canciones públicas en una base temporal. Las filas
//! se insertan a mano en `songs`, sin generar audio, y el índice FTS5 las
//! sigue por sus disparadores como siempre.
use super::*;
use crate::index::history;
use crate::testkit::Scratch;
use serde_json::json;
use std::time::{Duration, Instant};

const SONG_KEYS: [&str; 9] = [
    "id", "artist", "title", "album", "duration", "key", "bpm", "stars", "favorite",
];
const INSERT_SONG: &str = "INSERT INTO songs (id, path, root, folder, file, artist, title, album, \
     feat, match_key, duration, stars, favorite) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)";

// ------------------------------------------------------------------ ayudas

/// Una canción para `Lib::add_song`.
#[derive(Default)]
struct Song<'a> {
    artist: &'a str,
    title: &'a str,
    file: Option<&'a str>,
    album: &'a str,
    stars: i64,
    favorite: i64,
    sid: Option<i64>,
}

/// Una biblioteca sintética en una base temporal.
struct Lib {
    _dir: Scratch,
    db: PathBuf,
    conn: Connection,
    next_id: i64,
    /// el `FULL_LIMIT` con el que se lee (3: con el prefiltro FTS5)
    full_limit: usize,
}

impl Lib {
    fn new() -> Self {
        let dir = Scratch::new("emparejador");
        let db = dir.join("danplay.db");
        let conn = open(&db).expect("abrir");
        crate::index::schema::prepare(&conn).expect("esquema");
        Self {
            _dir: dir,
            db,
            conn,
            next_id: 1,
            full_limit: FULL_LIMIT,
        }
    }

    fn add(&mut self, artist: &str, title: &str) -> i64 {
        self.add_song(&Song {
            artist,
            title,
            ..Song::default()
        })
    }

    fn add_song(&mut self, s: &Song<'_>) -> i64 {
        let sid = s.sid.unwrap_or(self.next_id);
        self.next_id = self.next_id.max(sid + 1);
        let file = s.file.map_or_else(
            || {
                if s.artist.is_empty() {
                    format!("{}.mp3", s.title)
                } else {
                    format!("{} - {}.mp3", s.artist, s.title)
                }
            },
            str::to_owned,
        );
        let folder = if s.artist.is_empty() {
            String::new()
        } else {
            format!("Artistas/{}", s.artist)
        };
        self.conn
            .execute(
                INSERT_SONG,
                rusqlite::params![
                    sid,
                    format!("/musica/{folder}/{file}"),
                    "/musica",
                    folder,
                    file,
                    s.artist,
                    s.title,
                    s.album,
                    "",
                    "",
                    200.0,
                    s.stars,
                    s.favorite
                ],
            )
            .expect("insertar");
        sid
    }

    fn path_of(&self, sid: i64) -> String {
        self.conn
            .query_row("SELECT path FROM songs WHERE id=?", [sid], |r| r.get(0))
            .expect("la ruta")
    }

    fn remove(&self, sid: i64) {
        self.conn
            .execute("DELETE FROM songs WHERE id=?", [sid])
            .expect("borrar");
    }

    fn sql(&self, query: &str) {
        self.conn.execute_batch(query).expect("sql");
    }

    /// El índice de ahora (se vuelve a cargar en cada llamada).
    fn index(&self) -> Index {
        Index::load(&self.conn, &self.db, self.full_limit).expect("el índice")
    }

    /// ¿Tenemos esta canción? (solo el título)
    fn m(&self, title: &str) -> Resolution {
        self.ask(&Ask {
            title,
            ..Ask::default()
        })
    }

    /// ¿Tenemos esta canción? (título y artista)
    fn ma(&self, title: &str, artist: &str) -> Resolution {
        self.ask(&Ask {
            title,
            artist,
            ..Ask::default()
        })
    }

    fn ask(&self, a: &Ask<'_>) -> Resolution {
        ask_in(&self.index(), a)
    }
}

/// Lo que se le pide a `match_song`, con sus valores por defecto.
struct Ask<'a> {
    title: &'a str,
    artist: &'a str,
    hints: Vec<String>,
    url: &'a str,
    video_title: &'a str,
    prefer: Vec<i64>,
    limit: usize,
    strict: bool,
}

impl Default for Ask<'_> {
    fn default() -> Self {
        Self {
            title: "",
            artist: "",
            hints: Vec::new(),
            url: "",
            video_title: "",
            prefer: Vec::new(),
            limit: 5,
            strict: false,
        }
    }
}

fn ask_in(index: &Index, a: &Ask<'_>) -> Resolution {
    let prefer: HashSet<i64> = a.prefer.iter().copied().collect();
    index
        .matching(
            a.title,
            a.artist,
            &a.hints,
            a.url,
            a.video_title,
            &prefer,
            a.limit,
            a.strict,
        )
        .expect("emparejar")
}

/// Los ids de la biblioteca de las pruebas, por nombre corto.
struct Base {
    dda_vivo: i64,
    dda: i64,
    dda_acu: i64,
    dda_medley: i64,
    sublime: i64,
    castillo: i64,
    cena: i64,
    manana_n: i64,
    manana: i64,
    ano: i64,
    rey: i64,
    big: i64,
    tu: i64,
    es_el: i64,
    dont: i64,
    hosanna: i64,
    sub: i64,
    dirty_tags: i64,
    crossed: i64,
    babel: i64,
    babel_r: i64,
    babel_r2: i64,
}

/// La biblioteca de las pruebas: una canción en tres versiones y un medley
/// que la lleva, canciones con ñ y con apóstrofo, copias « - r», una
/// etiqueta sucia y otra cruzada, y títulos hechos solo de palabras vacías.
fn base() -> (Lib, Base) {
    let mut db = Lib::new();
    let dda_vivo = db.add("Coro Aurora", "Digno De Adorar (En Vivo)");
    let dda = db.add("Banda Horizonte", "Digno De Adorar");
    let dda_acu = db.add("Lucero D'Alba", "Digno De Adorar (Acustico)");
    let dda_medley = db.add(
        "Trio Faro",
        "Toma Tu Lugar + Dios + Digno De Adorar (Espontaneo)",
    );
    let sublime = db.add("Coro Aurora", "Sublime Gracia");
    let castillo = db.add("Banda Horizonte", "Castillo Fuerte Es Nuestro Dios");
    db.add("Lucero D'Alba", "Cuan Grande Es Dios");
    db.add("Trio Faro", "Santo Santo Santo");
    let cena = db.add("Coro Aurora", "Cena Del Señor");
    let manana_n = db.add("Banda Horizonte", "Mañana");
    let manana = db.add("Lucero D'Alba", "Manana");
    let ano = db.add("Trio Faro", "Año Nuevo");
    let rey = db.add("Lucero D'Alba", "Rey Infinito");
    db.add("Coro Aurora", "De Gloria En Gloria");
    let big = db.add("Banda Horizonte", "Grande Y Fuerte Es Nuestro Dios");
    db.add("Trio Faro", "Personal");
    db.add("Altura Worship", "Alabanza");
    let tu = db.add("Coro Aurora", "Tú");
    let es_el = db.add("Trio Faro", "Es El");
    let dont = db.add("Lucero D'Alba", "Don't Stop Believin'");
    let hosanna = db.add("Banda Horizonte", "Hosanna En Las Alturas");
    let sub = db.add("Banda Horizonte", "Fuego Santo (Cae El Muro)");
    // la etiqueta de título trae al artista y ruido de YouTube; el archivo está limpio
    let dirty_tags = db.add_song(&Song {
        artist: "Coro Aurora",
        title: "Coro Aurora - Fuego En El Altar (Video Oficial)",
        file: Some("Coro Aurora - Fuego En El Altar.mp3"),
        ..Song::default()
    });
    // etiquetas cruzadas: el «artista» es el título y el «título», el artista
    let crossed = db.add_song(&Song {
        artist: "Rio De Vida",
        title: "Pedro Arango",
        file: Some("Rio De Vida - Pedro Arango.mp3"),
        ..Song::default()
    });
    // copias de la casa
    let copy = |db: &mut Lib, title: &str, file: &str| {
        db.add_song(&Song {
            artist: "Coro Aurora",
            title,
            file: Some(file),
            ..Song::default()
        })
    };
    let babel = copy(&mut db, "Babel", "Coro Aurora - Babel.mp3");
    let babel_r = copy(&mut db, "Babel - r", "Coro Aurora - Babel - r.mp3");
    let babel_r2 = copy(&mut db, "Babel - r2", "Coro Aurora - Babel - r2.mp3");
    let ids = Base {
        dda_vivo,
        dda,
        dda_acu,
        dda_medley,
        sublime,
        castillo,
        cena,
        manana_n,
        manana,
        ano,
        rey,
        big,
        tu,
        es_el,
        dont,
        hosanna,
        sub,
        dirty_tags,
        crossed,
        babel,
        babel_r,
        babel_r2,
    };
    (db, ids)
}

fn id_of(song: &Dict) -> i64 {
    song["id"].as_i64().expect("id")
}

fn song_ids(r: &Resolution) -> Vec<i64> {
    r.candidates.iter().map(|c| id_of(&c.song)).collect()
}

/// La canción elegida (la prueba falla si no hay).
fn chosen(r: &Resolution) -> &Dict {
    r.song
        .as_ref()
        .unwrap_or_else(|| panic!("no se eligió ninguna ({})", r.status))
}

fn pick(r: &Resolution) -> i64 {
    id_of(chosen(r))
}

fn found_or_ambiguous(r: &Resolution) -> bool {
    matches!(r.status, "found" | "ambiguous")
}

/// Llama a `f` con un tope de tiempo: si se cuelga, la prueba falla (no se
/// queda esperando).
fn within<T: Send + 'static>(seconds: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(seconds))
        .unwrap_or_else(|_| panic!("se quedó más de {seconds} s (o falló)"))
}

/// El tope de tiempo de lo que se mide, más holgado sin optimizar (`cargo
/// test` compila sin optimizar, y SQLite también).
fn time_limit(seconds: f64) -> f64 {
    if cfg!(debug_assertions) {
        seconds * 10.0
    } else {
        seconds
    }
}

// ------------------------------------------------- estados y orden

#[test]
fn exact_title_is_found() {
    let (db, b) = base();
    let r = db.m("Rey Infinito");
    assert!(r.status == "found" && r.via == "titulo" && !r.other_version);
    assert_eq!(pick(&r), b.rey);
    let mut keys: Vec<&str> = chosen(&r).keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut want = SONG_KEYS.to_vec();
    want.sort_unstable();
    assert_eq!(
        keys, want,
        "la fila ligera de siempre (la de `_song_brief`)"
    );
    assert_eq!(Some(&r.candidates[0].song), r.song.as_ref());
    assert_eq!(
        (r.candidates[0].why, r.candidates[0].score),
        ("titulo exacto", 1.0)
    );
}

/// «Digno De Adorar» está en estudio, en vivo y acústica (y dentro de un
/// medley): se elige una y las demás van de alternativas, con el medley la
/// última.
#[test]
fn three_versions_is_ambiguous_with_one_chosen() {
    let (db, b) = base();
    let r = db.m("Digno De Adorar");
    assert!(r.status == "ambiguous" && r.via == "titulo");
    let versions = [b.dda, b.dda_vivo, b.dda_acu];
    assert!(versions.contains(&pick(&r)));
    let ids = song_ids(&r);
    assert_eq!(ids[0], pick(&r), "la elegida va primero");
    let mut first3 = ids[..3].to_vec();
    first3.sort_unstable();
    let mut want = versions.to_vec();
    want.sort_unstable();
    assert_eq!(first3, want, "las otras dos versiones, de alternativas");
    assert!(ids[3] == b.dda_medley && r.candidates[3].score < r.candidates[0].score);
    assert!(r.note.contains("2 versiones más"), "{}", r.note);
    // sin nada que decida, gana el título más corto (la versión de estudio)
    assert_eq!(pick(&r), b.dda);
}

#[test]
fn the_requested_version_is_no_longer_ambiguous() {
    let (db, b) = base();
    let live = db.m("Digno De Adorar (En Vivo)");
    assert_eq!((live.status, pick(&live)), ("found", b.dda_vivo));
    let acoustic = db.m("digno de adorar acustico");
    assert_eq!((acoustic.status, pick(&acoustic)), ("found", b.dda_acu));
    // la nota de la lista pide la versión igual que el paréntesis
    let with_note = db.ask(&Ask {
        title: "Digno De Adorar",
        hints: vec!["(En Vivo)".into()],
        ..Ask::default()
    });
    assert_eq!((with_note.status, pick(&with_note)), ("found", b.dda_vivo));
    // y el artista que la canta
    let by_banda = db.ma("Digno De Adorar", "Banda Horizonte");
    assert_eq!((by_banda.status, pick(&by_banda)), ("found", b.dda));
    let by_lucero = db.ma("Digno De Adorar", "Lucero D Alba");
    assert_eq!((by_lucero.status, pick(&by_lucero)), ("found", b.dda_acu));
}

#[test]
fn a_noisy_query_finds_the_same_song() {
    let (db, _) = base();
    let clean_match = db.m("Digno De Adorar");
    for text in [
        "*1.- Digno De Adorar* @Ana Lopez @Beto Ruiz (En Su Presencia) Voz",
        "1) Digno De Adorar @Ana Lopez Voz",
        "*Digno De Adorar*❤️🙏🏻",
        "• _Digno De Adorar_ 🎶 (tono G)",
        "  DIGNO DE ADORAR  [Letra] HD ",
    ] {
        let dirty = db.m(text);
        assert_eq!(
            (dirty.status, song_ids(&dirty)),
            (clean_match.status, song_ids(&clean_match)),
            "{text}"
        );
    }
}

#[test]
fn whatsapp_decorations_do_not_get_in_the_way() {
    let (db, b) = base();
    for text in [
        "Rey Infinito",
        "REY INFINITO",
        "rey infinito",
        "Rey Infinito!!",
        "«Rey Infinito»",
        "\"Rey Infinito\"",
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
    ] {
        let r = db.m(text);
        assert!(found_or_ambiguous(&r) && pick(&r) == b.rey, "{text}");
    }
}

#[test]
fn a_wrong_artist_or_a_mention_does_not_hide_the_song() {
    let (db, _) = base();
    let without = db.m("Rey Infinito");
    for artist in [
        "Un Artista Cualquiera",
        "Trio Faro",
        "Ana Lopez",
        "Coro Aurora",
    ] {
        let r = db.ma("Rey Infinito", artist);
        assert_eq!(
            (r.status, pick(&r)),
            (without.status, pick(&without)),
            "{artist}"
        );
    }
    // la @mención no es el artista: aunque coincida con uno de la biblioteca, no decide
    let plain = db.m("Digno De Adorar");
    let with_mention = db.m("Digno De Adorar @Lucero D'Alba");
    assert_eq!(
        (with_mention.status, pick(&with_mention)),
        (plain.status, pick(&plain))
    );
    // lo mismo con la pista suelta de una nota
    let with_note = db.ask(&Ask {
        title: "Digno De Adorar",
        hints: vec!["Lucero D'Alba".into(), "Voz".into()],
        ..Ask::default()
    });
    assert_eq!(pick(&with_note), pick(&plain));
}

#[test]
fn the_right_artist_only_adds() {
    let (db, b) = base();
    let r = db.ma("Rey Infinito", "Lucero D'Alba");
    assert!(r.status == "found" && pick(&r) == b.rey);
    for shape in [
        "Lucero DAlba",
        "Lucero D’Alba",
        "lucero d'alba",
        "LUCERO D ALBA",
        "Lucero Dalba",
    ] {
        let r = db.ma("Digno De Adorar", shape);
        assert_eq!((r.status, pick(&r)), ("found", b.dda_acu), "{shape}");
    }
}

#[test]
fn the_artist_attached_or_first() {
    let (db, b) = base();
    for text in [
        "Rey Infinito Lucero D'Alba",
        "Lucero D'Alba - Rey Infinito",
        "Lucero D'Alba Rey Infinito",
    ] {
        let r = db.m(text);
        assert!(found_or_ambiguous(&r) && pick(&r) == b.rey, "{text}");
    }
    let r = db.m("Banda Horizonte - Digno De Adorar");
    assert_eq!((r.status, pick(&r)), ("found", b.dda));
}

#[test]
fn dirty_tags_are_searched_by_file_and_by_tag() {
    let (db, b) = base();
    for text in [
        "Fuego En El Altar",
        "Coro Aurora - Fuego En El Altar (Video Oficial)",
    ] {
        let r = db.m(text);
        assert!(r.status == "found" && pick(&r) == b.dirty_tags, "{text}");
    }
}

/// Una canción con el artista en el título y al revés: se encuentra si se
/// pide con su artista; sin él, nada dice que «Rio De Vida» sea un título.
#[test]
fn crossed_tags_are_found_with_the_artist() {
    let (db, b) = base();
    let r = db.ma("Rio De Vida", "Pedro Arango");
    assert!(r.status == "found" && pick(&r) == b.crossed);
    assert_eq!(pick(&db.ma("Pedro Arango", "Rio De Vida")), b.crossed);
    assert_eq!(db.m("Rio De Vida").status, "missing");
}

/// «Babel», «Babel - r» y «Babel - r2» son tres archivos de la misma
/// versión: no hay nada que elegir.
#[test]
fn in_house_copies_are_the_same_song() {
    let (db, b) = base();
    for text in ["Babel", "Babel - r2", "babel - r"] {
        let r = db.m(text);
        assert_eq!(r.status, "found", "{text}");
        assert!(
            song_ids(&r)
                .iter()
                .all(|i| [b.babel, b.babel_r, b.babel_r2].contains(i)),
            "{text}"
        );
        assert_eq!(pick(&r), b.babel, "{text}");
    }
}

#[test]
fn titles_made_of_stop_words() {
    let (db, b) = base();
    assert_eq!(pick(&db.m("Tú")), b.tu);
    assert_eq!(db.m("tu").status, "found");
    assert_eq!(pick(&db.m("Es El")), b.es_el);
    // un orden distinto de solo palabras vacías no es una pista fuerte
    assert!(matches!(db.m("El Es").status, "probable" | "missing"));
    // y una sola letra suelta no es nada
    assert_eq!(db.m("a").status, "missing");
    assert_eq!(db.m("x").status, "missing");
}

// --------------------------------------------------------------- la ñ

/// Mañana y Manana son dos canciones: cada una se encuentra por su forma, y
/// la otra solo aparece como alternativa de la segunda pasada, con descuento.
#[test]
fn enye_is_not_n_in_the_normal_form() {
    let (db, b) = base();
    let with_enye = db.m("Mañana");
    assert!(pick(&with_enye) == b.manana_n && with_enye.candidates[0].why == "titulo exacto");
    let other = with_enye
        .candidates
        .iter()
        .find(|c| id_of(&c.song) == b.manana)
        .expect("la otra");
    assert!(other.why == "enye~n" && other.score < with_enye.candidates[0].score);

    let without_enye = db.m("Manana");
    assert!(pick(&without_enye) == b.manana && without_enye.candidates[0].why == "titulo exacto");
    let other = without_enye
        .candidates
        .iter()
        .find(|c| id_of(&c.song) == b.manana_n)
        .expect("la otra");
    assert!(other.why == "enye~n" && other.score < without_enye.candidates[0].score);

    for variant in ["MAÑANA", "mañana", "*2.-Mañana* ✅"] {
        assert_eq!(pick(&db.m(variant)), b.manana_n, "{variant}");
    }
}

/// Quien escribe «Senor» sin ñ encuentra «Señor», marcado como de menor
/// confianza.
#[test]
fn the_second_pass_forgives_enye_at_a_discount() {
    let (db, b) = base();
    let r = db.m("Cena Del Senor");
    assert!(r.status == "found" && pick(&r) == b.cena);
    let first = &r.candidates[0];
    assert!(first.why == "enye~n" && (0.9..1.0).contains(&first.score));
    assert!(r.note.contains('ñ'), "{}", r.note);
    let exact = db.m("Cena Del Señor");
    assert_eq!(
        (exact.candidates[0].why, exact.candidates[0].score),
        ("titulo exacto", 1.0)
    );
    assert_eq!(exact.note, "");
    // también a la inversa: la ñ en lo pedido y la n en la biblioteca
    assert_eq!(db.m("Año Nuevo").candidates[0].why, "titulo exacto");
    let without = db.m("Ano Nuevo");
    assert!(pick(&without) == b.ano && without.candidates[0].why == "enye~n");
    // la ñ cuesta: con otra imperfección encima ya no es found
    assert_eq!(db.m("Ano Nuevoo").status, "probable");
}

/// La errata de una letra no puede colar la n por la ñ en la forma normal.
#[test]
fn enye_and_n_do_not_count_as_a_typo() {
    assert!(!near("mañana", "manana"));
    assert!(!near("senor", "señor"));
    assert!(near("hosanna", "hosnna") && near("hosanna", "hosanan"));
    assert!(near("hosanna", "hosannaa") && near("grita", "grit"));
    assert!(!near("rey", "rei"), "las palabras cortas no se tocan");
    assert!(!near("hosanna", "hosanna"));
}

// ------------------------------------------------------ apóstrofos

#[test]
fn apostrophes_in_titles_and_artists() {
    let (db, b) = base();
    for text in [
        "Don't Stop Believin'",
        "Dont Stop Believin",
        "Don’t Stop Believin’",
        "DONT STOP BELIEVIN",
    ] {
        let r = db.m(text);
        assert!(r.status == "found" && pick(&r) == b.dont, "{text}");
    }
    assert_eq!(
        parse_query("Lucero D'Alba", "", &[]).core,
        parse_query("Lucero DAlba", "", &[]).core
    );
    assert_eq!(
        title_key("Rey Infinito", "Lucero D'Alba"),
        title_key("Rey Infinito", "Lucero DAlba")
    );
}

#[test]
fn the_artist_apostrophe_with_a_gap_too() {
    let mut db = Lib::new();
    let sid = db.add_song(&Song {
        artist: "Christine D' Clario",
        title: "Fe",
        file: Some("Christine D'Clario - Fe.mp3"),
        ..Song::default()
    });
    let other = db.add("Otro Artista", "Fe");
    for shape in [
        "Christine D'Clario",
        "Christine DClario",
        "Christine D Clario",
        "Christine D’Clario",
    ] {
        let r = db.ma("Fe", shape);
        assert!(pick(&r) == sid && r.status == "found", "{shape}");
    }
    assert_eq!(pick(&db.ma("Fe", "Otro Artista")), other);
}

// ------------------------------------------------ erratas, partes, orden

#[test]
fn a_typo_is_probable_and_finds_the_song() {
    let (db, b) = base();
    for text in [
        "Hosznna En Las Alturas",  // una letra cambiada
        "Hosnna En Las Alturas",   // una de menos
        "Hosannna En Las Alturas", // una de más
        "Hosanan En Las Alturas",  // dos seguidas cambiadas de sitio
        "Hosanna En Las Altruas",
        "hosanna en las alturaz",
    ] {
        let r = db.m(text);
        assert!(
            r.status == "probable" && r.via == "difuso" && pick(&r) == b.hosanna,
            "{text}"
        );
        assert_eq!(r.candidates[0].why, "difuso");
        assert!((T_MISSING..T_FOUND).contains(&r.candidates[0].score));
        assert!(!r.note.is_empty());
    }
}

#[test]
fn a_typo_with_the_right_artist_is_found() {
    let (db, b) = base();
    let r = db.ma("Hosznna En Las Alturas", "Banda Horizonte");
    assert!(r.status == "found" && pick(&r) == b.hosanna);
}

/// La errata de una letra se tolera en palabras de 4 o más (una de 5 a la
/// que le falta una queda en 4). En una corta solo se ve al mirar el título
/// entero.
#[test]
fn a_typo_in_a_short_word_only_shows_when_joined() {
    let (db, _) = base();
    assert_eq!(
        db.m("Rei Infinito").status,
        "probable",
        "entero: «reiinfinito»"
    );
    assert_eq!(db.m("Gracia Sublim").status, "probable");
    assert_eq!(
        db.m("Sublimme").status,
        "missing",
        "una sola palabra suelta no basta"
    );
}

#[test]
fn the_start_of_a_title_is_probable() {
    let (db, b) = base();
    let r = db.m("Grande Y");
    assert!(r.status == "probable" && pick(&r) == b.big);
    assert!(r.candidates[0].why == "frase en titulo" && !r.note.is_empty());
    assert_eq!(pick(&db.m("Castillo Fuerte")), b.castillo);
    // pero una sola palabra, o el final, o el medio, no bastan
    assert_eq!(db.m("Grande").status, "missing");
    assert_eq!(db.m("Sublime").status, "missing");
    assert_eq!(db.m("Nuestro Dios").status, "missing");
}

#[test]
fn the_same_words_in_another_order_is_probable() {
    let (db, b) = base();
    for text in ["Gracia Sublime", "Alturas Las En Hosanna", "Infinito Rey"] {
        let r = db.m(text);
        assert!(
            r.status == "probable" && r.candidates[0].why == "palabras",
            "{text}"
        );
    }
    assert_eq!(pick(&db.m("Gracia Sublime")), b.sublime);
}

/// «ReyInfinito» o «CenaDelSeñor» son el mismo título con los espacios en
/// otro sitio.
#[test]
fn words_joined_or_split_differently() {
    let (db, b) = base();
    for text in ["ReyInfinito", "Rey Infini to", "CenaDelSeñor"] {
        let r = db.m(text);
        assert!(r.status == "found" && r.song.is_some(), "{text}");
    }
    assert_eq!(pick(&db.m("ReyInfinito")), b.rey);
    assert_eq!(pick(&db.m("CenaDelSeñor")), b.cena);
}

#[test]
fn a_subtitle_and_part_of_a_medley() {
    let (db, b) = base();
    let sub = db.m("Cae El Muro");
    assert!(sub.status == "probable" && pick(&sub) == b.sub);
    let full = db.m("Fuego Santo Cae El Muro");
    assert!(full.status == "found" && pick(&full) == b.sub);
    let medley = db.m("Toma Tu Lugar");
    assert_eq!(pick(&medley), b.dda_medley);
    assert!(
        (T_MISSING..1.0).contains(&medley.candidates[0].score),
        "una parte vale menos que un título"
    );
}

// --------------------------------------------------- lo que NO está

/// Lo que da el medidor: (id, puntuación, estado) de cada candidata, diez como
/// mucho, la primera con el estado del emparejador; vacío si dice que no está.
fn rank_of(db: &Lib, title: &str) -> Vec<(i64, f64, &'static str)> {
    let r = db.ask(&Ask {
        title,
        limit: 10,
        ..Ask::default()
    });
    if r.status == "missing" {
        return Vec::new();
    }
    r.candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (
                id_of(&c.song),
                c.score,
                if i == 0 { r.status } else { "alt" },
            )
        })
        .collect()
}

#[test]
fn what_is_missing_is_not_matched() {
    let (db, _) = base();
    for text in [
        // canciones públicas que no están
        "Oceans Where Feet May Fail",
        "Way Maker",
        "Rey de Reyes",
        "*5.-Océanos (Donde Mis Pies Pueden Fallar)* ✅🙏🏻",
        // casi: una palabra cambiada
        "Cuán Grande Es Él",
        "*2.- Cuán Grande Es Él* 🙏🏻",
        "Santo Es El Señor",
        "Digno Es El Cordero",
        "Castillo Debil Es Nuestro Dios",
        // casi: el título de otra con algo de más
        "Personal Jesus",
        "Sublime Gracia Maravillosa Eterna",
        // una palabra suelta del artista, del álbum, de un paréntesis
        "Altura",
        "Altura Worship",
        "Horizonte",
        "Aurora",
        "Heaven",
        // nada que ver
        "Despacito",
        "Bohemian Rhapsody",
        "Gloria Aleluya Santo",
    ] {
        let r = db.m(text);
        assert!(
            r.status == "missing" && r.song.is_none() && r.via.is_empty(),
            "{text:?} -> {}",
            r.status
        );
        assert!(rank_of(&db, text).is_empty(), "{text}");
    }
}

#[test]
fn what_is_missing_with_its_artist_is_missing_too() {
    let (db, _) = base();
    for (title, artist) in [
        ("Good Good Father", "Chris Tomlin"),
        ("Rey de Reyes", "Hillsong en Español"),
        ("Alabanza Eterna", "Altura Worship"),
        ("Cuán Grande Es Él", "Lucero D'Alba"),
    ] {
        assert_eq!(db.ma(title, artist).status, "missing", "{title} / {artist}");
    }
}

/// Quien pide «Rey Infinito Poderoso» no pide «Rey Infinito»: como mucho,
/// probable.
#[test]
fn an_extra_word_is_never_found() {
    let (db, _) = base();
    for text in [
        "Rey Infinito Poderoso",
        "Hosanna En Las Alturas Eterno",
        "Cena Del Señor Santa",
    ] {
        let r = db.m(text);
        assert!(
            matches!(r.status, "probable" | "missing"),
            "{text}: {}",
            r.status
        );
    }
}

/// Ningún título que se parece a uno de la biblioteca (una palabra
/// cambiada) se cuela como `found` ni `ambiguous`.
#[test]
fn a_changed_word_is_never_found() {
    let (db, _) = base();
    let index = db.index();
    for a in ["Rey", "Gloria", "Sublime", "Cena", "Santo", "Hosanna"] {
        for b in ["Eterno", "Poderoso", "Nuevo", "Perfecto"] {
            let text = format!("{a} {b}");
            let r = ask_in(
                &index,
                &Ask {
                    title: &text,
                    ..Ask::default()
                },
            );
            assert!(!found_or_ambiguous(&r), "{text}");
        }
    }
}

// ------------------------------------------------- enlaces e historial

const VIDEO: &str = "AbCdEfGhIjK";
const URL: &str = "https://www.youtube.com/watch?v=AbCdEfGhIjK";
const OTHER_URL: &str = "https://youtu.be/Q1w2E3r4T5y";

/// Lo que se apunta de una descarga.
#[derive(Clone)]
struct Download<'a> {
    url: &'a str,
    song_id: i64,
    target: Option<String>,
    ok: bool,
    already: bool,
    at: Option<f64>,
    title: &'a str,
}

fn download(url: &str, song_id: i64) -> Download<'_> {
    Download {
        url,
        song_id,
        target: None,
        ok: true,
        already: false,
        at: None,
        title: "",
    }
}

fn record_download(db: &Lib, d: &Download<'_>) {
    let target = d.target.clone().unwrap_or_else(|| db.path_of(d.song_id));
    let entry = json!({
        "at": d.at,
        "source": "assistant",
        "query": d.url,
        "title": d.title,
        "url": d.url,
        "ok": d.ok,
        "already": d.already,
        "song_id": d.song_id,
        "target": target,
    });
    let Value::Object(entry) = entry else {
        unreachable!()
    };
    history::log_download(&db.conn, &entry).expect("el historial");
    // (la transacción la cierra quien llama)
    if !db.conn.is_autocommit() {
        db.sql("COMMIT");
    }
}

#[test]
fn an_already_downloaded_link_beats_the_title() {
    let (db, b) = base();
    record_download(&db, &download(URL, b.dda_acu));
    let r = db.ask(&Ask {
        title: "cualquier cosa que no se parece",
        url: URL,
        ..Ask::default()
    });
    assert_eq!(
        (r.status, r.via, r.other_version),
        ("found", "enlace", false)
    );
    assert_eq!(pick(&r), b.dda_acu);
    let whys: Vec<(&str, f64)> = r.candidates.iter().map(|c| (c.why, c.score)).collect();
    assert_eq!(whys, [("enlace", 1.0)]);
    // aunque el título apunte a otra versión del mismo nombre
    let other = db.ask(&Ask {
        title: "Digno De Adorar",
        artist: "Banda Horizonte",
        url: URL,
        ..Ask::default()
    });
    assert_eq!((other.via, pick(&other)), ("enlace", b.dda_acu));
    // y sin título
    let without = db.ask(&Ask {
        url: URL,
        ..Ask::default()
    });
    assert_eq!((without.via, pick(&without)), ("enlace", b.dda_acu));
}

#[test]
fn the_link_is_the_same_video_however_it_is_written() {
    let (db, b) = base();
    record_download(&db, &download(URL, b.rey));
    for url in [
        format!("https://www.youtube.com/watch?v={VIDEO}&list=PLxxxxxxxx&t=30s&feature=share"),
        format!("https://youtu.be/{VIDEO}?feature=share"),
        format!("youtu.be/{VIDEO}"),
        format!("https://music.youtube.com/watch?v={VIDEO}"),
        format!("https://m.youtube.com/watch?v={VIDEO}"),
        format!("https://www.youtube.com/shorts/{VIDEO}"),
    ] {
        assert_eq!(links::video_id(&url), VIDEO, "{url}");
        let r = db.ask(&Ask {
            url: &url,
            ..Ask::default()
        });
        assert_eq!((r.via, pick(&r)), ("enlace", b.rey), "{url}");
    }
}

#[test]
fn what_is_not_a_youtube_video_is_not_a_link() {
    let (db, b) = base();
    record_download(&db, &download(URL, b.rey));
    for url in [
        format!("https://youtube.com.otro.example/watch?v={VIDEO}"),
        format!("https://youtube.com@otro.example/watch?v={VIDEO}"),
        format!("https://otro.example/?u=https://youtube.com/watch?v={VIDEO}"),
        format!("http://127.0.0.1/watch?v={VIDEO}"),
        format!("javascript:alert('youtube.com/watch?v={VIDEO}')"),
        format!("file:///youtube.com/watch?v={VIDEO}"),
        format!("https://www.youtube.com/playlist?list={VIDEO}"),
        "https://www.youtube.com/watch?v=corto".to_owned(),
        "https://www.youtube.com/".to_owned(),
        String::new(),
    ] {
        assert_eq!(links::video_id(&url), "", "{url}");
        let r = db.ask(&Ask {
            title: "Cena Del Señor",
            url: &url,
            ..Ask::default()
        });
        assert!(
            r.via != "enlace" && pick(&r) == b.cena && !r.other_version,
            "{url}"
        );
    }
}

/// Los ids de canción se reutilizan al borrar la última fila: la fila del
/// historial solo vale si el archivo de esa canción sigue siendo el que se
/// bajó.
#[test]
fn a_reused_id_does_not_count() {
    let (mut db, _) = base();
    let last = db.add("Banda Horizonte", "Cancion Temporal");
    record_download(&db, &download(URL, last));
    db.remove(last); // sale de la biblioteca y el id queda libre…
    let reused = db.add_song(&Song {
        artist: "Otro Artista",
        title: "Otro Titulo Distinto",
        sid: Some(last),
        ..Song::default()
    }); // …y otra canción lo toma
    assert_eq!(reused, last);
    let r = db.ask(&Ask {
        title: "Otro Titulo Distinto",
        url: URL,
        ..Ask::default()
    });
    assert_ne!(r.via, "enlace", "el historial apuntaba a otra canción");
    assert!(
        pick(&r) == reused && r.other_version,
        "se encuentra por el título; el enlace queda por bajar"
    );
    // y sin título el enlace no lleva a nada
    let without = db.ask(&Ask {
        url: URL,
        ..Ask::default()
    });
    assert!(without.status == "missing" && without.via.is_empty());
}

/// El caso real: un enlace con tres filas; la más nueva apunta a una canción
/// que ya no es esa (id reutilizado) y la anterior sigue valiendo.
#[test]
fn history_is_walked_from_newest_to_oldest() {
    let (db, b) = base();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("reloj")
        .as_secs_f64();
    let at = |d: Download<'static>, t: f64| Download { at: Some(t), ..d };
    record_download(&db, &at(download(URL, b.dda_vivo), now - 300.0)); // vale
    record_download(&db, &at(download(URL, b.dda_vivo), now - 200.0)); // vale (repetida)
    record_download(
        &db,
        &Download {
            target: Some("/musica/Artistas/Otro/Otro Archivo.mp3".into()),
            ..at(download(URL, b.rey), now - 100.0)
        },
    ); // no vale
    let r = db.ask(&Ask {
        url: URL,
        ..Ask::default()
    });
    assert_eq!((r.via, pick(&r)), ("enlace", b.dda_vivo));
    // si la más nueva SÍ vale, gana ella
    record_download(&db, &at(download(URL, b.sublime), now));
    let r = db.ask(&Ask {
        url: URL,
        ..Ask::default()
    });
    assert_eq!(pick(&r), b.sublime);
    // solo valen las que salieron bien (o las que ya estaban)
    let failed_url = "https://youtu.be/ZyXwVuTsRqP";
    record_download(
        &db,
        &Download {
            ok: false,
            ..download(failed_url, b.rey)
        },
    );
    let r = db.ask(&Ask {
        url: failed_url,
        ..Ask::default()
    });
    assert_ne!(r.via, "enlace");
}

/// Si se movió la biblioteca de sitio, el archivo sigue llamándose igual.
#[test]
fn the_same_file_name_counts_even_if_the_folder_moved() {
    let (db, b) = base();
    let path = db.path_of(b.cena);
    let name = path.rsplit('/').next().expect("nombre");
    record_download(
        &db,
        &Download {
            target: Some(format!(
                "C:\\Users\\alguien\\Musica\\Artistas\\Coro Aurora\\{name}"
            )),
            ..download(URL, b.cena)
        },
    );
    let r = db.ask(&Ask {
        url: URL,
        ..Ask::default()
    });
    assert_eq!(r.via, "enlace");
}

/// Una descarga «ya la tenías» no deja archivo nuevo: se comprueba el nombre.
#[test]
fn already_there_is_checked_by_the_name_key() {
    let (db, b) = base();
    db.conn
        .execute(
            "UPDATE songs SET match_key=? WHERE id=?",
            rusqlite::params![names::match_key(&names::clean("Cena Del Señor")), b.cena],
        )
        .expect("clave");
    record_download(
        &db,
        &Download {
            target: Some(String::new()),
            ok: false,
            already: true,
            title: "CENA DEL SEÑOR (Video Oficial)",
            ..download(URL, b.cena)
        },
    );
    let r = db.ask(&Ask {
        url: URL,
        ..Ask::default()
    });
    assert_eq!((r.via, pick(&r)), ("enlace", b.cena));
    // con otro nombre de vídeo, esa fila no demuestra nada
    let other = "https://youtu.be/QwErTyUiOpA";
    record_download(
        &db,
        &Download {
            target: Some(String::new()),
            ok: false,
            already: true,
            title: "Algo Que No Es",
            ..download(other, b.cena)
        },
    );
    let r = db.ask(&Ask {
        url: other,
        ..Ask::default()
    });
    assert_ne!(r.via, "enlace");
}

/// El enlace no está en el historial pero la biblioteca tiene el título: se
/// usa la de la biblioteca (`other_version`) o, con `strict`, no se sustituye.
#[test]
fn other_version_and_strict() {
    let (db, b) = base();
    let cena = |strict: bool, url: &str| {
        db.ask(&Ask {
            title: "Cena Del Señor",
            url,
            strict,
            ..Ask::default()
        })
    };
    let r = cena(false, OTHER_URL);
    assert_eq!(
        (r.status, pick(&r), r.other_version, r.via),
        ("found", b.cena, true, "titulo")
    );
    assert!(r.note.contains("historial"), "{}", r.note);
    let strict_match = cena(true, OTHER_URL);
    assert_eq!(
        (
            strict_match.status,
            strict_match.song.is_none(),
            strict_match.other_version
        ),
        ("missing", true, true)
    );
    assert_eq!(
        id_of(&strict_match.candidates[0].song),
        b.cena,
        "la otra versión sigue a la vista"
    );
    // el enlace de verdad ya bajado no es «otra versión» ni siquiera en modo estricto
    record_download(&db, &download(OTHER_URL, b.cena));
    let ok = cena(true, OTHER_URL);
    assert_eq!(
        (ok.status, ok.via, ok.other_version),
        ("found", "enlace", false)
    );
    // sin enlace no hay otra versión
    assert!(!cena(true, "").other_version);
    // lo que no está sigue sin estar, con o sin enlace
    let way = db.ask(&Ask {
        title: "Way Maker",
        url: OTHER_URL,
        ..Ask::default()
    });
    assert!(!way.other_version);
}

#[test]
fn the_video_title() {
    let (db, b) = base();
    let video = |title: &str, video_title: &str, strict: bool| {
        db.ask(&Ask {
            title,
            url: OTHER_URL,
            video_title,
            strict,
            ..Ask::default()
        })
    };
    let r = video(
        "",
        "Banda Horizonte - Cena Del Señor (Video Oficial) HD",
        false,
    );
    assert!(r.status == "found" && pick(&r) == b.cena && r.other_version);
    // el canal o el artista del vídeo pueden ser otros: solo suman
    let r = video("", "Alguien Que No Es - Rey Infinito (Letra)", false);
    assert_eq!(pick(&r), b.rey);
    // si el título escrito no encuentra nada, se prueba con el del vídeo
    let r = video("Lo Que Dijo La Lista", "Rey Infinito", false);
    assert_eq!(pick(&r), b.rey);
    // sin título ni vídeo no hay nada que mirar
    let empty = video("", "", false);
    assert!(empty.status == "missing" && !empty.note.is_empty());
    // y con strict no se sustituye
    assert_eq!(video("", "Rey Infinito", true).status, "missing");
}

#[test]
fn a_link_pasted_in_the_title_is_not_the_title() {
    let (db, b) = base();
    record_download(&db, &download(URL, b.sublime));
    let r = db.m(&format!("*3.-Sublime Gracia* {URL} @Ana Lopez"));
    assert_eq!((r.via, pick(&r)), ("enlace", b.sublime));
    let new = db.m("*3.-Rey Infinito* https://youtu.be/Q1w2E3r4T5y?feature=share");
    assert!(pick(&new) == b.rey && new.other_version);
}

// ------------------------------------------------ elegir y desempatar

/// «Digno De Adorar» tiene tres versiones: la que se eligió la semana
/// pasada (mismo `title_key`) gana a igualdad, y deja de dar la duda de
/// siempre.
#[test]
fn the_remembered_choice_wins_ties() {
    let (db, b) = base();
    let remembered = (title_key("*1.- Digno De Adorar* @Ana Lopez", ""), b.dda_acu);
    let key = title_key("digno de adorar (En Su Presencia)", "");
    assert_eq!(key, title_key("DIGNO DE ADORAR", ""));
    assert_eq!(key, "digno de adorar");
    assert_eq!(remembered.0, key);
    let prefer = |ids: Vec<i64>, title: &str| {
        db.ask(&Ask {
            title,
            prefer: ids,
            ..Ask::default()
        })
    };
    let r = prefer(vec![remembered.1], "Digno De Adorar");
    assert_eq!(
        r.status, "ambiguous",
        "sigue habiendo tres; solo cambia la elegida"
    );
    assert!(pick(&r) == b.dda_acu && song_ids(&r)[0] == b.dda_acu);
    // una elección que no está entre las empatadas no cambia nada
    assert_eq!(pick(&prefer(vec![b.rey], "Digno De Adorar")), b.dda);
    assert_eq!(pick(&prefer(vec![99999], "Digno De Adorar")), b.dda);
    // y lo que pide la persona vale más que lo recordado
    let live = prefer(vec![b.dda_acu], "Digno De Adorar (En Vivo)");
    assert_eq!(pick(&live), b.dda_vivo);
}

#[test]
fn title_key_is_stable_and_tells_apart() {
    let same_titles = [
        "Digno De Adorar",
        "*1.- Digno De Adorar* ✅🙏🏻",
        "digno de adorar @Ana Lopez @Beto Ruiz (En Su Presencia) Voz",
        "DIGNO  DE ADORAR  [HD]",
        "Digno De Adorar https://youtu.be/AbCdEfGhIjK",
    ];
    let keys: HashSet<String> = same_titles.iter().map(|t| title_key(t, "")).collect();
    assert_eq!(keys.len(), 1, "{keys:?}");
    assert_ne!(
        title_key("Digno De Adorar", ""),
        title_key("Digno De Adorar Ya", "")
    );
    assert_ne!(
        title_key("Mañana", ""),
        title_key("Manana", ""),
        "la ñ se conserva, como en toda comparación"
    );
    let with_artist = title_key("Digno De Adorar", "Banda Horizonte");
    assert_eq!(
        with_artist,
        title_key("*1.-Digno De Adorar*", "banda  horizonte")
    );
    assert_ne!(with_artist, title_key("Digno De Adorar", ""));
    // sin artista ni título, la clave sale vacía
    assert_eq!(title_key("", ""), "");
}

#[test]
fn tiebreaks_choice_favorite_stars_and_short_title() {
    let mut db = Lib::new();
    let length = db.add("Artista Uno", "Canto Nuevo Largo (En Vivo)");
    let short = db.add_song(&Song {
        artist: "Artista Dos",
        title: "Canto Nuevo",
        stars: 2,
        ..Song::default()
    });
    let four = db.add_song(&Song {
        artist: "Artista Tres",
        title: "Canto Nuevo (Acustico)",
        stars: 4,
        ..Song::default()
    });
    let favorite_song = db.add_song(&Song {
        artist: "Artista Cuatro",
        title: "Canto Nuevo (Pista)",
        favorite: 1,
        ..Song::default()
    });
    assert_eq!(
        pick(&db.m("Canto Nuevo")),
        favorite_song,
        "la favorita gana a las estrellas"
    );
    db.sql("UPDATE songs SET favorite=0");
    assert_eq!(pick(&db.m("Canto Nuevo")), four, "más estrellas");
    db.sql("UPDATE songs SET stars=0");
    assert_eq!(pick(&db.m("Canto Nuevo")), short, "el título más corto");
    let prefer = |ids: Vec<i64>| {
        pick(&db.ask(&Ask {
            title: "Canto Nuevo",
            prefer: ids,
            ..Ask::default()
        }))
    };
    assert_eq!(prefer(vec![four]), four, "la recordada, la primera");
    // el que solo empieza igual no es un empate: no se elige por más
    // estrellas ni por recordado
    assert_eq!(prefer(vec![length]), short);
    let all = db.ask(&Ask {
        title: "Canto Nuevo",
        limit: 10,
        ..Ask::default()
    });
    let got: HashSet<i64> = song_ids(&all).into_iter().collect();
    assert_eq!(got, HashSet::from([length, short, four, favorite_song]));
}

#[test]
fn rank_is_the_measure_search_adapter() {
    let (db, b) = base();
    assert_eq!(rank_of(&db, "Rey Infinito"), [(b.rey, 1.0, "found")]);
    let amb = rank_of(&db, "Digno De Adorar");
    assert_eq!(amb[0].2, "ambiguous");
    assert!(amb[1..].iter().all(|r| r.2 == "alt"));
    assert!(amb[0].0 == b.dda && amb.len() >= 3);
    assert!(amb.iter().all(|r| 0.0 < r.1 && r.1 <= 1.0));
    assert!(rank_of(&db, "Despacito").is_empty() && rank_of(&db, "").is_empty());
    assert_eq!(rank_of(&db, "Hosznna En Las Alturas")[0].2, "probable");
}

#[test]
fn limit_and_the_copies_returned() {
    let (db, _) = base();
    let index = db.index();
    let with_limit = |limit: usize| {
        ask_in(
            &index,
            &Ask {
                title: "Digno De Adorar",
                limit,
                ..Ask::default()
            },
        )
    };
    assert_eq!(with_limit(2).candidates.len(), 2);
    assert_eq!(with_limit(1).candidates.len(), 1);
    // lo que se devuelve es una copia: cambiarlo no cambia el índice
    let rey = Ask {
        title: "Rey Infinito",
        ..Ask::default()
    };
    let mut r = ask_in(&index, &rey);
    if let Some(song) = r.song.as_mut() {
        song.insert("title".into(), json!("MUTADO"));
    }
    r.candidates[0]
        .song
        .insert("artist".into(), json!("MUTADO"));
    let other = ask_in(&index, &rey);
    assert_eq!(chosen(&other)["title"], json!("Rey Infinito"));
    assert_eq!(chosen(&other)["artist"], json!("Lucero D'Alba"));
}

#[test]
fn the_song_data_comes_from_the_db() {
    let mut db = Lib::new();
    let sid = db.add_song(&Song {
        artist: "Alguien",
        title: "Una Cancion",
        album: "Un Disco",
        stars: 3,
        favorite: 1,
        ..Song::default()
    });
    db.sql(&format!(
        "UPDATE songs SET key='Bb', bpm=87.4, duration=213.6 WHERE id={sid}"
    ));
    let r = db.m("Una Cancion");
    assert_eq!(
        Value::Object(chosen(&r).clone()),
        json!({
            "id": sid,
            "artist": "Alguien",
            "title": "Una Cancion",
            "album": "Un Disco",
            "duration": 214,
            "key": "Bb",
            "bpm": 87,
            "stars": 3,
            "favorite": true,
        })
    );
}

// ------------------------------------------------------ entradas hostiles

fn hostiles() -> Vec<(&'static str, String)> {
    vec![
        ("200 KB de palabras", "gloria dios señor ".repeat(12_000)),
        ("200 KB de asteriscos", "*".repeat(200_000)),
        ("200 KB de paréntesis sin cerrar", "(".repeat(200_000)),
        (
            "200 KB de corchetes",
            "[".repeat(200_000) + &"x".repeat(1000),
        ),
        ("200 KB de (a)", "(a)".repeat(70_000)),
        (
            "paréntesis anidados",
            "(".repeat(5000) + "Rey Infinito" + &")".repeat(5000),
        ),
        ("50 000 arrobas", "@".repeat(50_000)),
        ("enlaces repetidos", "https://youtu.be/".repeat(12_000)),
        ("20 000 líneas de numeración", "1.- Titulo\n".repeat(20_000)),
        (
            "solo caracteres raros",
            "★☆✿❀♪♫🙏🏻🎶".repeat(50) + "\u{202e}\u{200b}\u{0}\u{7}",
        ),
        ("una letra", "x".into()),
        ("un punto", ".".into()),
        ("vacío", String::new()),
        ("solo espacios", " ".repeat(100_000)),
        ("una palabra gigante", "a".repeat(200_000)),
        ("Tú", "Tú".into()),
        ("Él", "Él".into()),
        (
            "sintaxis FTS con comillas",
            "\"gloria\" OR NEAR(dios senor, 3) AND * ( ) :: title:gloria -x ^y \"".into(),
        ),
        ("sintaxis FTS con asterisco", "gloria* OR dios*".into()),
        ("palabras reservadas de FTS", "OR AND NOT NEAR".into()),
        ("comillas sueltas", "\"".repeat(2000)),
        ("apóstrofes sueltos", "'".repeat(2000)),
        (
            "dos puntos de filtro",
            "artista:barak tono:Bb bpm>100 title:x".into(),
        ),
        ("NUL y controles", "gloria\u{0}dios\u{7}\u{1b}[31m".into()),
        ("texto bidi", "\u{202e}gloria\u{202c} dios".into()),
    ]
}

/// Los mismos casos con el índice entero en memoria y con el prefiltro FTS5.
fn modes() -> Vec<(&'static str, Lib)> {
    let (memory, _) = base();
    assert!(!memory.index().large);
    let (mut fts, _) = base();
    fts.full_limit = 3;
    assert!(fts.index().large);
    vec![("en memoria", memory), ("con prefiltro FTS5", fts)]
}

/// Los `n` primeros caracteres.
fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[test]
fn hostile_inputs_within_a_time_limit() {
    for (mode, db) in modes() {
        let index = Arc::new(db.index());
        for (name, text) in hostiles() {
            let index = index.clone();
            let r = within(5, move || {
                ask_in(
                    &index,
                    &Ask {
                        title: &text,
                        artist: &head(&text, 40),
                        hints: vec![text.clone()],
                        url: &head(&text, 300),
                        ..Ask::default()
                    },
                )
            });
            assert!(
                matches!(r.status, "found" | "ambiguous" | "probable" | "missing"),
                "{mode}, {name}"
            );
            assert_eq!(r.song.is_none(), r.status == "missing", "{mode}, {name}");
        }
    }
}

/// Un lote entero de entradas hostiles no falla en ninguna: ninguna consulta da
/// error.
#[test]
fn a_whole_hostile_batch_does_not_fail() {
    for (mode, db) in modes() {
        let index = Arc::new(db.index());
        let done = within(30, move || {
            hostiles()
                .into_iter()
                .map(|(name, t)| {
                    let hints = vec![t.clone()];
                    let r = index.matching(
                        &t,
                        &head(&t, 30),
                        &hints,
                        &head(&t, 300),
                        "",
                        &HashSet::new(),
                        5,
                        false,
                    );
                    (name, r.is_ok())
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(done.len(), hostiles().len());
        for (name, ok) in done {
            assert!(ok, "{mode}, {name}");
        }
    }
}

/// Comillas, paréntesis, OR, NEAR y * dentro del título son texto: no se
/// interpretan.
#[test]
fn fts_syntax_does_not_change_the_search() {
    for (mode, db) in modes() {
        let index = Arc::new(db.index());
        for text in [
            "\"Rey Infinito\"",
            "Rey* OR Cena*",
            "NEAR(rey infinito, 2)",
            "title:rey",
            "Rey AND (Infinito OR Cena)",
        ] {
            let index = index.clone();
            let r = within(5, move || {
                ask_in(
                    &index,
                    &Ask {
                        title: text,
                        ..Ask::default()
                    },
                )
            });
            assert!(
                matches!(r.status, "found" | "ambiguous" | "probable" | "missing"),
                "{mode}, {text}"
            );
        }
        assert_eq!(db.m("\"Rey Infinito\"").status, "found", "{mode}");
        assert_ne!(db.m("Rey* OR Cena*").status, "found", "{mode}");
    }
}

#[test]
fn a_title_of_only_stop_words_and_one_letter() {
    for (mode, db) in modes() {
        let r = db.m("Tú");
        assert!(
            r.status == "found" && chosen(&r)["title"] == json!("Tú"),
            "{mode}"
        );
        for text in ["Él", "y", "x"] {
            assert_eq!(db.m(text).status, "missing", "{mode}, {text}");
        }
    }
}

/// Cualquier texto da una respuesta válida (300 textos de un generador fijo,
/// con lo que suele romper).
#[test]
fn any_text_gives_a_valid_answer() {
    let (db, _) = base();
    let index = db.index();
    let pieces = [
        "Rey",
        "Infinito",
        " ",
        "  ",
        "*",
        "(",
        ")",
        "[",
        "]",
        "@",
        "#",
        "-",
        ".",
        "1",
        "ñ",
        "Ñ",
        "á",
        "\u{301}",
        "\u{0}",
        "\u{7}",
        "\n",
        "\t",
        "'",
        "\"",
        "’",
        "🙏🏻",
        "✅",
        "\u{202e}",
        "\u{200b}",
        "https://youtu.be/",
        "AbCdEfGhIjK",
        "?v=",
        "OR",
        "NEAR",
        ":",
        "Digno",
        "De",
        "Adorar",
        "Tú",
        "a",
        "x",
        "ß",
        "İ",
        "ﬁ",
        "\u{fffd}",
        "日本",
        "\u{10ffff}",
    ];
    let mut state = 0x1234_5678_9abc_def1_u64;
    let mut next = |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % n as u64) as usize
    };
    for _ in 0..300 {
        let title: String = (0..next(40)).map(|_| pieces[next(pieces.len())]).collect();
        let artist: String = (0..next(6)).map(|_| pieces[next(pieces.len())]).collect();
        let tail: String = {
            let c: Vec<char> = title.chars().collect();
            c[c.len().saturating_sub(30)..].iter().collect()
        };
        let r = ask_in(
            &index,
            &Ask {
                title: &title,
                artist: &artist,
                hints: vec![head(&title, 20)],
                url: &tail,
                ..Ask::default()
            },
        );
        assert!(
            matches!(r.status, "found" | "ambiguous" | "probable" | "missing"),
            "{title:?}"
        );
        assert_eq!(r.song.is_none(), r.status == "missing", "{title:?}");
        assert!(
            r.candidates.iter().all(|c| (0.0..=1.0).contains(&c.score)),
            "{title:?}"
        );
        if r.status != "missing" {
            assert_eq!(
                r.candidates.first().map(|c| &c.song),
                r.song.as_ref(),
                "{title:?}"
            );
        }
    }
}

// ----------------------------------------- el prefiltro FTS5 y el índice

#[test]
fn the_fts_prefilter_matches_the_in_memory_index() {
    let (mut db, _) = base();
    let queries = [
        ("Rey Infinito", ""),
        (
            "*3.-Rey Infinito* ✅🙏🏻 @Ana Lopez Voz (En Su Presencia)",
            "",
        ),
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
    ];
    let summary = |db: &Lib| -> Vec<(&'static str, Option<Dict>, Vec<i64>)> {
        let index = db.index();
        queries
            .iter()
            .map(|(t, a)| {
                let r = ask_in(
                    &index,
                    &Ask {
                        title: t,
                        artist: a,
                        ..Ask::default()
                    },
                );
                (r.status, r.song.clone(), song_ids(&r))
            })
            .collect()
    };
    let in_memory = summary(&db);
    db.full_limit = 3;
    assert!(db.index().large);
    let with_fts = summary(&db);
    for ((t, a), (mem, fts)) in queries.iter().zip(in_memory.iter().zip(&with_fts)) {
        assert_eq!(mem, fts, "{t} / {a}");
    }
}

/// Una palabra mal escrita no existe en el índice FTS5: la rescatan sus tres
/// primeros caracteres (solo si nada convenció al primer intento).
#[test]
fn the_prefilter_finds_a_typo_with_the_tolerant_retry() {
    let (mut db, b) = base();
    db.full_limit = 3;
    assert!(db.index().large);
    for text in ["Sublmie Gracai", "Sublime Gracai"] {
        let r = db.m(text);
        assert!(r.status == "probable" && pick(&r) == b.sublime, "{text}");
    }
}

// --------------------------------------------------------------- escala

/// Un generador fijo.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn choice<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// El emparejador con 50 000 canciones: el prefiltro FTS5 y la puntuación
/// solo sobre los 400 mejores. Presupuesto del diseño: menos de 300 ms por
/// item (sin optimizar, diez veces más).
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "la biblioteca, lo pedido y lo medido, en una sola prueba"
)]
fn songs_50000_within_a_time_limit() {
    let db = Lib::new();
    let mut rng = Rng(11);
    let syllables: Vec<String> = "bcdfglmnprstvz"
        .chars()
        .flat_map(|a| "aeiou".chars().map(move |b| format!("{a}{b}")))
        .collect();
    let word = |rng: &mut Rng| -> String {
        (0..rng.range(2, 4))
            .map(|_| rng.choice(&syllables).clone())
            .collect()
    };
    let vocab: Vec<String> = (0..5000).map(|_| word(&mut rng)).collect();
    let common_words = [
        "gloria", "dios", "señor", "santo", "rey", "vida", "amor", "luz", "fuego", "cielo",
    ];
    let stop_words = ["de", "la", "el", "en", "y", "que", "tu", "mi", "su", "con"];
    let capitalize = |w: &str| {
        let mut c = w.chars();
        c.next().map_or_else(String::new, |f| {
            f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()
        })
    };
    let artists: Vec<String> = (0..800)
        .map(|_| {
            let a = word(&mut rng);
            let b = word(&mut rng);
            format!("{} {}", capitalize(&a), capitalize(&b))
        })
        .collect();
    let title = |rng: &mut Rng| -> String {
        let mut ws: Vec<String> = Vec::new();
        for i in 0..rng.range(1, 5) {
            let r = rng.unit();
            if r < 0.25 {
                ws.push((*rng.choice(&common_words)).to_owned());
            } else if r < 0.45 && i > 0 {
                ws.push((*rng.choice(&stop_words)).to_owned());
            } else {
                ws.push(rng.choice(&vocab).clone());
            }
        }
        ws.iter()
            .map(|w| capitalize(w))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let planted = [
        ("Zafiro Del Amanecer", "Coro Zafiro"),
        ("Quimera Eterna", "Banda Quimera"),
        ("Mañana Sera Otro Dia", "Los Del Alba"),
        ("Pandora Y El Fuego Santo", "Trio Pandora"),
        ("Nebulosa", "Astro Coro"),
    ];
    let t0 = Instant::now();
    db.sql("BEGIN");
    {
        let mut insert = db.conn.prepare(INSERT_SONG).expect("insertar");
        let mut row = |i: i64, artist: &str, t: &str| {
            let file = format!("{artist} - {t}.mp3");
            insert
                .execute(rusqlite::params![
                    i,
                    format!("/n/{i}/{file}"),
                    "/n",
                    format!("Artistas/{artist}"),
                    file,
                    artist,
                    t,
                    "",
                    "",
                    "",
                    200.0,
                    0,
                    0
                ])
                .expect("fila");
        };
        for i in 1..=50_000 {
            let artist = rng.choice(&artists).clone();
            let t = title(&mut rng);
            row(i, &artist, &t);
        }
        for (k, (t, a)) in planted.iter().enumerate() {
            row(50_001 + k as i64, a, t);
        }
    }
    db.sql("COMMIT");
    let index = db.index();
    assert!(index.large && index.count == 50_005);
    assert!(
        t0.elapsed().as_secs_f64() < time_limit(60.0),
        "montar la base y el índice"
    );

    let requested = [
        (
            "*1.- Zafiro Del Amanecer* ✅🙏🏻 @Ana Lopez Voz (En Su Presencia)",
            "found",
        ),
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
    ];
    let items: Vec<(&str, &str)> = requested.iter().copied().cycle().take(56).collect();
    let t0 = Instant::now();
    let out: Vec<Resolution> = items
        .iter()
        .map(|(t, _)| {
            ask_in(
                &index,
                &Ask {
                    title: t,
                    ..Ask::default()
                },
            )
        })
        .collect();
    let per_item = t0.elapsed().as_secs_f64() / items.len() as f64;
    assert!(
        per_item < time_limit(0.3),
        "{:.0} ms por item",
        per_item * 1000.0
    );
    for ((text, state), r) in items.iter().zip(&out) {
        assert_eq!(r.status, *state, "{text}");
        if *state != "missing" {
            assert!(pick(r) > 50_000, "{text}: una de las plantadas");
        }
    }
    // ninguno pasa de un segundo y medio, ni siquiera los hostiles
    let hostile = hostiles();
    for name in [
        "200 KB de palabras",
        "sintaxis FTS con comillas",
        "Tú",
        "una palabra gigante",
    ] {
        let text = &hostile.iter().find(|(n, _)| *n == name).expect("caso").1;
        let t0 = Instant::now();
        ask_in(
            &index,
            &Ask {
                title: text,
                ..Ask::default()
            },
        );
        assert!(t0.elapsed().as_secs_f64() < time_limit(1.5), "{name}");
    }
}
