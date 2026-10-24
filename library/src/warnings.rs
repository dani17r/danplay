//! Lo que una función quiere dejar apuntado en el registro («no pude borrar
//! la miniatura de...»). La biblioteca no escribe registros: los devuelve,
//! y quien la llama los apunta en el suyo (con el mismo nivel y el mismo
//! nombre de módulo que antes).

/// Lo grave que es: para saberlo (`Info`) o porque algo salió mal (`Warning`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub level: Level,
    pub text: String,
}

/// Los avisos de una llamada, en orden.
pub type Warnings = Vec<Warning>;

pub fn warning(warnings: &mut Warnings, text: impl Into<String>) {
    warnings.push(Warning {
        level: Level::Warning,
        text: text.into(),
    });
}

pub fn info(warnings: &mut Warnings, text: impl Into<String>) {
    warnings.push(Warning {
        level: Level::Info,
        text: text.into(),
    });
}
