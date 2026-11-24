//! Languages: HeMiMizer is shown in English only. The Czech texts stay next to the English ones in `tr`.


/// False: HeMiMizer speaks English only (the user, 2026-11-15); the Czech texts stay in the code but are not shown.
pub fn cz() -> bool {
    false
}

/// The text in the language of the app.
pub fn tr<'a>(en: &'a str, cs: &'a str) -> &'a str {
    if cz() { cs } else { en }
}

/// The Czech form for a count: 1 soubor / 2–4 soubory / 0, 5 and more souborů.
pub fn plural<'a>(n: usize, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    match n {
        1 => one,
        2..=4 => few,
        _ => many,
    }
}

/// A count with its noun in the language of the app: "1 folder", "3 folders" / "1 složka", "3 složky", "5 složek".
pub fn count(n: usize, en: (&str, &str), cs: (&str, &str, &str)) -> String {
    let word = if cz() { plural(n, cs.0, cs.1, cs.2) } else if n == 1 { en.0 } else { en.1 };
    format!("{n} {word}")
}

/// A number with one decimal place, with a decimal comma in Czech.
pub fn dec1(x: f64) -> String {
    let s = format!("{x:.1}");
    if cz() { s.replace('.', ",") } else { s }
}
