//! Leer los pesos de un checkpoint de PyTorch (`.ckpt`, `.pth`) sin PyTorch.
//!
//! Un checkpoint moderno es un zip: `data.pkl` (un pickle que dice qué
//! tensor es cuál) y `data/<n>` (los números de cada almacén, en crudo).
//! Abrir el pickle con el `pickle.load` de Python sin más ejecutaría lo que
//! trajera dentro: aquí lo lee una máquina de pickle propia que solo conoce
//! lo que un diccionario de pesos necesita (reconstruir un tensor desde su
//! almacén, un OrderedDict, un `torch.Size`) y rechaza todo lo demás. No se
//! copia ningún número: lo que sale es dónde está cada tensor dentro del
//! archivo (los checkpoints van sin comprimir), para mapearlo.
//!
//! Para el separador de máxima calidad: los pesos del Mega53 de ZFTurbo (MIT)
//! se publican así, y DanPlay los baja de su autor como los demás.
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::rc::Rc;
use zip::result::ZipError;

/// El tipo de los números de un almacén.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    F32,
    F16,
    F64,
    I64,
    I32,
    I16,
    I8,
    U8,
    Bool,
    /// bfloat16: los 16 bits altos de un float32 (se pasa a float32 al usarlo).
    BF16,
}

impl Tipo {
    fn de_almacen(nombre: &str) -> Option<Self> {
        Some(match nombre {
            "FloatStorage" => Self::F32,
            "HalfStorage" => Self::F16,
            "DoubleStorage" => Self::F64,
            "LongStorage" => Self::I64,
            "IntStorage" => Self::I32,
            "ShortStorage" => Self::I16,
            "CharStorage" => Self::I8,
            "ByteStorage" => Self::U8,
            "BoolStorage" => Self::Bool,
            "BFloat16Storage" => Self::BF16,
            _ => return None,
        })
    }

    /// Lo que ocupa cada número en el archivo.
    pub fn bytes(self) -> u64 {
        match self {
            Self::F64 | Self::I64 => 8,
            Self::F32 | Self::I32 => 4,
            Self::F16 | Self::I16 | Self::BF16 => 2,
            Self::I8 | Self::U8 | Self::Bool => 1,
        }
    }

    /// El nombre del tipo en numpy (el de los números crudos: el bfloat16
    /// se lee como uint16).
    pub fn numpy(self) -> &'static str {
        match self {
            Self::F32 => "float32",
            Self::F16 => "float16",
            Self::F64 => "float64",
            Self::I64 => "int64",
            Self::I32 => "int32",
            Self::I16 => "int16",
            Self::I8 => "int8",
            Self::U8 => "uint8",
            Self::Bool => "bool",
            Self::BF16 => "bfloat16",
        }
    }
}

/// Un almacén: un miembro `data/<clave>` del zip, sin comprimir.
#[derive(Debug, PartialEq, Eq)]
pub struct Almacen {
    pub clave: String,
    pub tipo: Tipo,
    /// Dónde empiezan sus números en el archivo.
    pub inicio: u64,
    /// Cuántos números hay (los que dice el checkpoint, o los que caben).
    pub elementos: u64,
}

/// Un tensor: una vista (forma y pasos, en números) de un almacén.
#[derive(Debug, PartialEq, Eq)]
pub struct Tensor {
    pub almacen: Rc<Almacen>,
    pub desplazamiento: u64,
    pub forma: Vec<u64>,
    pub pasos: Vec<u64>,
}

/// Lo que puede salir mal, con la excepción que daba la versión de Python.
#[derive(Debug, PartialEq, Eq)]
pub enum Fallo {
    /// `CheckpointError`: no es un checkpoint que se sepa leer sin riesgo.
    Checkpoint(String),
    /// `pickle.UnpicklingError`: el pickle está roto.
    Pickle(String),
    /// `KeyError`
    Clave(String),
    /// `TypeError`
    Tipo(String),
    /// `ValueError`
    Valor(String),
    /// `OSError` (`FileNotFoundError` si no existe)
    Archivo(std::io::ErrorKind, String),
}

fn checkpoint(msg: impl Into<String>) -> Fallo {
    Fallo::Checkpoint(msg.into())
}

fn roto(msg: impl Into<String>) -> Fallo {
    Fallo::Pickle(msg.into())
}

// ------------------------------------------------------------ los valores

/// Lo que se le permite reconstruir al pickle (el `find_class` de antes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clase {
    OrderedDict,
    RebuildTensor,
    RebuildParameter,
    /// `torch.Size`: una tupla.
    Size,
    /// `torch.FloatStorage` y los demás: solo valen dentro del identificador
    /// de un almacén (`persistent_load`), por su nombre.
    Almacen(&'static str),
}

impl Clase {
    fn de(module: &str, name: &str) -> Result<Self, Fallo> {
        const ALMACENES: [&str; 10] = [
            "FloatStorage",
            "HalfStorage",
            "DoubleStorage",
            "LongStorage",
            "IntStorage",
            "ShortStorage",
            "CharStorage",
            "ByteStorage",
            "BoolStorage",
            "BFloat16Storage",
        ];
        match (module, name) {
            ("collections", "OrderedDict") => Ok(Self::OrderedDict),
            ("torch._utils", "_rebuild_tensor_v2" | "_rebuild_tensor") => Ok(Self::RebuildTensor),
            ("torch._utils", "_rebuild_parameter") => Ok(Self::RebuildParameter),
            ("torch", "Size") => Ok(Self::Size),
            ("torch", n) if ALMACENES.contains(&n) => Ok(Self::Almacen(
                ALMACENES.iter().find(|a| **a == n).copied().unwrap_or(""),
            )),
            _ => Err(checkpoint(format!(
                "el checkpoint trae algo que no es un peso: {module}.{name}"
            ))),
        }
    }

    /// El `__name__` de lo que devolvía `find_class`.
    fn nombre(self) -> &'static str {
        match self {
            Self::OrderedDict => "OrderedDict",
            Self::RebuildTensor => "_rebuild_tensor_v2",
            Self::RebuildParameter => "_rebuild_parameter",
            Self::Size => "tuple",
            Self::Almacen(n) => n,
        }
    }
}

type Lista = Rc<RefCell<Vec<V>>>;
type Dict = Rc<RefCell<Vec<(V, V)>>>;

#[derive(Clone, Debug)]
enum V {
    Nada,
    Logico(bool),
    Entero(i64),
    Real(f64),
    Texto(Rc<str>),
    Bytes(Rc<[u8]>),
    Tupla(Rc<[V]>),
    Lista(Lista),
    Dict(Dict),
    Conjunto(Lista),
    Clase(Clase),
    Almacen(Rc<Almacen>),
    Tensor(Rc<Tensor>),
}

impl V {
    fn tipo(&self) -> &'static str {
        match self {
            Self::Nada => "NoneType",
            Self::Logico(_) => "bool",
            Self::Entero(_) => "int",
            Self::Real(_) => "float",
            Self::Texto(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::Tupla(_) => "tuple",
            Self::Lista(_) => "list",
            Self::Dict(_) => "OrderedDict",
            Self::Conjunto(_) => "set",
            Self::Clase(c) => {
                if matches!(c, Clase::Almacen(_)) {
                    "str"
                } else {
                    "type"
                }
            }
            Self::Almacen(_) => "_Storage",
            Self::Tensor(_) => "numpy.ndarray",
        }
    }

    /// Como `==` de Python entre claves de un diccionario.
    fn igual(&self, otro: &Self) -> bool {
        match (self.numero(), otro.numero()) {
            (Some(a), Some(b)) => return a == b,
            (Some(_), None) | (None, Some(_)) => return false,
            (None, None) => {}
        }
        match (self, otro) {
            (Self::Nada, Self::Nada) => true,
            (Self::Texto(a), Self::Texto(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::Tupla(a), Self::Tupla(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.igual(y))
            }
            (Self::Clase(a), Self::Clase(b)) => a == b,
            _ => false,
        }
    }

    fn numero(&self) -> Option<f64> {
        match self {
            Self::Logico(b) => Some(f64::from(u8::from(*b))),
            Self::Entero(i) => Some(*i as f64),
            Self::Real(f) => Some(*f),
            _ => None,
        }
    }

    fn hashable(&self) -> Result<(), Fallo> {
        match self {
            Self::Lista(_) | Self::Dict(_) | Self::Conjunto(_) => {
                Err(Fallo::Tipo(format!("unhashable type: '{}'", self.tipo())))
            }
            Self::Tupla(t) => t.iter().try_for_each(Self::hashable),
            _ => Ok(()),
        }
    }

    /// `str()` de Python, para las claves del diccionario de pesos.
    fn texto(&self) -> String {
        match self {
            Self::Nada => "None".into(),
            Self::Logico(true) => "True".into(),
            Self::Logico(false) => "False".into(),
            Self::Entero(i) => i.to_string(),
            Self::Real(f) => format!("{f:?}"),
            Self::Texto(s) => s.to_string(),
            Self::Tupla(t) => {
                let partes: Vec<String> = t.iter().map(Self::repr).collect();
                if partes.len() == 1 {
                    format!("({},)", partes[0])
                } else {
                    format!("({})", partes.join(", "))
                }
            }
            otro => format!("<{}>", otro.tipo()),
        }
    }

    fn repr(&self) -> String {
        match self {
            Self::Texto(s) => format!("'{s}'"),
            otro => otro.texto(),
        }
    }

    fn entero(&self) -> Result<i64, Fallo> {
        match self {
            Self::Entero(i) => Ok(*i),
            Self::Logico(b) => Ok(i64::from(*b)),
            Self::Real(f) if f.is_finite() => Ok(f.trunc() as i64),
            Self::Texto(s) => s.trim().replace('_', "").parse().map_err(|_| {
                Fallo::Valor(format!("invalid literal for int() with base 10: '{s}'"))
            }),
            otro => Err(Fallo::Tipo(format!(
                "int() argument must be a string, a bytes-like object or a real number, not '{}'",
                otro.tipo()
            ))),
        }
    }

    /// Una tupla de enteros (la forma o los pasos de un tensor).
    fn enteros(&self) -> Result<Vec<i64>, Fallo> {
        let items: Vec<V> = match self {
            Self::Tupla(t) => t.to_vec(),
            Self::Lista(l) => l.borrow().clone(),
            otro => {
                return Err(Fallo::Tipo(format!(
                    "'{}' object is not iterable",
                    otro.tipo()
                )));
            }
        };
        items.iter().map(Self::entero).collect()
    }

    fn items(&self) -> Result<Vec<V>, Fallo> {
        match self {
            Self::Tupla(t) => Ok(t.to_vec()),
            Self::Lista(l) | Self::Conjunto(l) => Ok(l.borrow().clone()),
            Self::Dict(d) => Ok(d.borrow().iter().map(|(k, _)| k.clone()).collect()),
            Self::Texto(s) => Ok(s
                .chars()
                .map(|c| Self::Texto(c.to_string().into()))
                .collect()),
            otro => Err(Fallo::Tipo(format!(
                "'{}' object is not iterable",
                otro.tipo()
            ))),
        }
    }
}

fn poner(d: &Dict, k: V, v: V) -> Result<(), Fallo> {
    k.hashable()?;
    let mut d = d.borrow_mut();
    if let Some(par) = d.iter_mut().find(|(clave, _)| clave.igual(&k)) {
        par.1 = v;
    } else {
        d.push((k, v));
    }
    Ok(())
}

// ------------------------------------------------------------ el archivo

/// El zip del checkpoint, para buscar los almacenes.
struct Zip {
    archivo: zip::ZipArchive<File>,
    prefijo: String,
    /// Los almacenes ya leídos, por su clave (uno sirve a varios tensores).
    leidos: HashMap<String, Rc<Almacen>>,
}

impl Zip {
    /// `persistent_load`: `('storage', tipo, clave, dispositivo, cuántos)`.
    fn almacen(&mut self, pid: &V) -> Result<Rc<Almacen>, Fallo> {
        let V::Tupla(pid) = pid else {
            return Err(checkpoint("almacen desconocido en el checkpoint"));
        };
        if pid.len() < 5 || !matches!(&pid[0], V::Texto(s) if &**s == "storage") {
            return Err(checkpoint("almacen desconocido en el checkpoint"));
        }
        let clave = pid[2].texto();
        let numel = pid[4].entero()?;
        if let Some(a) = self.leidos.get(&clave) {
            return Ok(Rc::clone(a));
        }
        // el nombre del tipo (lo que devolvía `find_class`, o un texto)
        let nombre = match &pid[1] {
            V::Clase(c) => c.nombre().to_owned(),
            V::Texto(s) => s.to_string(),
            _ => String::new(),
        };
        let tipo = Tipo::de_almacen(&nombre)
            .ok_or_else(|| checkpoint(format!("tipo de almacen desconocido: {nombre}")))?;
        let miembro = format!("{}data/{clave}", self.prefijo);
        let i = self.archivo.index_for_name(&miembro).ok_or_else(|| {
            Fallo::Clave(format!("There is no item named '{miembro}' in the archive"))
        })?;
        let (inicio, bytes) = {
            // sin descomprimir: solo hace falta dónde empiezan sus datos (tras
            // su cabecera local, que puede traer extras distintos de los del
            // directorio)
            let f = self
                .archivo
                .by_index_raw(i)
                .map_err(|_| checkpoint("zip dañado"))?;
            if f.compression() != zip::CompressionMethod::Stored {
                return Err(checkpoint("el checkpoint va comprimido"));
            }
            (
                f.data_start().ok_or_else(|| checkpoint("zip dañado"))?,
                f.size(),
            )
        };
        let item = tipo.bytes();
        if bytes % item != 0 {
            return Err(Fallo::Valor(
                "When changing to a smaller dtype, its size must be a divisor of the size of original dtype".into(),
            ));
        }
        let caben = bytes / item;
        let elementos = if numel > 0 {
            (numel as u64).min(caben)
        } else {
            caben
        };
        let a = Rc::new(Almacen {
            clave: clave.clone(),
            tipo,
            inicio,
            elementos,
        });
        self.leidos.insert(clave, Rc::clone(&a));
        Ok(a)
    }
}

/// `_rebuild_tensor(almacen, desplazamiento, forma, pasos, ...)`. A diferencia
/// del `as_strided` de numpy, se comprueba que la vista no se salga de su
/// almacén (un checkpoint mal hecho leía memoria de fuera).
fn tensor(args: &[V]) -> Result<V, Fallo> {
    if args.len() < 4 {
        return Err(Fallo::Tipo(format!(
            "_rebuild_tensor() missing {} required positional arguments",
            4 - args.len()
        )));
    }
    let V::Almacen(almacen) = &args[0] else {
        return Err(Fallo::Tipo(format!(
            "'{}' object has no attribute 'data'",
            args[0].tipo()
        )));
    };
    let desplazamiento = args[1].entero()?;
    let forma = args[2].enteros()?;
    let pasos = args[3].enteros()?;
    if desplazamiento < 0 || forma.iter().any(|n| *n < 0) || pasos.iter().any(|p| *p < 0) {
        return Err(checkpoint("un tensor con una forma imposible"));
    }
    let desplazamiento = desplazamiento as u64;
    let forma: Vec<u64> = forma.into_iter().map(|n| n as u64).collect();
    let pasos: Vec<u64> = pasos.into_iter().map(|p| p as u64).collect();
    if !forma.is_empty() && pasos.len() != forma.len() {
        return Err(Fallo::Valor(
            "mismatch in length of strides and shape".into(),
        ));
    }
    // el último número que toca la vista
    let vacio = forma.contains(&0);
    let ultimo = forma
        .iter()
        .zip(&pasos)
        .try_fold(desplazamiento, |acc, (n, p)| {
            n.saturating_sub(1)
                .checked_mul(*p)
                .and_then(|x| acc.checked_add(x))
        });
    match ultimo {
        Some(u) if vacio || u < almacen.elementos => {}
        _ => return Err(checkpoint("un tensor se sale de su almacén")),
    }
    Ok(V::Tensor(Rc::new(Tensor {
        almacen: Rc::clone(almacen),
        desplazamiento,
        forma,
        pasos,
    })))
}

fn llamar(f: &V, args: &[V]) -> Result<V, Fallo> {
    match f {
        V::Clase(Clase::OrderedDict) => {
            let d: Dict = Rc::new(RefCell::new(Vec::new()));
            match args {
                [] => {}
                [V::Dict(otro)] => {
                    for (k, v) in otro.borrow().iter() {
                        poner(&d, k.clone(), v.clone())?;
                    }
                }
                [pares] => {
                    for par in pares.items()? {
                        let kv = par.items()?;
                        if kv.len() != 2 {
                            return Err(Fallo::Valor("need more than 1 value to unpack".into()));
                        }
                        poner(&d, kv[0].clone(), kv[1].clone())?;
                    }
                }
                _ => return Err(Fallo::Tipo("expected at most 1 arguments".into())),
            }
            Ok(V::Dict(d))
        }
        V::Clase(Clase::RebuildTensor) => tensor(args),
        V::Clase(Clase::RebuildParameter) => args
            .first()
            .cloned()
            .ok_or_else(|| Fallo::Tipo("_rebuild_parameter() missing argument".into())),
        V::Clase(Clase::Size) => match args {
            [] => Ok(V::Tupla(Rc::from(Vec::new()))),
            [x] => Ok(V::Tupla(Rc::from(x.items()?))),
            _ => Err(Fallo::Tipo("tuple expected at most 1 argument".into())),
        },
        otro => Err(Fallo::Tipo(format!(
            "'{}' object is not callable",
            otro.tipo()
        ))),
    }
}

// ------------------------------------------------------------ el pickle

struct Lector<'a> {
    datos: &'a [u8],
    pos: usize,
}

impl Lector<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], Fallo> {
        let fin = self
            .pos
            .checked_add(n)
            .filter(|f| *f <= self.datos.len())
            .ok_or_else(|| roto("pickle data was truncated"))?;
        let b = &self.datos[self.pos..fin];
        self.pos = fin;
        Ok(b)
    }

    fn u8(&mut self) -> Result<u8, Fallo> {
        Ok(self.bytes(1)?[0])
    }

    fn le<const N: usize>(&mut self) -> Result<[u8; N], Fallo> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    fn linea(&mut self) -> Result<&str, Fallo> {
        let resto = &self.datos[self.pos..];
        let fin = resto
            .iter()
            .position(|b| *b == b'\n')
            .ok_or_else(|| roto("pickle data was truncated"))?;
        self.pos += fin + 1;
        std::str::from_utf8(&resto[..fin]).map_err(|_| roto("la línea del pickle no es texto"))
    }
}

fn largo(n: u64) -> Result<usize, Fallo> {
    usize::try_from(n).map_err(|_| roto("pickle data was truncated"))
}

fn texto(b: &[u8]) -> Result<V, Fallo> {
    std::str::from_utf8(b)
        .map(|s| V::Texto(s.into()))
        .map_err(|_| roto("'utf-8' codec can't decode the pickle"))
}

/// Un entero de LONG1/LONG4: en complemento a dos, de menor a mayor.
fn entero_largo(b: &[u8]) -> Result<V, Fallo> {
    if b.is_empty() {
        return Ok(V::Entero(0));
    }
    if b.len() > 8 {
        return Err(Fallo::Valor(
            "un entero demasiado grande para un checkpoint".into(),
        ));
    }
    let negativo = b[b.len() - 1] & 0x80 != 0;
    let mut buf = if negativo { [0xffu8; 8] } else { [0u8; 8] };
    buf[..b.len()].copy_from_slice(b);
    Ok(V::Entero(i64::from_le_bytes(buf)))
}

/// Un texto entre comillas de STRING (protocolo 0), sin sus escapes raros.
fn cadena(linea: &str) -> Result<V, Fallo> {
    let l = linea.trim_end();
    let dentro = if l.len() >= 2
        && ((l.starts_with('\'') && l.ends_with('\'')) || (l.starts_with('"') && l.ends_with('"')))
    {
        &l[1..l.len() - 1]
    } else {
        return Err(roto("the STRING opcode argument must be quoted"));
    };
    let mut out = String::new();
    let mut it = dentro.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(c @ ('\\' | '\'' | '"')) => out.push(c),
            Some('x') => {
                let h: String = it.by_ref().take(2).collect();
                let n = u32::from_str_radix(&h, 16).map_err(|_| roto("invalid \\x escape"))?;
                out.push(char::from_u32(n).unwrap_or('\u{fffd}'));
            }
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    Ok(V::Texto(out.into()))
}

/// Lee el pickle de `data.pkl`, con los almacenes del zip.
// una máquina de pickle es un `match` grande, una rama por instrucción
#[allow(clippy::too_many_lines)]
fn deshacer(datos: &[u8], zip: &mut Zip) -> Result<V, Fallo> {
    let mut l = Lector { datos, pos: 0 };
    let mut pila: Vec<V> = Vec::new();
    let mut marcas: Vec<usize> = Vec::new();
    let mut memo: HashMap<u64, V> = HashMap::new();

    // Como el de CPython: lo de antes de la última marca no se toca (la
    // «valla») hasta que se cierra esa marca.
    macro_rules! sacar {
        () => {{
            if pila.len() <= marcas.last().copied().unwrap_or(0) {
                return Err(roto("unpickling stack underflow"));
            }
            pila.pop()
                .ok_or_else(|| roto("unpickling stack underflow"))?
        }};
    }
    macro_rules! cima {
        () => {{
            if pila.len() <= marcas.last().copied().unwrap_or(0) {
                return Err(roto("unpickling stack underflow"));
            }
            pila.last()
                .ok_or_else(|| roto("unpickling stack underflow"))?
        }};
    }
    macro_rules! hasta_marca {
        () => {{
            let m = marcas.pop().ok_or_else(|| roto("could not find MARK"))?;
            if m > pila.len() {
                return Err(roto("could not find MARK"));
            }
            pila.split_off(m)
        }};
    }

    loop {
        let op = l.u8()?;
        match op {
            // PROTO, FRAME: nada que hacer
            0x80 => {
                l.u8()?;
            }
            0x95 => {
                l.bytes(8)?;
            }
            b'.' => return pila.pop().ok_or_else(|| roto("unpickling stack underflow")),
            b'(' => marcas.push(pila.len()),
            // POP: lo de arriba, o la marca si no hay nada encima
            b'0' => {
                if marcas.last() == Some(&pila.len()) {
                    marcas.pop();
                } else {
                    sacar!();
                }
            }
            b'1' => drop(hasta_marca!()),
            b'2' => {
                let v = cima!().clone();
                pila.push(v);
            }
            b'N' => pila.push(V::Nada),
            0x88 => pila.push(V::Logico(true)),
            0x89 => pila.push(V::Logico(false)),
            b'K' => {
                let n = l.u8()?;
                pila.push(V::Entero(i64::from(n)));
            }
            b'M' => {
                let n = u16::from_le_bytes(l.le()?);
                pila.push(V::Entero(i64::from(n)));
            }
            b'J' => {
                let n = i32::from_le_bytes(l.le()?);
                pila.push(V::Entero(i64::from(n)));
            }
            0x8a => {
                let n = usize::from(l.u8()?);
                pila.push(entero_largo(l.bytes(n)?)?);
            }
            0x8b => {
                let n = i32::from_le_bytes(l.le()?);
                if n < 0 {
                    return Err(roto("LONG pickle has negative byte count"));
                }
                pila.push(entero_largo(l.bytes(n as usize)?)?);
            }
            b'I' => {
                let s = l.linea()?.trim().to_owned();
                pila.push(match s.as_str() {
                    "00" => V::Logico(false),
                    "01" => V::Logico(true),
                    _ => {
                        V::Entero(s.parse().map_err(|_| {
                            Fallo::Valor(format!("invalid literal for int(): '{s}'"))
                        })?)
                    }
                });
            }
            b'L' => {
                let s = l.linea()?.trim().trim_end_matches('L').to_owned();
                pila.push(V::Entero(s.parse().map_err(|_| {
                    Fallo::Valor(format!("invalid literal for int(): '{s}'"))
                })?));
            }
            b'F' => {
                let s = l.linea()?.trim().to_owned();
                pila.push(V::Real(s.parse().map_err(|_| {
                    Fallo::Valor(format!("could not convert string to float: '{s}'"))
                })?));
            }
            b'G' => pila.push(V::Real(f64::from_be_bytes(l.le()?))),
            b'X' => {
                let n = u32::from_le_bytes(l.le()?);
                let n = largo(u64::from(n))?;
                pila.push(texto(l.bytes(n)?)?);
            }
            0x8c => {
                let n = usize::from(l.u8()?);
                pila.push(texto(l.bytes(n)?)?);
            }
            0x8d => {
                let n = u64::from_le_bytes(l.le()?);
                let n = largo(n)?;
                pila.push(texto(l.bytes(n)?)?);
            }
            b'V' => {
                let s = l.linea()?.to_owned();
                pila.push(V::Texto(s.into()));
            }
            b'S' => {
                let s = l.linea()?.to_owned();
                pila.push(cadena(&s)?);
            }
            b'T' => {
                let n = i32::from_le_bytes(l.le()?);
                if n < 0 {
                    return Err(roto("BINSTRING pickle has negative byte count"));
                }
                let b = l.bytes(n as usize)?;
                pila.push(V::Texto(
                    b.iter().map(|c| char::from(*c)).collect::<String>().into(),
                ));
            }
            b'U' => {
                let n = usize::from(l.u8()?);
                let b = l.bytes(n)?;
                pila.push(V::Texto(
                    b.iter().map(|c| char::from(*c)).collect::<String>().into(),
                ));
            }
            b'B' => {
                let n = u32::from_le_bytes(l.le()?);
                let n = largo(u64::from(n))?;
                pila.push(V::Bytes(Rc::from(l.bytes(n)?)));
            }
            b'C' => {
                let n = usize::from(l.u8()?);
                pila.push(V::Bytes(Rc::from(l.bytes(n)?)));
            }
            0x8e | 0x96 => {
                let n = u64::from_le_bytes(l.le()?);
                let n = largo(n)?;
                pila.push(V::Bytes(Rc::from(l.bytes(n)?)));
            }
            0x97 | 0x98 => {
                return Err(roto(
                    "pickle stream refers to out-of-band data but no *buffers* argument was given",
                ));
            }
            b')' => pila.push(V::Tupla(Rc::from(Vec::new()))),
            b't' => {
                let items = hasta_marca!();
                pila.push(V::Tupla(Rc::from(items)));
            }
            0x85..=0x87 => {
                let n = usize::from(op - 0x84);
                if pila.len() < n {
                    return Err(roto("unpickling stack underflow"));
                }
                let items = pila.split_off(pila.len() - n);
                pila.push(V::Tupla(Rc::from(items)));
            }
            b']' => pila.push(V::Lista(Rc::new(RefCell::new(Vec::new())))),
            b'l' => {
                let items = hasta_marca!();
                pila.push(V::Lista(Rc::new(RefCell::new(items))));
            }
            b'}' => pila.push(V::Dict(Rc::new(RefCell::new(Vec::new())))),
            b'd' => {
                let items = hasta_marca!();
                let d: Dict = Rc::new(RefCell::new(Vec::new()));
                for par in items.chunks(2) {
                    if let [k, v] = par {
                        poner(&d, k.clone(), v.clone())?;
                    }
                }
                pila.push(V::Dict(d));
            }
            0x8f => pila.push(V::Conjunto(Rc::new(RefCell::new(Vec::new())))),
            0x91 => {
                let items = hasta_marca!();
                pila.push(V::Conjunto(Rc::new(RefCell::new(items))));
            }
            0x90 => {
                let items = hasta_marca!();
                match cima!() {
                    V::Conjunto(c) => c.borrow_mut().extend(items),
                    otro => {
                        return Err(Fallo::Tipo(format!(
                            "'{}' object has no attribute 'add'",
                            otro.tipo()
                        )));
                    }
                }
            }
            b'a' => {
                let v = sacar!();
                match cima!() {
                    V::Lista(lista) => lista.borrow_mut().push(v),
                    otro => {
                        return Err(Fallo::Tipo(format!(
                            "'{}' object has no attribute 'append'",
                            otro.tipo()
                        )));
                    }
                }
            }
            b'e' => {
                let items = hasta_marca!();
                match cima!() {
                    V::Lista(lista) => lista.borrow_mut().extend(items),
                    otro => {
                        return Err(Fallo::Tipo(format!(
                            "'{}' object has no attribute 'extend'",
                            otro.tipo()
                        )));
                    }
                }
            }
            b's' => {
                let v = sacar!();
                let k = sacar!();
                match cima!() {
                    V::Dict(d) => poner(d, k, v)?,
                    otro => {
                        return Err(Fallo::Tipo(format!(
                            "'{}' object does not support item assignment",
                            otro.tipo()
                        )));
                    }
                }
            }
            b'u' => {
                let items = hasta_marca!();
                match cima!() {
                    V::Dict(d) => {
                        for par in items.chunks(2) {
                            if let [k, v] = par {
                                poner(d, k.clone(), v.clone())?;
                            }
                        }
                    }
                    otro => {
                        return Err(Fallo::Tipo(format!(
                            "'{}' object does not support item assignment",
                            otro.tipo()
                        )));
                    }
                }
            }
            b'c' => {
                let module = l.linea()?.to_owned();
                let name = l.linea()?.to_owned();
                pila.push(V::Clase(Clase::de(&module, &name)?));
            }
            0x93 => {
                let name = sacar!();
                let module = sacar!();
                let (V::Texto(module), V::Texto(name)) = (&module, &name) else {
                    return Err(roto("STACK_GLOBAL requires str"));
                };
                pila.push(V::Clase(Clase::de(module, name)?));
            }
            b'i' => {
                let module = l.linea()?.to_owned();
                let name = l.linea()?.to_owned();
                let clase = Clase::de(&module, &name)?;
                let args = hasta_marca!();
                pila.push(llamar(&V::Clase(clase), &args)?);
            }
            b'o' => {
                let mut items = hasta_marca!();
                if items.is_empty() {
                    return Err(roto("could not find MARK"));
                }
                let clase = items.remove(0);
                pila.push(llamar(&clase, &items)?);
            }
            b'R' => {
                let args = sacar!();
                let f = sacar!();
                let V::Tupla(args) = args else {
                    return Err(roto("REDUCE argument must be a tuple"));
                };
                pila.push(llamar(&f, &args)?);
            }
            0x81 | 0x92 => {
                if op == 0x92 {
                    sacar!(); // los argumentos con nombre
                }
                let args = sacar!();
                let clase = sacar!();
                let V::Tupla(args) = args else {
                    return Err(roto("NEWOBJ expected an arg tuple"));
                };
                pila.push(match clase {
                    // `OrderedDict.__new__`: un diccionario vacío
                    V::Clase(Clase::OrderedDict) => V::Dict(Rc::new(RefCell::new(Vec::new()))),
                    V::Clase(Clase::Size) => llamar(&clase, &args)?,
                    otro => {
                        return Err(Fallo::Tipo(format!(
                            "'{}' object is not a class",
                            otro.tipo()
                        )));
                    }
                });
            }
            b'b' => {
                let estado = sacar!();
                match cima!() {
                    // el `_metadata` de un state_dict: un atributo, no un peso
                    V::Dict(_) => {}
                    otro => {
                        return Err(Fallo::Tipo(format!(
                            "'{}' object has no attribute '__dict__' ({})",
                            otro.tipo(),
                            estado.tipo()
                        )));
                    }
                }
            }
            b'Q' => {
                let pid = sacar!();
                pila.push(V::Almacen(zip.almacen(&pid)?));
            }
            b'P' => {
                l.linea()?;
                return Err(checkpoint("almacen desconocido en el checkpoint"));
            }
            0x82..=0x84 => return Err(Fallo::Valor("unregistered extension code".into())),
            b'g' => {
                let n: u64 = l.linea()?.trim().parse().map_err(|_| roto("bad GET"))?;
                pila.push(
                    memo.get(&n)
                        .cloned()
                        .ok_or_else(|| roto(format!("Memo value not found at index {n}")))?,
                );
            }
            b'h' => {
                let n = u64::from(l.u8()?);
                pila.push(
                    memo.get(&n)
                        .cloned()
                        .ok_or_else(|| roto(format!("Memo value not found at index {n}")))?,
                );
            }
            b'j' => {
                let n = u64::from(u32::from_le_bytes(l.le()?));
                pila.push(
                    memo.get(&n)
                        .cloned()
                        .ok_or_else(|| roto(format!("Memo value not found at index {n}")))?,
                );
            }
            b'p' => {
                let n: u64 = l.linea()?.trim().parse().map_err(|_| roto("bad PUT"))?;
                memo.insert(n, cima!().clone());
            }
            b'q' => {
                let n = u64::from(l.u8()?);
                memo.insert(n, cima!().clone());
            }
            b'r' => {
                let n = u64::from(u32::from_le_bytes(l.le()?));
                memo.insert(n, cima!().clone());
            }
            0x94 => {
                let n = memo.len() as u64;
                memo.insert(n, cima!().clone());
            }
            otro => {
                return Err(roto(format!(
                    "invalid load key, '{}'.",
                    char::from(otro).escape_default()
                )));
            }
        }
    }
}

// ------------------------------------------------------------ todo junto

/// Los tensores del checkpoint, en su orden: `[(nombre, tensor)]`. Si el
/// checkpoint guarda el modelo dentro de otra clave (`state_dict`, `model`),
/// se dice con `clave` o se busca solo.
pub fn leer(ruta: &Path, clave: Option<&str>) -> Result<Vec<(String, Rc<Tensor>)>, Fallo> {
    let nombre = ruta
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let archivo = File::open(ruta).map_err(|e| Fallo::Archivo(e.kind(), e.to_string()))?;
    let mut archivo = match zip::ZipArchive::new(archivo) {
        Ok(a) => a,
        Err(ZipError::Io(e)) if e.kind() != std::io::ErrorKind::UnexpectedEof => {
            return Err(Fallo::Archivo(e.kind(), e.to_string()));
        }
        Err(_) => {
            return Err(checkpoint(format!(
                "{nombre} no es un checkpoint de PyTorch moderno"
            )));
        }
    };
    let pkl = (0..archivo.len())
        .filter_map(|i| {
            archivo
                .name_for_index(i)
                .and_then(Result::ok)
                .map(std::borrow::Cow::into_owned)
        })
        .find(|n| n.ends_with("/data.pkl") || n == "data.pkl")
        .ok_or_else(|| checkpoint(format!("{nombre} no trae data.pkl")))?;
    let prefijo = pkl[..pkl.len() - "data.pkl".len()].to_owned();
    let mut datos = Vec::new();
    archivo
        .by_name(&pkl)
        .map_err(|_| checkpoint("zip dañado"))?
        .read_to_end(&mut datos)
        .map_err(|e| Fallo::Archivo(e.kind(), e.to_string()))?;
    let mut zip = Zip {
        archivo,
        prefijo,
        leidos: HashMap::new(),
    };
    let mut obj = deshacer(&datos, &mut zip)?;
    if let Some(clave) = clave {
        let V::Dict(d) = &obj else {
            return Err(Fallo::Tipo(format!(
                "'{}' object is not subscriptable",
                obj.tipo()
            )));
        };
        let k = V::Texto(clave.into());
        let v = d
            .borrow()
            .iter()
            .find(|(c, _)| c.igual(&k))
            .map(|(_, v)| v.clone());
        obj = v.ok_or_else(|| Fallo::Clave(format!("'{clave}'")))?;
    } else if let V::Dict(d) = &obj {
        let sin_tensores = !d.borrow().iter().any(|(_, v)| matches!(v, V::Tensor(_)));
        if sin_tensores {
            let dentro = ["state_dict", "model", "model_state_dict"]
                .iter()
                .find_map(|c| {
                    let k = V::Texto((*c).into());
                    d.borrow()
                        .iter()
                        .find(|(clave, v)| clave.igual(&k) && matches!(v, V::Dict(_)))
                        .map(|(_, v)| v.clone())
                });
            if let Some(dentro) = dentro {
                obj = dentro;
            }
        }
    }
    let V::Dict(d) = obj else {
        return Err(checkpoint(format!(
            "{nombre} no es un diccionario de pesos"
        )));
    };
    let pesos = d
        .borrow()
        .iter()
        .filter_map(|(k, v)| match v {
            V::Tensor(t) => Some((k.texto(), Rc::clone(t))),
            _ => None,
        })
        .collect();
    Ok(pesos)
}

// ------------------------------------------------------------ safetensors

/// Un tensor de un `.safetensors`: dónde están sus números en el archivo
/// (desde el principio) y su forma.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TensorSt {
    pub nombre: String,
    pub tipo: Tipo,
    pub inicio: u64,
    pub fin: u64,
    pub forma: Vec<u64>,
}

/// Los tensores de un `.safetensors`, sin la biblioteca: una cabecera JSON y
/// los datos crudos detrás. No se lee ningún número (se mapean después).
pub fn safetensors(ruta: &Path) -> Result<Vec<TensorSt>, Fallo> {
    let nombre = ruta
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let no_es = || Fallo::Valor(format!("{nombre} no es un safetensors"));
    let mut f = File::open(ruta).map_err(|e| Fallo::Archivo(e.kind(), e.to_string()))?;
    let mut largo = [0u8; 8];
    let leidos = f
        .read(&mut largo)
        .map_err(|e| Fallo::Archivo(e.kind(), e.to_string()))?;
    let size = u64::from_le_bytes(largo);
    if leidos < 8 || size == 0 || size > 50_000_000 {
        return Err(no_es());
    }
    let mut cabecera = vec![0u8; size as usize];
    f.read_exact(&mut cabecera).map_err(|_| no_es())?;
    let json: serde_json::Value =
        serde_json::from_slice(&cabecera).map_err(|e| Fallo::Valor(e.to_string()))?;
    let serde_json::Value::Object(cabecera) = json else {
        return Err(Fallo::Valor(format!("{nombre} no es un safetensors")));
    };
    let datos = 8 + size;
    let mut out = Vec::new();
    for (k, meta) in cabecera {
        if k == "__metadata__" {
            continue;
        }
        let dtype = meta
            .get("dtype")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let tipo = match dtype {
            "F32" => Tipo::F32,
            "F16" => Tipo::F16,
            "I64" => Tipo::I64,
            "I32" => Tipo::I32,
            otro => return Err(Fallo::Valor(format!("tipo {otro} desconocido en {nombre}"))),
        };
        let pos: Vec<u64> = meta
            .get("data_offsets")
            .and_then(serde_json::Value::as_array)
            .map(|v| v.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default();
        let forma: Vec<u64> = meta
            .get("shape")
            .and_then(serde_json::Value::as_array)
            .map(|v| v.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default();
        let [a, z] = pos[..] else { return Err(no_es()) };
        let cuantos: u64 = forma.iter().product();
        if z < a || (z - a) != cuantos * tipo.bytes() {
            return Err(Fallo::Valor(format!(
                "cannot reshape array of size {} into shape {forma:?}",
                (z.saturating_sub(a)) / tipo.bytes()
            )));
        }
        out.push(TensorSt {
            nombre: k,
            tipo,
            inicio: datos + a,
            fin: datos + z,
            forma,
        });
    }
    Ok(out)
}

// ------------------------------------------------------------ a float32

fn a_f32(tipo: Tipo, b: &[u8]) -> f32 {
    match tipo {
        Tipo::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        Tipo::F16 => half::f16::from_le_bytes([b[0], b[1]]).to_f32(),
        Tipo::BF16 => f32::from_bits(u32::from(u16::from_le_bytes([b[0], b[1]])) << 16),
        Tipo::F64 => f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32,
        Tipo::I64 => i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32,
        Tipo::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
        Tipo::I16 => f32::from(i16::from_le_bytes([b[0], b[1]])),
        Tipo::I8 => f32::from(b[0] as i8),
        Tipo::U8 => f32::from(b[0]),
        Tipo::Bool => f32::from(u8::from(b[0] != 0)),
    }
}

fn leer_en(archivo: &mut File, inicio: u64, bytes: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Seek, SeekFrom};
    archivo.seek(SeekFrom::Start(inicio))?;
    let mut out = vec![0u8; usize::try_from(bytes).map_err(std::io::Error::other)?];
    archivo.read_exact(&mut out)?;
    Ok(out)
}

/// Los números de un tensor de un checkpoint, en float32 y en orden C (como
/// `astype(float32)` de la vista de numpy): solo se lee lo que ocupa.
pub fn materializar(archivo: &mut File, t: &Tensor) -> std::io::Result<(Vec<usize>, Vec<f32>)> {
    let a = &t.almacen;
    let item = a.tipo.bytes();
    let forma: Vec<usize> = t.forma.iter().map(|n| *n as usize).collect();
    let cuantos: usize = forma.iter().product();
    if cuantos == 0 {
        return Ok((forma, Vec::new()));
    }
    // el tramo del almacén que toca la vista
    let ultimo = t
        .forma
        .iter()
        .zip(&t.pasos)
        .map(|(n, p)| (n - 1) * p)
        .sum::<u64>()
        + t.desplazamiento;
    let crudo = leer_en(
        archivo,
        a.inicio + t.desplazamiento * item,
        (ultimo - t.desplazamiento + 1) * item,
    )?;
    let mut out = Vec::with_capacity(cuantos);
    let mut idx = vec![0usize; forma.len()];
    for _ in 0..cuantos {
        let pos: u64 = idx.iter().zip(&t.pasos).map(|(i, p)| *i as u64 * p).sum();
        let b = (pos * item) as usize;
        out.push(a_f32(a.tipo, &crudo[b..b + item as usize]));
        // el siguiente índice, el último eje el más rápido
        for d in (0..idx.len()).rev() {
            idx[d] += 1;
            if idx[d] < forma[d] {
                break;
            }
            idx[d] = 0;
        }
    }
    Ok((forma, out))
}

/// Los números de un tensor de un `.safetensors`, en float32.
pub fn materializar_st(
    archivo: &mut File,
    t: &TensorSt,
) -> std::io::Result<(Vec<usize>, Vec<f32>)> {
    let crudo = leer_en(archivo, t.inicio, t.fin - t.inicio)?;
    let item = t.tipo.bytes() as usize;
    let datos = crudo.chunks_exact(item).map(|b| a_f32(t.tipo, b)).collect();
    Ok((t.forma.iter().map(|n| *n as usize).collect(), datos))
}

#[cfg(test)]
mod tests;
