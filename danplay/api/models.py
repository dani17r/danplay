"""Los cuerpos de las peticiones: lo que se espera y nada mas."""

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field


class Body_(BaseModel):
    """Base de todos los cuerpos: lo que no se espera, se rechaza.

    Antes se pasaba el diccionario entero a la funcion del nucleo, asi que
    una clave de mas era un 500 y, en el caso de editar, una forma de tocar
    campos que solo debe cambiar el propio programa.
    """

    model_config = ConfigDict(extra="forbid")


class SongEdit(Body_):
    """Lo unico que se puede editar a mano de una cancion."""

    artist: str | None = Field(default=None, max_length=300)
    title: str | None = Field(default=None, max_length=300)
    album: str | None = Field(default=None, max_length=300)
    year: str | None = Field(default=None, max_length=10)
    genre: str | None = Field(default=None, max_length=120)
    key: str | None = Field(default=None, max_length=12)
    bpm: float | None = Field(default=None, ge=0, le=400)
    lyrics: str | None = Field(default=None, max_length=200_000)


EDITABLE = tuple(SongEdit.model_fields)


class Stars(Body_):
    stars: int = Field(ge=0, le=5)


class Favorite(Body_):
    favorite: bool = True


class Blur(Body_):
    blur: bool = True


class FolderIn(Body_):
    path: str = Field(min_length=1, max_length=4096)
    label: str = Field(default="", max_length=120)
    force: bool = False


class PathIn(Body_):
    path: str = Field(min_length=1, max_length=4096)


class RelocateIn(Body_):
    """Una carpeta gestionada que se movio (`from`) y donde esta ahora (`to`)."""

    model_config = ConfigDict(extra="forbid", populate_by_name=True)
    old: str = Field(alias="from", min_length=1, max_length=4096)
    new: str = Field(alias="to", min_length=1, max_length=4096)


class IdsIn(Body_):
    ids: list[int] = Field(default_factory=list, max_length=20000)


class PlaylistName(Body_):
    name: str = Field(min_length=1, max_length=120)


class ExclusionIn(Body_):
    pattern: str = Field(min_length=1, max_length=512)
    kind: Literal["glob", "path"] = "glob"
    note: str = Field(default="", max_length=300)


class ConvertIn(Body_):
    quality: Literal["high", "medium", "variable"] | None = None
    keep: bool = False
    dry_run: bool = True


class ResolveIn(Body_):
    keep: str = Field(min_length=1, max_length=4096)
    remove: list[str] = Field(default_factory=list, max_length=200)
    # Borrar es lo excepcional: hay que pedirlo. Antes bastaba con no decir
    # nada y se borraba de verdad.
    dry_run: bool = True


class ImportIn(Body_):
    dry_run: bool = False
    convert: bool | None = None


class EnrichIn(Body_):
    lyrics: bool = True
    cover: bool = True
    details: bool = True


class TransposeIn(Body_):
    text: str = Field(default="", max_length=100_000)
    from_key: str = Field(default="", max_length=12)
    to_key: str = Field(default="", max_length=12)
    semitones: int = Field(default=0, ge=-24, le=24)


class PlaylistIn(Body_):
    name: str = Field(default="Nueva lista", min_length=1, max_length=200)
    note: str = Field(default="", max_length=500)
    color: str = Field(default="", max_length=32)


class PlaylistEdit(Body_):
    """Lo que se puede cambiar de una lista ya creada."""

    name: str | None = Field(default=None, min_length=1, max_length=200)
    note: str | None = Field(default=None, max_length=500)


class SongsIn(Body_):
    ids: list[int] = Field(default_factory=list, max_length=5000)
    id: int | None = None


class OrderIn(Body_):
    ids: list[int] = Field(default_factory=list, max_length=5000)


class SettingsIn(Body_):
    convert_mp3: bool | None = None
    keep_original: bool | None = None
    write_tags: bool | None = None
    ai_enabled: bool | None = None
    quality: Literal["high", "medium", "variable"] | None = None
    # `model` y `ai_key` van al perfil de IA activo (compatibilidad con la
    # interfaz de antes); lo demas de la IA entra por /api/ai/*
    model: str | None = Field(default=None, max_length=200)
    ai_key: str | None = Field(default=None, max_length=400)
    fingerprint_key: str | None = Field(default=None, max_length=400)
    library: str | None = Field(default=None, max_length=4096)


class AiProfileIn(Body_):
    """Un proveedor tal y como lo rellena el formulario de Ajustes.

    Sirve para guardar, para probar sin guardar y para pedir los modelos:
    en los dos ultimos casos, si no trae clave se usa la guardada.
    """

    id: str | None = Field(default=None, max_length=80)
    provider: str = Field(max_length=40)
    name: str | None = Field(default=None, max_length=80)
    key: str | None = Field(default=None, max_length=1000)
    base_url: str | None = Field(default=None, max_length=1000)
    fields: dict[str, str] | None = None
    model: str | None = Field(default=None, max_length=200)
    chat_model: str | None = Field(default=None, max_length=200)
    headers: dict[str, str] | None = None
    extra: dict | None = None
    timeout: float | None = Field(default=None, ge=5, le=600)
    activate: bool = True


class AiActivateIn(Body_):
    id: str = Field(max_length=80)


class YoutubeIn(Body_):
    query: str = Field(default="", max_length=2000)
    results: int = Field(default=5, ge=1, le=20)
    quality: Literal["high", "medium", "variable"] | None = None
    file_it: bool = True
    force: bool = False


class ChatMessage(BaseModel):
    """Un mensaje de la conversacion, como lo guarda y lo pinta la interfaz.

    Lo que usa el asistente tiene su tipo (quien habla, el texto, las
    herramientas que uso, si lo escribio la app); lo demas que la interfaz
    guarda para pintarlo (`id`, `at`, `usage`, `via`...) pasa tal cual.
    """

    model_config = ConfigDict(extra="allow")
    role: str = Field(max_length=20)
    text: str | None = Field(default="", max_length=200_000)
    tools: list[dict] | None = None
    app: bool | None = None
    event: str | None = Field(default=None, max_length=40)
    hidden: bool | None = None

    def plain(self) -> dict:
        """El mensaje como diccionario, sin huecos: lo que esperan `chat` y `chats`."""
        out = self.model_dump(exclude_none=True)
        out["text"] = self.text or ""
        return out


class ChatIn(Body_):
    messages: list[ChatMessage] = Field(default_factory=list, max_length=200)
    # lo que la persona tiene delante: vista, seleccion, lo que suena (§3)
    context: dict | None = None


class ChatCreateIn(Body_):
    title: str = Field(default="", max_length=120)


class ChatRenameIn(Body_):
    title: str = Field(min_length=1, max_length=120)


class ChatAppendIn(Body_):
    messages: list[ChatMessage] = Field(default_factory=list, max_length=200)


class AiFallbackIn(Body_):
    enabled: bool


class AiBudgetIn(Body_):
    dollars: float = Field(ge=0, le=100000)


class SheetIn(Body_):
    with_lyrics: bool = False


class StudyMarker(BaseModel):
    """Un marcador: un tramo con nombre (`t`-`end`) y sus notas; sin `end`, un
    instante. Lo que no cuadre (un `t` negativo, un `end` antes de `t`) lo
    descarta `library.set_study`; aqui solo se exige que sean numeros."""

    model_config = ConfigDict(extra="ignore")
    t: float | None = None
    end: float | None = None
    label: str = ""
    notes: str = ""
    # varios tramos que se repiten seguidos, como uno solo
    parts: list[list[float]] | None = Field(default=None, max_length=32)


class StudyMetronome(BaseModel):
    """Lo ajustado a mano del metronomo sobre lo detectado (docs/CONTRATO-INTERNO.md §3)."""

    model_config = ConfigDict(extra="ignore")
    bpm: float | None = None
    meter: int | None = None
    shift: int | None = None
    mult: int | None = None


class StudyTrack(BaseModel):
    """Una pista separada en el mezclador del estudio."""

    model_config = ConfigDict(extra="ignore")
    gain: float | None = Field(default=None, ge=0, le=2)
    pan: float | None = Field(default=None, ge=-1, le=1)
    mute: bool | None = None
    solo: bool | None = None


class StudyMixer(BaseModel):
    """El mezclador de las pistas separadas: si suenan ellas en vez de la
    cancion, y como esta cada una."""

    model_config = ConfigDict(extra="ignore")
    on: bool | None = None
    tracks: dict[str, StudyTrack] | None = Field(default=None, max_length=8)


class StudyIn(Body_):
    """El modo estudio de una cancion: bucle [a, b], velocidad, tono corrido
    (semitonos, con fracciones: 0.5 es un cuarto de tono), ajustes del
    metronomo, marcadores [{t, end, label, notes}] y notas. Lo que no venga
    se quita."""

    loop: list[float] | None = None
    # varios tramos que se repiten uno detras de otro
    loops: list[list[float]] | None = Field(default=None, max_length=64)
    speed: float | None = None
    pitch: float | None = Field(default=None, ge=-12, le=12)
    metronome: StudyMetronome | None = None
    markers: list[StudyMarker] | None = Field(default=None, max_length=200)
    notes: str | None = Field(default=None, max_length=4000)
    mixer: StudyMixer | None = None


class ChatConfirmIn(Body_):
    tool: str = Field(min_length=1, max_length=64)
    args: dict = Field(default_factory=dict)


class SeparateIn(Body_):
    """Separar una cancion en pistas: con el modelo de 6 o el de 4."""

    model: Literal["6", "4"] = "6"


class StemMixTrack(Body_):
    source: str = Field(min_length=1, max_length=16)
    gain: float = Field(default=1.0, ge=0, le=2)
    pan: float = Field(default=0.0, ge=-1, le=1)


class StemMixIn(Body_):
    """Guardar la mezcla de las pistas separadas en un archivo."""

    tracks: list[StemMixTrack] = Field(min_length=1, max_length=8)
    path: str = Field(min_length=1, max_length=4096)
    format: Literal["mp3", "flac", "wav"] | None = None
    speed: float = Field(default=1.0, ge=0.25, le=3)
    pitch: float = Field(default=0.0, ge=-12, le=12)
