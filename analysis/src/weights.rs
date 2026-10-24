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
pub enum Type {
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

impl Type {
    fn from_storage_name(name: &str) -> Option<Self> {
        Some(match name {
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
pub struct Storage {
    pub key: String,
    pub kind: Type,
    /// Dónde empiezan sus números en el archivo.
    pub start: u64,
    /// Cuántos números hay (los que dice el checkpoint, o los que caben).
    pub elements: u64,
}

/// Un tensor: una vista (forma y pasos, en números) de un almacén.
#[derive(Debug, PartialEq, Eq)]
pub struct Tensor {
    pub storage: Rc<Storage>,
    pub storage_offset: u64,
    pub shape: Vec<u64>,
    pub steps: Vec<u64>,
}

/// Lo que puede salir mal.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// No es un checkpoint (o un safetensors) que se sepa leer sin riesgo.
    Checkpoint(String),
    /// El pickle está roto: lo que trae no tiene sentido.
    Pickle(String),
    /// Al leer el archivo (`NotFound`, si no existe).
    Io(std::io::ErrorKind, String),
}

fn checkpoint(msg: impl Into<String>) -> Error {
    Error::Checkpoint(msg.into())
}

fn corrupt(msg: impl Into<String>) -> Error {
    Error::Pickle(msg.into())
}

// ------------------------------------------------------------ los valores

/// Lo que se le permite reconstruir al pickle (el `find_class` de antes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    OrderedDict,
    RebuildTensor,
    RebuildParameter,
    /// `torch.Size`: una tupla.
    Size,
    /// `torch.FloatStorage` y los demás: solo valen dentro del identificador
    /// de un almacén (`persistent_load`), por su nombre.
    Storage(&'static str),
}

impl Class {
    fn from_names(module: &str, name: &str) -> Result<Self, Error> {
        const STORAGE_TYPES: [&str; 10] = [
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
            ("torch", n) if STORAGE_TYPES.contains(&n) => Ok(Self::Storage(
                STORAGE_TYPES
                    .iter()
                    .find(|a| **a == n)
                    .copied()
                    .unwrap_or(""),
            )),
            _ => Err(checkpoint(format!(
                "el checkpoint trae algo que no es un peso: {module}.{name}"
            ))),
        }
    }

    /// El `__name__` de lo que devolvía `find_class`.
    fn name(self) -> &'static str {
        match self {
            Self::OrderedDict => "OrderedDict",
            Self::RebuildTensor => "_rebuild_tensor_v2",
            Self::RebuildParameter => "_rebuild_parameter",
            Self::Size => "tuple",
            Self::Storage(n) => n,
        }
    }
}

type List = Rc<RefCell<Vec<V>>>;
type Dict = Rc<RefCell<Vec<(V, V)>>>;

#[derive(Clone, Debug)]
enum V {
    Nothing,
    Boolean(bool),
    Integer(i64),
    Real(f64),
    Text(Rc<str>),
    Bytes(Rc<[u8]>),
    Tuple(Rc<[V]>),
    List(List),
    Dict(Dict),
    Set(List),
    Class(Class),
    Storage(Rc<Storage>),
    Tensor(Rc<Tensor>),
}

impl V {
    fn kind(&self) -> &'static str {
        match self {
            Self::Nothing => "NoneType",
            Self::Boolean(_) => "bool",
            Self::Integer(_) => "int",
            Self::Real(_) => "float",
            Self::Text(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::Tuple(_) => "tuple",
            Self::List(_) => "list",
            Self::Dict(_) => "OrderedDict",
            Self::Set(_) => "set",
            Self::Class(c) => {
                if matches!(c, Class::Storage(_)) {
                    "str"
                } else {
                    "type"
                }
            }
            Self::Storage(_) => "_Storage",
            Self::Tensor(_) => "numpy.ndarray",
        }
    }

    /// Como `==` de Python entre claves de un diccionario.
    fn equals(&self, other: &Self) -> bool {
        match (self.number(), other.number()) {
            (Some(a), Some(b)) => return a == b,
            (Some(_), None) | (None, Some(_)) => return false,
            (None, None) => {}
        }
        match (self, other) {
            (Self::Nothing, Self::Nothing) => true,
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::Tuple(a), Self::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
            }
            (Self::Class(a), Self::Class(b)) => a == b,
            _ => false,
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Self::Boolean(b) => Some(f64::from(u8::from(*b))),
            Self::Integer(i) => Some(*i as f64),
            Self::Real(f) => Some(*f),
            _ => None,
        }
    }

    fn hashable(&self) -> Result<(), Error> {
        match self {
            Self::List(_) | Self::Dict(_) | Self::Set(_) => Err(Error::Pickle(format!(
                "un {} no sirve de clave",
                self.kind()
            ))),
            Self::Tuple(t) => t.iter().try_for_each(Self::hashable),
            _ => Ok(()),
        }
    }

    /// `str()` de Python, para las claves del diccionario de pesos.
    fn text(&self) -> String {
        match self {
            Self::Nothing => "None".into(),
            Self::Boolean(true) => "True".into(),
            Self::Boolean(false) => "False".into(),
            Self::Integer(i) => i.to_string(),
            Self::Real(f) => format!("{f:?}"),
            Self::Text(s) => s.to_string(),
            Self::Tuple(t) => {
                let parts: Vec<String> = t.iter().map(Self::repr).collect();
                if parts.len() == 1 {
                    format!("({},)", parts[0])
                } else {
                    format!("({})", parts.join(", "))
                }
            }
            other => format!("<{}>", other.kind()),
        }
    }

    fn repr(&self) -> String {
        match self {
            Self::Text(s) => format!("'{s}'"),
            other => other.text(),
        }
    }

    fn integer(&self) -> Result<i64, Error> {
        match self {
            Self::Integer(i) => Ok(*i),
            Self::Boolean(b) => Ok(i64::from(*b)),
            Self::Real(f) if f.is_finite() => Ok(f.trunc() as i64),
            Self::Text(s) => s
                .trim()
                .replace('_', "")
                .parse()
                .map_err(|_| Error::Pickle(format!("«{s}» no es un entero"))),
            other => Err(Error::Pickle(format!(
                "se esperaba un entero y llegó un {}",
                other.kind()
            ))),
        }
    }

    /// Una tupla de enteros (la forma o los pasos de un tensor).
    fn integers(&self) -> Result<Vec<i64>, Error> {
        let items: Vec<V> = match self {
            Self::Tuple(t) => t.to_vec(),
            Self::List(l) => l.borrow().clone(),
            other => {
                return Err(Error::Pickle(format!("un {} no se recorre", other.kind())));
            }
        };
        items.iter().map(Self::integer).collect()
    }

    fn items(&self) -> Result<Vec<V>, Error> {
        match self {
            Self::Tuple(t) => Ok(t.to_vec()),
            Self::List(l) | Self::Set(l) => Ok(l.borrow().clone()),
            Self::Dict(d) => Ok(d.borrow().iter().map(|(k, _)| k.clone()).collect()),
            Self::Text(s) => Ok(s
                .chars()
                .map(|c| Self::Text(c.to_string().into()))
                .collect()),
            other => Err(Error::Pickle(format!("un {} no se recorre", other.kind()))),
        }
    }
}

fn put(d: &Dict, k: V, v: V) -> Result<(), Error> {
    k.hashable()?;
    let mut d = d.borrow_mut();
    if let Some(par) = d.iter_mut().find(|(key, _)| key.equals(&k)) {
        par.1 = v;
    } else {
        d.push((k, v));
    }
    Ok(())
}

// ------------------------------------------------------------ el archivo

/// El zip del checkpoint, para buscar los almacenes.
struct Zip {
    file: zip::ZipArchive<File>,
    prefix: String,
    /// Los almacenes ya leídos, por su clave (uno sirve a varios tensores).
    already_read: HashMap<String, Rc<Storage>>,
}

impl Zip {
    /// `persistent_load`: `('storage', kind, key, dispositivo, cuántos)`.
    fn storage(&mut self, pid: &V) -> Result<Rc<Storage>, Error> {
        let V::Tuple(pid) = pid else {
            return Err(checkpoint("almacen desconocido en el checkpoint"));
        };
        if pid.len() < 5 || !matches!(&pid[0], V::Text(s) if &**s == "storage") {
            return Err(checkpoint("almacen desconocido en el checkpoint"));
        }
        let key = pid[2].text();
        let numel = pid[4].integer()?;
        if let Some(a) = self.already_read.get(&key) {
            return Ok(Rc::clone(a));
        }
        // el nombre del tipo (lo que devolvía `find_class`, o un texto)
        let name = match &pid[1] {
            V::Class(c) => c.name().to_owned(),
            V::Text(s) => s.to_string(),
            _ => String::new(),
        };
        let kind = Type::from_storage_name(&name)
            .ok_or_else(|| checkpoint(format!("tipo de almacen desconocido: {name}")))?;
        let member = format!("{}data/{key}", self.prefix);
        let i = self
            .file
            .index_for_name(&member)
            .ok_or_else(|| Error::Pickle(format!("el archivo no trae «{member}»")))?;
        let (start, bytes) = {
            // sin descomprimir: solo hace falta dónde empiezan sus datos (tras
            // su cabecera local, que puede traer extras distintos de los del
            // directorio)
            let f = self
                .file
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
        let item = kind.bytes();
        if bytes % item != 0 {
            return Err(Error::Pickle(
                "el tipo más pequeño no cabe un número entero de veces en el de antes".into(),
            ));
        }
        let capacity = bytes / item;
        let elements = if numel > 0 {
            (numel as u64).min(capacity)
        } else {
            capacity
        };
        let a = Rc::new(Storage {
            key: key.clone(),
            kind,
            start,
            elements,
        });
        self.already_read.insert(key, Rc::clone(&a));
        Ok(a)
    }
}

/// `_rebuild_tensor(storage, storage_offset, shape, steps, ...)`. A diferencia
/// del `as_strided` de numpy, se comprueba que la vista no se salga de su
/// almacén (un checkpoint mal hecho leía memoria de fuera).
fn tensor(args: &[V]) -> Result<V, Error> {
    if args.len() < 4 {
        return Err(Error::Pickle(format!(
            "a la reconstrucción de un tensor le faltan {} argumentos",
            4 - args.len()
        )));
    }
    let V::Storage(storage) = &args[0] else {
        return Err(Error::Pickle(format!(
            "se esperaba un almacén y llegó un {}",
            args[0].kind()
        )));
    };
    let storage_offset = args[1].integer()?;
    let shape = args[2].integers()?;
    let steps = args[3].integers()?;
    if storage_offset < 0 || shape.iter().any(|n| *n < 0) || steps.iter().any(|p| *p < 0) {
        return Err(checkpoint("un tensor con una forma imposible"));
    }
    let storage_offset = storage_offset as u64;
    let shape: Vec<u64> = shape.into_iter().map(|n| n as u64).collect();
    let steps: Vec<u64> = steps.into_iter().map(|p| p as u64).collect();
    if !shape.is_empty() && steps.len() != shape.len() {
        return Err(Error::Pickle(
            "los pasos y la forma del tensor no casan".into(),
        ));
    }
    // el último número que toca la vista
    let empty = shape.contains(&0);
    let last = shape
        .iter()
        .zip(&steps)
        .try_fold(storage_offset, |acc, (n, p)| {
            n.saturating_sub(1)
                .checked_mul(*p)
                .and_then(|x| acc.checked_add(x))
        });
    match last {
        Some(u) if empty || u < storage.elements => {}
        _ => return Err(checkpoint("un tensor se sale de su almacén")),
    }
    Ok(V::Tensor(Rc::new(Tensor {
        storage: Rc::clone(storage),
        storage_offset,
        shape,
        steps,
    })))
}

fn call(f: &V, args: &[V]) -> Result<V, Error> {
    match f {
        V::Class(Class::OrderedDict) => {
            let d: Dict = Rc::new(RefCell::new(Vec::new()));
            match args {
                [] => {}
                [V::Dict(other)] => {
                    for (k, v) in other.borrow().iter() {
                        put(&d, k.clone(), v.clone())?;
                    }
                }
                [pares] => {
                    for par in pares.items()? {
                        let kv = par.items()?;
                        if kv.len() != 2 {
                            return Err(Error::Pickle("faltan valores".into()));
                        }
                        put(&d, kv[0].clone(), kv[1].clone())?;
                    }
                }
                _ => return Err(Error::Pickle("sobran argumentos".into())),
            }
            Ok(V::Dict(d))
        }
        V::Class(Class::RebuildTensor) => tensor(args),
        V::Class(Class::RebuildParameter) => args.first().cloned().ok_or_else(|| {
            Error::Pickle("a la reconstrucción de un parámetro le falta su tensor".into())
        }),
        V::Class(Class::Size) => match args {
            [] => Ok(V::Tuple(Rc::from(Vec::new()))),
            [x] => Ok(V::Tuple(Rc::from(x.items()?))),
            _ => Err(Error::Pickle("sobran argumentos para una tupla".into())),
        },
        other => Err(Error::Pickle(format!(
            "un {} no se puede llamar",
            other.kind()
        ))),
    }
}

// ------------------------------------------------------------ el pickle

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|f| *f <= self.data.len())
            .ok_or_else(|| corrupt("pickle data was truncated"))?;
        let b = &self.data[self.pos..end];
        self.pos = end;
        Ok(b)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }

    fn le<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    fn line(&mut self) -> Result<&str, Error> {
        let rest = &self.data[self.pos..];
        let end = rest
            .iter()
            .position(|b| *b == b'\n')
            .ok_or_else(|| corrupt("pickle data was truncated"))?;
        self.pos += end + 1;
        std::str::from_utf8(&rest[..end]).map_err(|_| corrupt("la línea del pickle no es texto"))
    }
}

fn length(n: u64) -> Result<usize, Error> {
    usize::try_from(n).map_err(|_| corrupt("pickle data was truncated"))
}

fn text(b: &[u8]) -> Result<V, Error> {
    std::str::from_utf8(b)
        .map(|s| V::Text(s.into()))
        .map_err(|_| corrupt("'utf-8' codec can't decode the pickle"))
}

/// Un entero de LONG1/LONG4: en complemento a dos, de menor a mayor.
fn long_integer(b: &[u8]) -> Result<V, Error> {
    if b.is_empty() {
        return Ok(V::Integer(0));
    }
    if b.len() > 8 {
        return Err(Error::Pickle(
            "un entero demasiado grande para un checkpoint".into(),
        ));
    }
    let negative = b[b.len() - 1] & 0x80 != 0;
    let mut buf = if negative { [0xffu8; 8] } else { [0u8; 8] };
    buf[..b.len()].copy_from_slice(b);
    Ok(V::Integer(i64::from_le_bytes(buf)))
}

/// Un texto entre comillas de STRING (protocolo 0), sin sus escapes raros.
fn chain(line: &str) -> Result<V, Error> {
    let l = line.trim_end();
    let inside = if l.len() >= 2
        && ((l.starts_with('\'') && l.ends_with('\'')) || (l.starts_with('"') && l.ends_with('"')))
    {
        &l[1..l.len() - 1]
    } else {
        return Err(corrupt("the STRING opcode argument must be quoted"));
    };
    let mut out = String::new();
    let mut it = inside.chars();
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
                let n = u32::from_str_radix(&h, 16).map_err(|_| corrupt("invalid \\x escape"))?;
                out.push(char::from_u32(n).unwrap_or('\u{fffd}'));
            }
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    Ok(V::Text(out.into()))
}

/// Lee el pickle de `data.pkl`, con los almacenes del zip.
// una máquina de pickle es un `match` grande, una rama por instrucción
#[allow(clippy::too_many_lines)]
fn unpack(data: &[u8], zip: &mut Zip) -> Result<V, Error> {
    let mut l = Reader { data, pos: 0 };
    let mut stack: Vec<V> = Vec::new();
    let mut markers: Vec<usize> = Vec::new();
    let mut memo: HashMap<u64, V> = HashMap::new();

    // Como el de CPython: lo de antes de la última marca no se toca (la
    // «valla») hasta que se cierra esa marca.
    macro_rules! pop {
        () => {{
            if stack.len() <= markers.last().copied().unwrap_or(0) {
                return Err(corrupt("unpickling stack underflow"));
            }
            stack
                .pop()
                .ok_or_else(|| corrupt("unpickling stack underflow"))?
        }};
    }
    macro_rules! top {
        () => {{
            if stack.len() <= markers.last().copied().unwrap_or(0) {
                return Err(corrupt("unpickling stack underflow"));
            }
            stack
                .last()
                .ok_or_else(|| corrupt("unpickling stack underflow"))?
        }};
    }
    macro_rules! pop_to_mark {
        () => {{
            let m = markers
                .pop()
                .ok_or_else(|| corrupt("could not find MARK"))?;
            if m > stack.len() {
                return Err(corrupt("could not find MARK"));
            }
            stack.split_off(m)
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
            b'.' => {
                return stack
                    .pop()
                    .ok_or_else(|| corrupt("unpickling stack underflow"));
            }
            b'(' => markers.push(stack.len()),
            // POP: lo de arriba, o la marca si no hay nada encima
            b'0' => {
                if markers.last() == Some(&stack.len()) {
                    markers.pop();
                } else {
                    pop!();
                }
            }
            b'1' => drop(pop_to_mark!()),
            b'2' => {
                let v = top!().clone();
                stack.push(v);
            }
            b'N' => stack.push(V::Nothing),
            0x88 => stack.push(V::Boolean(true)),
            0x89 => stack.push(V::Boolean(false)),
            b'K' => {
                let n = l.u8()?;
                stack.push(V::Integer(i64::from(n)));
            }
            b'M' => {
                let n = u16::from_le_bytes(l.le()?);
                stack.push(V::Integer(i64::from(n)));
            }
            b'J' => {
                let n = i32::from_le_bytes(l.le()?);
                stack.push(V::Integer(i64::from(n)));
            }
            0x8a => {
                let n = usize::from(l.u8()?);
                stack.push(long_integer(l.bytes(n)?)?);
            }
            0x8b => {
                let n = i32::from_le_bytes(l.le()?);
                if n < 0 {
                    return Err(corrupt("LONG pickle has negative byte count"));
                }
                stack.push(long_integer(l.bytes(n as usize)?)?);
            }
            b'I' => {
                let s = l.line()?.trim().to_owned();
                stack.push(match s.as_str() {
                    "00" => V::Boolean(false),
                    "01" => V::Boolean(true),
                    _ => V::Integer(
                        s.parse()
                            .map_err(|_| Error::Pickle(format!("«{s}» no es un entero")))?,
                    ),
                });
            }
            b'L' => {
                let s = l.line()?.trim().trim_end_matches('L').to_owned();
                stack
                    .push(V::Integer(s.parse().map_err(|_| {
                        Error::Pickle(format!("«{s}» no es un entero"))
                    })?));
            }
            b'F' => {
                let s = l.line()?.trim().to_owned();
                stack
                    .push(V::Real(s.parse().map_err(|_| {
                        Error::Pickle(format!("«{s}» no es un número"))
                    })?));
            }
            b'G' => stack.push(V::Real(f64::from_be_bytes(l.le()?))),
            b'X' => {
                let n = u32::from_le_bytes(l.le()?);
                let n = length(u64::from(n))?;
                stack.push(text(l.bytes(n)?)?);
            }
            0x8c => {
                let n = usize::from(l.u8()?);
                stack.push(text(l.bytes(n)?)?);
            }
            0x8d => {
                let n = u64::from_le_bytes(l.le()?);
                let n = length(n)?;
                stack.push(text(l.bytes(n)?)?);
            }
            b'V' => {
                let s = l.line()?.to_owned();
                stack.push(V::Text(s.into()));
            }
            b'S' => {
                let s = l.line()?.to_owned();
                stack.push(chain(&s)?);
            }
            b'T' => {
                let n = i32::from_le_bytes(l.le()?);
                if n < 0 {
                    return Err(corrupt("BINSTRING pickle has negative byte count"));
                }
                let b = l.bytes(n as usize)?;
                stack.push(V::Text(
                    b.iter().map(|c| char::from(*c)).collect::<String>().into(),
                ));
            }
            b'U' => {
                let n = usize::from(l.u8()?);
                let b = l.bytes(n)?;
                stack.push(V::Text(
                    b.iter().map(|c| char::from(*c)).collect::<String>().into(),
                ));
            }
            b'B' => {
                let n = u32::from_le_bytes(l.le()?);
                let n = length(u64::from(n))?;
                stack.push(V::Bytes(Rc::from(l.bytes(n)?)));
            }
            b'C' => {
                let n = usize::from(l.u8()?);
                stack.push(V::Bytes(Rc::from(l.bytes(n)?)));
            }
            0x8e | 0x96 => {
                let n = u64::from_le_bytes(l.le()?);
                let n = length(n)?;
                stack.push(V::Bytes(Rc::from(l.bytes(n)?)));
            }
            0x97 | 0x98 => {
                return Err(corrupt(
                    "pickle stream refers to out-of-band data but no *buffers* argument was given",
                ));
            }
            b')' => stack.push(V::Tuple(Rc::from(Vec::new()))),
            b't' => {
                let items = pop_to_mark!();
                stack.push(V::Tuple(Rc::from(items)));
            }
            0x85..=0x87 => {
                let n = usize::from(op - 0x84);
                if stack.len() < n {
                    return Err(corrupt("unpickling stack underflow"));
                }
                let items = stack.split_off(stack.len() - n);
                stack.push(V::Tuple(Rc::from(items)));
            }
            b']' => stack.push(V::List(Rc::new(RefCell::new(Vec::new())))),
            b'l' => {
                let items = pop_to_mark!();
                stack.push(V::List(Rc::new(RefCell::new(items))));
            }
            b'}' => stack.push(V::Dict(Rc::new(RefCell::new(Vec::new())))),
            b'd' => {
                let items = pop_to_mark!();
                let d: Dict = Rc::new(RefCell::new(Vec::new()));
                for par in items.chunks(2) {
                    if let [k, v] = par {
                        put(&d, k.clone(), v.clone())?;
                    }
                }
                stack.push(V::Dict(d));
            }
            0x8f => stack.push(V::Set(Rc::new(RefCell::new(Vec::new())))),
            0x91 => {
                let items = pop_to_mark!();
                stack.push(V::Set(Rc::new(RefCell::new(items))));
            }
            0x90 => {
                let items = pop_to_mark!();
                match top!() {
                    V::Set(c) => c.borrow_mut().extend(items),
                    other => {
                        return Err(Error::Pickle(format!(
                            "se añade a un {}, que no es un conjunto",
                            other.kind()
                        )));
                    }
                }
            }
            b'a' => {
                let v = pop!();
                match top!() {
                    V::List(list) => list.borrow_mut().push(v),
                    other => {
                        return Err(Error::Pickle(format!(
                            "se añade a un {}, que no es una lista",
                            other.kind()
                        )));
                    }
                }
            }
            b'e' => {
                let items = pop_to_mark!();
                match top!() {
                    V::List(list) => list.borrow_mut().extend(items),
                    other => {
                        return Err(Error::Pickle(format!(
                            "se añade a un {}, que no es una lista",
                            other.kind()
                        )));
                    }
                }
            }
            b's' => {
                let v = pop!();
                let k = pop!();
                match top!() {
                    V::Dict(d) => put(d, k, v)?,
                    other => {
                        return Err(Error::Pickle(format!(
                            "se guarda una clave en un {}, que no es un diccionario",
                            other.kind()
                        )));
                    }
                }
            }
            b'u' => {
                let items = pop_to_mark!();
                match top!() {
                    V::Dict(d) => {
                        for par in items.chunks(2) {
                            if let [k, v] = par {
                                put(d, k.clone(), v.clone())?;
                            }
                        }
                    }
                    other => {
                        return Err(Error::Pickle(format!(
                            "se guarda una clave en un {}, que no es un diccionario",
                            other.kind()
                        )));
                    }
                }
            }
            b'c' => {
                let module = l.line()?.to_owned();
                let name = l.line()?.to_owned();
                stack.push(V::Class(Class::from_names(&module, &name)?));
            }
            0x93 => {
                let name = pop!();
                let module = pop!();
                let (V::Text(module), V::Text(name)) = (&module, &name) else {
                    return Err(corrupt("STACK_GLOBAL requires str"));
                };
                stack.push(V::Class(Class::from_names(module, name)?));
            }
            b'i' => {
                let module = l.line()?.to_owned();
                let name = l.line()?.to_owned();
                let cls = Class::from_names(&module, &name)?;
                let args = pop_to_mark!();
                stack.push(call(&V::Class(cls), &args)?);
            }
            b'o' => {
                let mut items = pop_to_mark!();
                if items.is_empty() {
                    return Err(corrupt("could not find MARK"));
                }
                let cls = items.remove(0);
                stack.push(call(&cls, &items)?);
            }
            b'R' => {
                let args = pop!();
                let f = pop!();
                let V::Tuple(args) = args else {
                    return Err(corrupt("REDUCE argument must be a tuple"));
                };
                stack.push(call(&f, &args)?);
            }
            0x81 | 0x92 => {
                if op == 0x92 {
                    pop!(); // los argumentos con nombre
                }
                let args = pop!();
                let cls = pop!();
                let V::Tuple(args) = args else {
                    return Err(corrupt("NEWOBJ expected an arg tuple"));
                };
                stack.push(match cls {
                    // `OrderedDict.__new__`: un diccionario vacío
                    V::Class(Class::OrderedDict) => V::Dict(Rc::new(RefCell::new(Vec::new()))),
                    V::Class(Class::Size) => call(&cls, &args)?,
                    other => {
                        return Err(Error::Pickle(format!(
                            "un {} no es una clase",
                            other.kind()
                        )));
                    }
                });
            }
            b'b' => {
                let state = pop!();
                match top!() {
                    // el `_metadata` de un state_dict: un atributo, no un peso
                    V::Dict(_) => {}
                    other => {
                        return Err(Error::Pickle(format!(
                            "se le da estado a un {} ({})",
                            other.kind(),
                            state.kind()
                        )));
                    }
                }
            }
            b'Q' => {
                let pid = pop!();
                stack.push(V::Storage(zip.storage(&pid)?));
            }
            b'P' => {
                l.line()?;
                return Err(checkpoint("almacen desconocido en el checkpoint"));
            }
            0x82..=0x84 => return Err(Error::Pickle("un código de extensión desconocido".into())),
            b'g' => {
                let n: u64 = l.line()?.trim().parse().map_err(|_| corrupt("bad GET"))?;
                stack.push(
                    memo.get(&n)
                        .cloned()
                        .ok_or_else(|| corrupt(format!("Memo value not found at index {n}")))?,
                );
            }
            b'h' => {
                let n = u64::from(l.u8()?);
                stack.push(
                    memo.get(&n)
                        .cloned()
                        .ok_or_else(|| corrupt(format!("Memo value not found at index {n}")))?,
                );
            }
            b'j' => {
                let n = u64::from(u32::from_le_bytes(l.le()?));
                stack.push(
                    memo.get(&n)
                        .cloned()
                        .ok_or_else(|| corrupt(format!("Memo value not found at index {n}")))?,
                );
            }
            b'p' => {
                let n: u64 = l.line()?.trim().parse().map_err(|_| corrupt("bad PUT"))?;
                memo.insert(n, top!().clone());
            }
            b'q' => {
                let n = u64::from(l.u8()?);
                memo.insert(n, top!().clone());
            }
            b'r' => {
                let n = u64::from(u32::from_le_bytes(l.le()?));
                memo.insert(n, top!().clone());
            }
            0x94 => {
                let n = memo.len() as u64;
                memo.insert(n, top!().clone());
            }
            other => {
                return Err(corrupt(format!(
                    "invalid load key, '{}'.",
                    char::from(other).escape_default()
                )));
            }
        }
    }
}

// ------------------------------------------------------------ todo junto

/// Los tensores del checkpoint, en su orden: `[(name, tensor)]`. Si el
/// checkpoint guarda el modelo dentro de otra clave (`state_dict`, `model`),
/// se dice con `key` o se busca solo.
pub fn read(path: &Path, key: Option<&str>) -> Result<Vec<(String, Rc<Tensor>)>, Error> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = File::open(path).map_err(|e| Error::Io(e.kind(), e.to_string()))?;
    let mut file = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(ZipError::Io(e)) if e.kind() != std::io::ErrorKind::UnexpectedEof => {
            return Err(Error::Io(e.kind(), e.to_string()));
        }
        Err(_) => {
            return Err(checkpoint(format!(
                "{name} no es un checkpoint de PyTorch moderno"
            )));
        }
    };
    let pkl = (0..file.len())
        .filter_map(|i| {
            file.name_for_index(i)
                .and_then(Result::ok)
                .map(std::borrow::Cow::into_owned)
        })
        .find(|n| n.ends_with("/data.pkl") || n == "data.pkl")
        .ok_or_else(|| checkpoint(format!("{name} no trae data.pkl")))?;
    let prefix = pkl[..pkl.len() - "data.pkl".len()].to_owned();
    let mut data = Vec::new();
    file.by_name(&pkl)
        .map_err(|_| checkpoint("zip dañado"))?
        .read_to_end(&mut data)
        .map_err(|e| Error::Io(e.kind(), e.to_string()))?;
    let mut zip = Zip {
        file,
        prefix,
        already_read: HashMap::new(),
    };
    let mut obj = unpack(&data, &mut zip)?;
    if let Some(key) = key {
        let V::Dict(d) = &obj else {
            return Err(Error::Pickle(format!(
                "lo guardado es un {}, no un diccionario de pesos",
                obj.kind()
            )));
        };
        let k = V::Text(key.into());
        let v = d
            .borrow()
            .iter()
            .find(|(c, _)| c.equals(&k))
            .map(|(_, v)| v.clone());
        obj = v.ok_or_else(|| Error::Checkpoint(format!("no trae «{key}»")))?;
    } else if let V::Dict(d) = &obj {
        let no_tensors = !d.borrow().iter().any(|(_, v)| matches!(v, V::Tensor(_)));
        if no_tensors {
            let inside = ["state_dict", "model", "model_state_dict"]
                .iter()
                .find_map(|c| {
                    let k = V::Text((*c).into());
                    d.borrow()
                        .iter()
                        .find(|(key, v)| key.equals(&k) && matches!(v, V::Dict(_)))
                        .map(|(_, v)| v.clone())
                });
            if let Some(inside) = inside {
                obj = inside;
            }
        }
    }
    let V::Dict(d) = obj else {
        return Err(checkpoint(format!("{name} no es un diccionario de pesos")));
    };
    let weights = d
        .borrow()
        .iter()
        .filter_map(|(k, v)| match v {
            V::Tensor(t) => Some((k.text(), Rc::clone(t))),
            _ => None,
        })
        .collect();
    Ok(weights)
}

// ------------------------------------------------------------ safetensors

/// Un tensor de un `.safetensors`: dónde están sus números en el archivo
/// (desde el principio) y su forma.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TensorSt {
    pub name: String,
    pub kind: Type,
    pub start: u64,
    pub end: u64,
    pub shape: Vec<u64>,
}

/// Los tensores de un `.safetensors`, sin la biblioteca: una cabecera JSON y
/// los datos crudos detrás. No se lee ningún número (se mapean después).
pub fn safetensors(path: &Path) -> Result<Vec<TensorSt>, Error> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let not_safetensors = || Error::Checkpoint(format!("{name} no es un safetensors"));
    let mut f = File::open(path).map_err(|e| Error::Io(e.kind(), e.to_string()))?;
    let mut length = [0u8; 8];
    let already_read = f
        .read(&mut length)
        .map_err(|e| Error::Io(e.kind(), e.to_string()))?;
    let size = u64::from_le_bytes(length);
    if already_read < 8 || size == 0 || size > 50_000_000 {
        return Err(not_safetensors());
    }
    let mut header = vec![0u8; size as usize];
    f.read_exact(&mut header).map_err(|_| not_safetensors())?;
    let json: serde_json::Value =
        serde_json::from_slice(&header).map_err(|e| Error::Checkpoint(e.to_string()))?;
    let serde_json::Value::Object(header) = json else {
        return Err(Error::Checkpoint(format!("{name} no es un safetensors")));
    };
    let data = 8 + size;
    let mut out = Vec::new();
    for (k, meta) in header {
        if k == "__metadata__" {
            continue;
        }
        let dtype = meta
            .get("dtype")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let kind = match dtype {
            "F32" => Type::F32,
            "F16" => Type::F16,
            "I64" => Type::I64,
            "I32" => Type::I32,
            other => {
                return Err(Error::Checkpoint(format!(
                    "tipo {other} desconocido en {name}"
                )));
            }
        };
        let pos: Vec<u64> = meta
            .get("data_offsets")
            .and_then(serde_json::Value::as_array)
            .map(|v| v.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default();
        let shape: Vec<u64> = meta
            .get("shape")
            .and_then(serde_json::Value::as_array)
            .map(|v| v.iter().filter_map(serde_json::Value::as_u64).collect())
            .unwrap_or_default();
        let [a, z] = pos[..] else {
            return Err(not_safetensors());
        };
        let count: u64 = shape.iter().product();
        if z < a || (z - a) != count * kind.bytes() {
            return Err(Error::Pickle(format!(
                "{} números no caben en la forma {shape:?}",
                (z.saturating_sub(a)) / kind.bytes()
            )));
        }
        out.push(TensorSt {
            name: k,
            kind,
            start: data + a,
            end: data + z,
            shape,
        });
    }
    Ok(out)
}

// ------------------------------------------------------------ a float32

fn a_f32(kind: Type, b: &[u8]) -> f32 {
    match kind {
        Type::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        Type::F16 => half::f16::from_le_bytes([b[0], b[1]]).to_f32(),
        Type::BF16 => f32::from_bits(u32::from(u16::from_le_bytes([b[0], b[1]])) << 16),
        Type::F64 => f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32,
        Type::I64 => i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32,
        Type::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
        Type::I16 => f32::from(i16::from_le_bytes([b[0], b[1]])),
        Type::I8 => f32::from(b[0] as i8),
        Type::U8 => f32::from(b[0]),
        Type::Bool => f32::from(u8::from(b[0] != 0)),
    }
}

fn read_at(file: &mut File, start: u64, bytes: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(start))?;
    let mut out = vec![0u8; usize::try_from(bytes).map_err(std::io::Error::other)?];
    file.read_exact(&mut out)?;
    Ok(out)
}

/// Los números de un tensor de un checkpoint, en float32 y en orden C (como
/// `astype(float32)` de la vista de numpy): solo se lee lo que ocupa.
pub fn materialize(file: &mut File, t: &Tensor) -> std::io::Result<(Vec<usize>, Vec<f32>)> {
    let a = &t.storage;
    let item = a.kind.bytes();
    let shape: Vec<usize> = t.shape.iter().map(|n| *n as usize).collect();
    let count: usize = shape.iter().product();
    if count == 0 {
        return Ok((shape, Vec::new()));
    }
    // el tramo del almacén que toca la vista
    let last = t
        .shape
        .iter()
        .zip(&t.steps)
        .map(|(n, p)| (n - 1) * p)
        .sum::<u64>()
        + t.storage_offset;
    let raw = read_at(
        file,
        a.start + t.storage_offset * item,
        (last - t.storage_offset + 1) * item,
    )?;
    let mut out = Vec::with_capacity(count);
    let mut idx = vec![0usize; shape.len()];
    for _ in 0..count {
        let pos: u64 = idx.iter().zip(&t.steps).map(|(i, p)| *i as u64 * p).sum();
        let b = (pos * item) as usize;
        out.push(a_f32(a.kind, &raw[b..b + item as usize]));
        // el siguiente índice, el último eje el más rápido
        for d in (0..idx.len()).rev() {
            idx[d] += 1;
            if idx[d] < shape[d] {
                break;
            }
            idx[d] = 0;
        }
    }
    Ok((shape, out))
}

/// Los números de un tensor de un `.safetensors`, en float32.
pub fn materialize_st(file: &mut File, t: &TensorSt) -> std::io::Result<(Vec<usize>, Vec<f32>)> {
    let raw = read_at(file, t.start, t.end - t.start)?;
    let item = t.kind.bytes() as usize;
    let data = raw.chunks_exact(item).map(|b| a_f32(t.kind, b)).collect();
    Ok((t.shape.iter().map(|n| *n as usize).collect(), data))
}

#[cfg(test)]
mod tests;
