//! Las pruebas de la API, en el mismo proceso: cada una sobre su biblioteca
//! de verdad en carpetas temporales (`testkit`). Un archivo por tema.
#![allow(
    clippy::too_many_lines,
    reason = "cada prueba recorre una ruta entera, de punta a punta"
)]
mod ai;
mod assistant;
mod chat;
mod chat_flow;
mod chat_openai;
mod cues;
mod downloads;
mod external;
mod files;
mod folders;
mod guard;
mod index;
mod jobs;
mod journal;
mod library;
mod plan;
mod privacy;
mod sequence;
mod songs;
mod stems;
mod structure;
mod terms;
mod tidy;
mod watch;
mod youtube;
