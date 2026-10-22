//! La conexión al índice (`biblioteca::indice`, rusqlite) para
//! danplay/library/db.py, que antes abría la base con `sqlite3`.
//!
//! Imita lo que el núcleo usaba de `sqlite3`: `execute`, `executemany`,
//! `executescript`, `commit`, `rollback`, `with` (que confirma o deshace y
//! cierra), cursores con `fetchone`/`fetchall`/`lastrowid`/`rowcount` y filas
//! que se leen por posición o por nombre, como `sqlite3.Row`. También sus
//! transacciones implícitas: un INSERT, UPDATE, DELETE o REPLACE abre una
//! (`BEGIN`) si no hay ninguna, y `commit` la cierra. Los errores son los de
//! `sqlite3` (`IntegrityError`, `OperationalError`...), y un `except
//! sqlite3.Error` los sigue cogiendo.
//!
//! Así Python y lo que ya está en Rust comparten la MISMA conexión, con sus
//! transacciones: un módulo pasa a Rust sin que nada más cambie. Y en todo el
//! proceso solo un SQLite abre el índice (dos copias de SQLite en un proceso
//! no ven los cerrojos de la otra).
use biblioteca::rusqlite;
use pyo3::exceptions::{PyIndexError, PyOverflowError, PyTypeError};
use pyo3::import_exception;
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyDict, PyFloat, PyInt, PyList, PySlice, PyString, PyTuple};
use rusqlite::ffi::ErrorCode;
use rusqlite::types::{Value, ValueRef};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

import_exception!(sqlite3, DatabaseError);
import_exception!(sqlite3, DataError);
import_exception!(sqlite3, IntegrityError);
import_exception!(sqlite3, InterfaceError);
import_exception!(sqlite3, InternalError);
import_exception!(sqlite3, OperationalError);
import_exception!(sqlite3, ProgrammingError);

// ------------------------------------------------------------------ errores

/// Lo que puede salir mal con el GIL suelto; se convierte en la excepción de
/// `sqlite3` al volver a Python.
enum Fallo {
    Sqlite(rusqlite::Error),
    Programacion(String),
    Operacion(String),
}

impl From<rusqlite::Error> for Fallo {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

/// La excepción que daría `sqlite3` para ese código de SQLite.
fn sqlite_error(code: ErrorCode, msg: String) -> PyErr {
    match code {
        ErrorCode::ConstraintViolation | ErrorCode::TypeMismatch => IntegrityError::new_err(msg),
        ErrorCode::InternalMalfunction | ErrorCode::NotFound => InternalError::new_err(msg),
        ErrorCode::OutOfMemory => pyo3::exceptions::PyMemoryError::new_err(msg),
        ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => DatabaseError::new_err(msg),
        ErrorCode::TooBig => DataError::new_err(msg),
        ErrorCode::ApiMisuse | ErrorCode::ParameterOutOfRange => InterfaceError::new_err(msg),
        _ => OperationalError::new_err(msg),
    }
}

impl From<Fallo> for PyErr {
    fn from(f: Fallo) -> Self {
        match f {
            Fallo::Programacion(msg) => ProgrammingError::new_err(msg),
            Fallo::Operacion(msg) => OperationalError::new_err(msg),
            Fallo::Sqlite(e) => match e {
                rusqlite::Error::SqliteFailure(err, msg) => {
                    let msg = msg.unwrap_or_else(|| err.to_string());
                    sqlite_error(err.code, msg)
                }
                rusqlite::Error::SqlInputError { error, msg, .. } => sqlite_error(error.code, msg),
                rusqlite::Error::MultipleStatement => {
                    ProgrammingError::new_err("You can only execute one statement at a time.")
                }
                other => DatabaseError::new_err(other.to_string()),
            },
        }
    }
}

// ------------------------------------------------------------------ valores

/// Los parámetros de una sentencia, ya en Rust: por posición (`?`) o por
/// nombre (`:nombre`), como los acepta `sqlite3`.
enum Parametros {
    Posicion(Vec<Value>),
    Nombre(Vec<(String, Value)>),
}

fn to_value(obj: &Bound<'_, PyAny>, index: usize) -> PyResult<Value> {
    if obj.is_none() {
        Ok(Value::Null)
    } else if obj.is_instance_of::<PyInt>() {
        // también `bool`, que es un `int`
        obj.extract::<i64>()
            .map(Value::Integer)
            .map_err(|_| PyOverflowError::new_err("Python int too large to convert to SQLite INTEGER"))
    } else if obj.is_instance_of::<PyFloat>() {
        Ok(Value::Real(obj.extract::<f64>()?))
    } else if let Ok(s) = obj.cast::<PyString>() {
        Ok(Value::Text(s.to_str()?.to_owned()))
    } else if let Ok(b) = obj.cast::<PyBytes>() {
        Ok(Value::Blob(b.as_bytes().to_vec()))
    } else if let Ok(b) = obj.cast::<PyByteArray>() {
        Ok(Value::Blob(b.to_vec()))
    } else {
        let kind = obj.get_type().name()?;
        Err(ProgrammingError::new_err(format!(
            "Error binding parameter {index}: type '{kind}' is not supported"
        )))
    }
}

fn parametros(params: Option<&Bound<'_, PyAny>>) -> PyResult<Parametros> {
    let Some(params) = params.filter(|p| !p.is_none()) else {
        return Ok(Parametros::Posicion(Vec::new()));
    };
    if let Ok(d) = params.cast::<PyDict>() {
        let mut out = Vec::with_capacity(d.len());
        for (k, v) in d.iter() {
            let key: String = k.extract()?;
            let value = to_value(&v, out.len() + 1)?;
            out.push((key, value));
        }
        return Ok(Parametros::Nombre(out));
    }
    let seq = params
        .try_iter()
        .map_err(|_| ProgrammingError::new_err("parameters are of unsupported type"))?;
    let mut out = Vec::new();
    for item in seq {
        out.push(to_value(&item?, out.len() + 1)?);
    }
    Ok(Parametros::Posicion(out))
}

fn bind(stmt: &mut rusqlite::Statement<'_>, params: &Parametros) -> Result<(), Fallo> {
    let wanted = stmt.parameter_count();
    match params {
        Parametros::Posicion(values) => {
            if values.len() != wanted {
                return Err(Fallo::Programacion(format!(
                    "Incorrect number of bindings supplied. The current statement uses {wanted}, and there are {} supplied.",
                    values.len()
                )));
            }
            for (i, v) in values.iter().enumerate() {
                stmt.raw_bind_parameter(i + 1, v)?;
            }
        }
        Parametros::Nombre(values) => {
            for i in 1..=wanted {
                let Some(name) = stmt.parameter_name(i).map(str::to_owned) else {
                    return Err(Fallo::Programacion(format!(
                        "Binding {i} has no name, but you supplied a dictionary (which has only names)."
                    )));
                };
                let key = &name[1..];
                let Some((_, v)) = values.iter().find(|(k, _)| k == key) else {
                    return Err(Fallo::Programacion(format!(
                        "You did not supply a value for binding parameter {name}."
                    )));
                };
                stmt.raw_bind_parameter(i, v)?;
            }
        }
    }
    Ok(())
}

/// Lo que SQLite devuelve, convertido ya (con el GIL suelto) en valores de
/// Rust. Un texto que no es UTF-8 es un error, como en `sqlite3`.
fn from_ref(v: ValueRef<'_>, column: &str) -> Result<Value, Fallo> {
    Ok(match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Integer(i),
        ValueRef::Real(f) => Value::Real(f),
        ValueRef::Text(t) => Value::Text(
            String::from_utf8(t.to_vec())
                .map_err(|_| Fallo::Operacion(format!("Could not decode to UTF-8 column '{column}'")))?,
        ),
        ValueRef::Blob(b) => Value::Blob(b.to_vec()),
    })
}

fn to_python(py: Python<'_>, v: Value) -> PyResult<Py<PyAny>> {
    Ok(match v {
        Value::Null => py.None(),
        Value::Integer(i) => i.into_pyobject(py)?.into_any().unbind(),
        Value::Real(f) => f.into_pyobject(py)?.into_any().unbind(),
        Value::Text(s) => s.into_pyobject(py)?.into_any().unbind(),
        Value::Blob(b) => PyBytes::new(py, &b).into_any().unbind(),
    })
}

// ------------------------------------------------------------ transacciones

/// Si la sentencia es de las que abren transacción en `sqlite3` (su modo de
/// siempre, el «legacy»): INSERT, UPDATE, DELETE o REPLACE, tras los
/// espacios y los comentarios del principio. Copiado de `lstrip_sql` de
/// CPython: un SELECT, un PRAGMA o un CREATE no abren nada.
fn opens_transaction(sql: &str) -> bool {
    let b = sql.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\x0c' | b'\n' | b'\r' => i += 1,
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i += 2;
            }
            _ => break,
        }
    }
    let rest = &b[i.min(b.len())..];
    let starts = |word: &str| rest.len() >= word.len() && rest[..word.len()].eq_ignore_ascii_case(word.as_bytes());
    starts("insert") || starts("update") || starts("delete") || starts("replace")
}

fn begin_if_needed(conn: &rusqlite::Connection, sql: &str) -> Result<bool, Fallo> {
    let dml = opens_transaction(sql);
    if dml && conn.is_autocommit() {
        conn.execute_batch("BEGIN ")?;
    }
    Ok(dml)
}

// ---------------------------------------------------------------- conexión

/// Una conexión al índice. Con `with`, confirma lo hecho (o lo deshace si
/// algo falló) y se cierra siempre, como `library.db.Connection`.
#[pyclass(frozen, module = "danplay_core")]
pub struct Connection {
    inner: Mutex<Option<rusqlite::Connection>>,
}

impl Connection {
    /// La conexión de Rust que hay debajo, para lo que ya está portado.
    /// Con el GIL suelto o no: el cerrojo es de la conexión.
    pub fn lock(&self) -> PyResult<ConnGuard<'_>> {
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.is_none() {
            return Err(ProgrammingError::new_err("Cannot operate on a closed database."));
        }
        Ok(ConnGuard(guard))
    }

    fn with<T: Send>(
        &self,
        py: Python<'_>,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T, Fallo> + Send,
    ) -> PyResult<T> {
        let inner = &self.inner;
        py.detach(|| {
            let guard = inner.lock().unwrap_or_else(PoisonError::into_inner);
            match guard.as_ref() {
                Some(conn) => f(conn).map_err(Some),
                None => Err(None),
            }
        })
        .map_err(|e| match e {
            Some(f) => f.into(),
            None => ProgrammingError::new_err("Cannot operate on a closed database."),
        })
    }
}

/// Lo que hace `f` con la conexión de Rust que hay dentro de `conn`, con el
/// GIL suelto; sus errores salen como los de `sqlite3`. Es como lo portado a
/// Rust usa la conexión que le pasa Python.
pub fn with_conn<T: Send>(
    py: Python<'_>,
    conn: &Bound<'_, Connection>,
    f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T> + Send,
) -> PyResult<T> {
    conn.get().with(py, |c| f(c).map_err(Fallo::Sqlite))
}

/// Un error de SQLite como la excepción de `sqlite3`, para lo de Rust que abre
/// su propia conexión (el emparejador, al consultar).
pub fn sqlite_err(e: rusqlite::Error) -> PyErr {
    Fallo::Sqlite(e).into()
}

/// La conexión prestada (ver `Connection::lock`).
pub struct ConnGuard<'a>(MutexGuard<'a, Option<rusqlite::Connection>>);

impl std::ops::Deref for ConnGuard<'_> {
    type Target = rusqlite::Connection;
    fn deref(&self) -> &rusqlite::Connection {
        // `lock` ya comprobó que está abierta
        self.0.as_ref().expect("conexión abierta")
    }
}

struct Resultado {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    rowcount: i64,
    lastrowid: Option<i64>,
}

fn run(conn: &rusqlite::Connection, sql: &str, params: &Parametros) -> Result<Resultado, Fallo> {
    let dml = begin_if_needed(conn, sql)?;
    let mut stmt = conn.prepare_cached(sql)?;
    bind(&mut stmt, params)?;
    let columns: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let mut rows = Vec::new();
    {
        let mut it = stmt.raw_query();
        while let Some(row) = it.next()? {
            let mut values = Vec::with_capacity(columns.len());
            for (i, name) in columns.iter().enumerate() {
                values.push(from_ref(row.get_ref(i)?, name)?);
            }
            rows.push(values);
        }
    }
    let rowcount = if dml {
        i64::try_from(conn.changes()).unwrap_or(i64::MAX)
    } else {
        -1
    };
    Ok(Resultado {
        columns,
        rows,
        rowcount,
        lastrowid: Some(conn.last_insert_rowid()),
    })
}

fn run_many(conn: &rusqlite::Connection, sql: &str, all: &[Parametros]) -> Result<i64, Fallo> {
    let dml = begin_if_needed(conn, sql)?;
    let mut stmt = conn.prepare_cached(sql)?;
    if stmt.readonly() {
        return Err(Fallo::Programacion(
            "executemany() can only execute DML statements.".into(),
        ));
    }
    let mut changed: i64 = 0;
    for params in all {
        bind(&mut stmt, params)?;
        let mut it = stmt.raw_query();
        while it.next()?.is_some() {}
        drop(it);
        changed += i64::try_from(conn.changes()).unwrap_or(0);
    }
    Ok(if dml { changed } else { -1 })
}

fn names_tuple(py: Python<'_>, columns: &[String]) -> PyResult<Py<PyTuple>> {
    Ok(PyTuple::new(py, columns)?.unbind())
}

#[pymethods]
impl Connection {
    #[pyo3(signature = (sql, parameters = None))]
    fn execute(&self, py: Python<'_>, sql: &str, parameters: Option<&Bound<'_, PyAny>>) -> PyResult<Cursor> {
        let params = parametros(parameters)?;
        let sql = sql.to_owned();
        let r = self.with(py, move |conn| run(conn, &sql, &params))?;
        Cursor::new(py, r)
    }

    fn executemany(&self, py: Python<'_>, sql: &str, seq_of_parameters: &Bound<'_, PyAny>) -> PyResult<Cursor> {
        let mut all = Vec::new();
        for item in seq_of_parameters.try_iter()? {
            all.push(parametros(Some(&item?))?);
        }
        let sql = sql.to_owned();
        let rowcount = self.with(py, move |conn| run_many(conn, &sql, &all))?;
        Ok(Cursor::empty(rowcount))
    }

    /// Confirma lo pendiente y ejecuta el guion entero, como `sqlite3`.
    fn executescript(&self, py: Python<'_>, sql_script: &str) -> PyResult<Cursor> {
        let script = sql_script.to_owned();
        self.with(py, move |conn| {
            if !conn.is_autocommit() {
                conn.execute_batch("COMMIT")?;
            }
            conn.execute_batch(&script)?;
            Ok(())
        })?;
        Ok(Cursor::empty(-1))
    }

    fn commit(&self, py: Python<'_>) -> PyResult<()> {
        self.with(py, |conn| {
            if !conn.is_autocommit() {
                conn.execute_batch("COMMIT")?;
            }
            Ok(())
        })
    }

    fn rollback(&self, py: Python<'_>) -> PyResult<()> {
        self.with(py, |conn| {
            if !conn.is_autocommit() {
                conn.execute_batch("ROLLBACK")?;
            }
            Ok(())
        })
    }

    /// Cerrarla deshace lo que no se confirmó. Cerrarla dos veces no pasa nada.
    fn close(&self, py: Python<'_>) {
        let inner = &self.inner;
        py.detach(|| drop(inner.lock().unwrap_or_else(PoisonError::into_inner).take()));
    }

    #[getter]
    fn in_transaction(&self, py: Python<'_>) -> PyResult<bool> {
        self.with(py, |conn| Ok(!conn.is_autocommit()))
    }

    #[getter]
    fn total_changes(&self, py: Python<'_>) -> PyResult<u64> {
        self.with(py, |conn| Ok(conn.total_changes()))
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    /// Como `sqlite3`: confirma si todo fue bien y deshace si no; y después,
    /// como `library.db.Connection`, la cierra siempre.
    #[pyo3(signature = (exc_type, _exc = None, _tb = None))]
    fn __exit__(
        &self,
        py: Python<'_>,
        exc_type: Option<&Bound<'_, PyAny>>,
        _exc: Option<&Bound<'_, PyAny>>,
        _tb: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        let ok = exc_type.is_none_or(PyAnyMethods::is_none);
        let result = if ok {
            self.commit(py).inspect_err(|_| {
                let _ = self.rollback(py);
            })
        } else {
            self.rollback(py)
        };
        self.close(py);
        result.map(|()| false)
    }
}

/// Abre el índice (ver `biblioteca::indice::abrir`).
#[pyfunction]
pub fn db_open(py: Python<'_>, path: PathBuf) -> PyResult<Connection> {
    let conn = py
        .detach(|| biblioteca::indice::abrir(&path))
        .map_err(|e| PyErr::from(Fallo::Sqlite(e)))?;
    Ok(Connection {
        inner: Mutex::new(Some(conn)),
    })
}

// ------------------------------------------------------------------ cursor

/// Las columnas de un resultado, compartidas por todas sus filas.
struct Columnas {
    names: Vec<String>,
    tuple: Py<PyTuple>,
}

/// El resultado de una sentencia. Las filas ya están leídas (SQLite trabaja
/// con el GIL suelto) y se convierten a Python al pedirlas.
#[pyclass(module = "danplay_core")]
pub struct Cursor {
    columns: Option<Arc<Columnas>>,
    rows: std::vec::IntoIter<Vec<Value>>,
    #[pyo3(get)]
    rowcount: i64,
    #[pyo3(get)]
    lastrowid: Option<i64>,
    #[pyo3(get, set)]
    arraysize: usize,
}

impl Cursor {
    fn new(py: Python<'_>, r: Resultado) -> PyResult<Self> {
        let columns = if r.columns.is_empty() {
            None
        } else {
            let tuple = names_tuple(py, &r.columns)?;
            Some(Arc::new(Columnas {
                names: r.columns,
                tuple,
            }))
        };
        Ok(Self {
            columns,
            rows: r.rows.into_iter(),
            rowcount: r.rowcount,
            lastrowid: r.lastrowid,
            arraysize: 1,
        })
    }

    fn empty(rowcount: i64) -> Self {
        Self {
            columns: None,
            rows: Vec::new().into_iter(),
            rowcount,
            lastrowid: None,
            arraysize: 1,
        }
    }

    fn row(&self, py: Python<'_>, values: Vec<Value>) -> PyResult<Row> {
        let columns = self.columns.clone().expect("una fila tiene columnas");
        let values: Vec<Py<PyAny>> = values.into_iter().map(|v| to_python(py, v)).collect::<PyResult<_>>()?;
        Ok(Row {
            columns,
            values: PyTuple::new(py, values)?.unbind(),
        })
    }
}

#[pymethods]
impl Cursor {
    fn fetchone(&mut self, py: Python<'_>) -> PyResult<Option<Row>> {
        self.rows.next().map(|v| self.row(py, v)).transpose()
    }

    fn fetchall(&mut self, py: Python<'_>) -> PyResult<Vec<Row>> {
        let rest: Vec<Vec<Value>> = self.rows.by_ref().collect();
        rest.into_iter().map(|v| self.row(py, v)).collect()
    }

    #[pyo3(signature = (size = None))]
    fn fetchmany(&mut self, py: Python<'_>, size: Option<usize>) -> PyResult<Vec<Row>> {
        let n = size.unwrap_or(self.arraysize);
        let some: Vec<Vec<Value>> = self.rows.by_ref().take(n).collect();
        some.into_iter().map(|v| self.row(py, v)).collect()
    }

    fn close(&mut self) {
        self.rows = Vec::new().into_iter();
    }

    /// `(nombre, None, None, None, None, None, None)` por columna, o `None`.
    #[getter]
    fn description<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyTuple>>> {
        let Some(columns) = &self.columns else { return Ok(None) };
        let items = columns
            .names
            .iter()
            .map(|n| {
                PyTuple::new(
                    py,
                    [
                        n.into_pyobject(py)?.into_any(),
                        py.None().into_bound(py),
                        py.None().into_bound(py),
                        py.None().into_bound(py),
                        py.None().into_bound(py),
                        py.None().into_bound(py),
                        py.None().into_bound(py),
                    ],
                )
            })
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Some(PyTuple::new(py, items)?))
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Row>> {
        self.fetchone(py)
    }
}

// -------------------------------------------------------------------- fila

/// Una fila: por posición (`fila[0]`), por nombre (`fila["id"]`, sin
/// distinguir mayúsculas, como `sqlite3.Row`), `keys()`, y `dict(fila)`.
#[pyclass(frozen, module = "danplay_core")]
pub struct Row {
    columns: Arc<Columnas>,
    values: Py<PyTuple>,
}

impl Row {
    fn index_of(&self, name: &str) -> Option<usize> {
        let names = &self.columns.names;
        names
            .iter()
            .position(|n| n == name)
            .or_else(|| names.iter().position(|n| n.eq_ignore_ascii_case(name)))
    }
}

#[pymethods]
impl Row {
    fn __getitem__<'py>(&self, py: Python<'py>, key: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let values = self.values.bind(py);
        if let Ok(name) = key.cast::<PyString>() {
            let i = self
                .index_of(name.to_str()?)
                .ok_or_else(|| PyIndexError::new_err("No item with that key"))?;
            return values.get_item(i);
        }
        if key.is_instance_of::<PyInt>() || key.is_instance_of::<PySlice>() {
            return values.as_any().get_item(key);
        }
        Err(PyTypeError::new_err("Index must be int or string"))
    }

    fn __len__(&self, py: Python<'_>) -> usize {
        self.values.bind(py).len()
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        Ok(self.values.bind(py).as_any().try_iter()?.into_any())
    }

    fn keys<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        PyList::new(py, self.columns.tuple.bind(py))
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        let Ok(other) = other.cast::<Row>() else {
            return Ok(false);
        };
        let other = other.get();
        Ok(self.columns.names == other.columns.names && self.values.bind(py).eq(other.values.bind(py))?)
    }

    fn __hash__(&self, py: Python<'_>) -> PyResult<isize> {
        let names = self.columns.tuple.bind(py).hash()?;
        Ok(names ^ self.values.bind(py).hash()?)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let pairs: Vec<String> = self
            .columns
            .names
            .iter()
            .zip(self.values.bind(py).iter())
            .map(|(n, v)| Ok(format!("{n}={}", v.repr()?)))
            .collect::<PyResult<_>>()?;
        Ok(format!("<Row {}>", pairs.join(", ")))
    }
}

/// Registra la conexión en el módulo.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Connection>()?;
    m.add_class::<Cursor>()?;
    m.add_class::<Row>()?;
    m.add_function(wrap_pyfunction!(db_open, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::opens_transaction;

    #[test]
    fn las_que_abren_transaccion_como_en_python() {
        assert!(opens_transaction("INSERT INTO t VALUES (1)"));
        assert!(opens_transaction("  \n\tupdate t SET a=1"));
        assert!(opens_transaction("-- comentario\nDELETE FROM t"));
        assert!(opens_transaction("/* uno */ /* dos */REPLACE INTO t VALUES (1)"));
        assert!(!opens_transaction("SELECT 1"));
        assert!(!opens_transaction("WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x"));
        assert!(!opens_transaction("PRAGMA user_version = 3"));
        assert!(!opens_transaction("BEGIN IMMEDIATE"));
        assert!(!opens_transaction("-- solo un comentario"));
        assert!(!opens_transaction("/* sin cerrar"));
        assert!(!opens_transaction("ins"));
    }
}
