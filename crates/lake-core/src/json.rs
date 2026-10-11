//! Just enough JSON writing for the files and results this project produces.

/// Round to `n` decimal places, rounding the exact value like Python's `round`
/// (so results match the original Python version to the last digit).
pub fn round(x: f64, n: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.n$}").parse().unwrap_or(x)
}

/// A number, or `null` if it isn't finite.
pub fn num(x: f64) -> String {
    if x.is_finite() {
        let s = format!("{x}");
        if s == "-0" { "0".into() } else { s }
    } else {
        "null".into()
    }
}

pub fn opt(x: Option<f64>) -> String {
    x.map(num).unwrap_or_else(|| "null".into())
}

/// A quoted, escaped string (non-ASCII left as UTF-8).
pub fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `[a,b,c]` from already-encoded items.
pub fn array<I: IntoIterator<Item = String>>(items: I) -> String {
    let v: Vec<String> = items.into_iter().collect();
    format!("[{}]", v.join(","))
}

/// `{"k":v,...}` from (key, already-encoded value) pairs.
pub fn object(pairs: &[(&str, String)]) -> String {
    let v: Vec<String> = pairs.iter().map(|(k, v)| format!("{}:{}", string(k), v)).collect();
    format!("{{{}}}", v.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes() {
        assert_eq!(num(551.234), "551.234");
        assert_eq!(num(552.0), "552");
        assert_eq!(num(f64::NAN), "null");
        assert_eq!(num(round(-0.0004, 3)), "0");
        assert_eq!(string("Rivière \"A\"\n"), "\"Rivière \\\"A\\\"\\n\"");
        assert_eq!(object(&[("a", num(1.5)), ("b", array(vec![opt(None), opt(Some(2.0))]))]), "{\"a\":1.5,\"b\":[null,2]}");
    }
}
