//! Lo que una función quiere dejar apuntado en el registro («no pude borrar
//! la miniatura de...»). La biblioteca no escribe registros: los devuelve,
//! y quien la llama los apunta en el suyo (Python, en su `logging`, con el
//! mismo nivel y el mismo nombre de módulo que antes).

/// Lo grave que es: lo que en Python era `log.info` o `log.warning`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nivel {
    Info,
    Aviso,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Aviso {
    pub nivel: Nivel,
    pub texto: String,
}

/// Los avisos de una llamada, en orden.
pub type Avisos = Vec<Aviso>;

pub fn aviso(avisos: &mut Avisos, texto: impl Into<String>) {
    avisos.push(Aviso {
        nivel: Nivel::Aviso,
        texto: texto.into(),
    });
}

pub fn info(avisos: &mut Avisos, texto: impl Into<String>) {
    avisos.push(Aviso {
        nivel: Nivel::Info,
        texto: texto.into(),
    });
}
