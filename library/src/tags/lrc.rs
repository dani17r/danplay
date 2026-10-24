//! La letra sin las marcas de tiempo de un LRC.
//!
//! Lo que va en el USLT del archivo y en la hoja del atril es la letra
//! limpia: con las marcas, la hoja imprimía «[00:12.34]» delante de cada
//! verso. Una marca es «[mm:ss]» o «[mm:ss.cc]» (con cifras de cualquier
//! escritura), y una línea de cabecera, «[ar:Barak]» o «[length: 3:42]» sola
//! en su línea.
use regex::Regex;
use std::sync::LazyLock;

static STAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\d{1,3}:\d{1,2}(?:[.:]\d{1,3})?\]").expect("la marca"));
static HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*\[[a-z]{1,8}:[^\]]*\]\s*$").expect("la cabecera"));

/// Si el texto lleva alguna marca de tiempo.
pub fn has_stamp(text: &str) -> bool {
    STAMP.is_match(text)
}

/// La letra sin las marcas de tiempo de un LRC. Un texto que no es LRC sale
/// tal cual.
pub fn lrc_to_plain(text: &str) -> String {
    if !has_stamp(text) {
        return text.to_owned();
    }
    let lines: Vec<String> = text
        .lines()
        .filter(|line| !HEADER.is_match(line))
        .map(|line| STAMP.replace_all(line, "").trim().to_owned())
        .collect();
    lines.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_stamps_and_headers_out() {
        assert_eq!(
            lrc_to_plain("[ar:Barak]\n[00:01.00]Mi gozo\n[00:02.00]en ti\n"),
            "Mi gozo\nen ti"
        );
        assert_eq!(lrc_to_plain("[length: 3:42]\n[1:2]a [01:02:03]b"), "a b");
        assert_eq!(lrc_to_plain("[00:01.00]a\r\n[00:02.00]b"), "a\nb");
        // sin marcas no es un LRC: tal cual, cabeceras y espacios incluidos
        assert_eq!(
            lrc_to_plain("[ar:Barak]\n  verso  "),
            "[ar:Barak]\n  verso  "
        );
        // las cifras de otras escrituras también valen
        assert_eq!(lrc_to_plain("[٠١:٠٢]verso"), "verso");
        assert_eq!(lrc_to_plain("[1234:00]x [00:01]y"), "[1234:00]x y");
        assert_eq!(lrc_to_plain("[00:01.1234]x [00:01]y"), "[00:01.1234]x y");
        // las cabeceras, sin mirar mayúsculas y de una a ocho letras
        assert_eq!(lrc_to_plain("[AR:x]\n[\u{212a}ey:x]\n[00:01]z"), "z");
        assert_eq!(lrc_to_plain("[abcdefghi:x]\n[00:01]z"), "[abcdefghi:x]\nz");
    }
}
