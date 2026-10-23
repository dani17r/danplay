//! La hora de aquí (lo que en Python daba `time.localtime()`): la hoja del
//! atril lleva la fecha del día y el paquete del celular fecha lo de dentro.
//!
//! Sin bibliotecas de fechas: la diferencia con UTC sale del archivo de zona
//! del sistema (TZif: `TZ`, o /etc/localtime), la de la última transición
//! que ya pasó. Donde no lo hay (Windows), UTC.
use std::path::PathBuf;

fn be32(b: &[u8], at: usize) -> Option<i64> {
    let s = b.get(at..at + 4)?;
    Some(i64::from(i32::from_be_bytes([s[0], s[1], s[2], s[3]])))
}

fn be64(b: &[u8], at: usize) -> Option<i64> {
    let s = b.get(at..at + 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Some(i64::from_be_bytes(a))
}

fn count(b: &[u8], at: usize) -> Option<usize> {
    let s = b.get(at..at + 4)?;
    usize::try_from(u32::from_be_bytes([s[0], s[1], s[2], s[3]])).ok()
}

/// La diferencia con UTC (segundos) en `now` según un TZif.
pub fn offset_in(tzif: &[u8], now: i64) -> Option<i64> {
    if tzif.get(..4)? != b"TZif" {
        return None;
    }
    let header = |base: usize| -> Option<[usize; 6]> {
        Some([
            count(tzif, base + 20)?,
            count(tzif, base + 24)?,
            count(tzif, base + 28)?,
            count(tzif, base + 32)?,
            count(tzif, base + 36)?,
            count(tzif, base + 40)?,
        ])
    };
    let [isut, isstd, leap, time, types, chars] = header(0)?;
    let version = *tzif.get(4)?;
    let (base, size) = if version >= b'2' {
        // el bloque de la v1 (tiempos de 4 bytes) se salta: el bueno es el de 8
        let v1 = 44 + time * 5 + types * 6 + chars + leap * 8 + isstd + isut;
        (v1, 8)
    } else {
        (0, 4)
    };
    let [_, _, _, time, types, _] = header(base)?;
    let times = base + 44;
    let indices = times + time * size;
    let infos = indices + time;
    let read_time = |i: usize| {
        if size == 8 {
            be64(tzif, times + i * 8)
        } else {
            be32(tzif, times + i * 4)
        }
    };
    let mut kind = 0usize;
    for i in 0..time {
        if read_time(i)? > now {
            break;
        }
        kind = usize::from(*tzif.get(indices + i)?);
    }
    if kind >= types {
        return None;
    }
    be32(tzif, infos + kind * 6)
}

fn zone_file() -> PathBuf {
    match std::env::var("TZ") {
        Ok(tz) if !tz.is_empty() => {
            let tz = tz.strip_prefix(':').unwrap_or(&tz);
            if tz.starts_with('/') {
                PathBuf::from(tz)
            } else {
                PathBuf::from("/usr/share/zoneinfo").join(tz)
            }
        }
        _ => PathBuf::from("/etc/localtime"),
    }
}

/// `time.localtime().tm_gmtoff`: la diferencia con UTC ahora mismo.
pub fn utc_offset() -> i64 {
    let now = library::index::now() as i64;
    std::fs::read(zone_file())
        .ok()
        .and_then(|b| offset_in(&b, now))
        .unwrap_or(0)
}

/// Día, mes y año de unos segundos desde 1970 (ya en hora de aquí).
pub fn civil(secs: i64) -> (i64, i64, i64) {
    // el algoritmo de Howard Hinnant (days_from_civil al revés)
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (d, m, if m <= 2 { y + 1 } else { y })
}

/// `time.strftime("%d/%m/%Y")`: la fecha de hoy, aquí.
pub fn today_dmy() -> String {
    let local = library::index::now() as i64 + utc_offset();
    let (d, m, y) = civil(local);
    format!("{d:02}/{m:02}/{y}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil(0), (1, 1, 1970));
        assert_eq!(civil(1_792_627_200), (22, 10, 2026));
        assert_eq!(civil(951_782_400), (29, 2, 2000));
    }

    #[test]
    fn the_system_zone_reads() {
        // en cualquier sistema con zona, algo razonable (±14 h)
        assert!(utc_offset().abs() <= 14 * 3600);
    }
}
