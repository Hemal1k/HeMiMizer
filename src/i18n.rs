//! Languages: English by default, Czech when the display language of Windows is Czech.


/// True when Windows shows its texts in Czech (primary language 0x05).
/// HeMimizer speaks English only (the user, 2026-11-15); the Czech texts stay in the code but are not shown.
pub fn cz() -> bool {
    false
}

/// The text in the language of the app.
pub fn tr<'a>(en: &'a str, cs: &'a str) -> &'a str {
    if cz() { cs } else { en }
}

