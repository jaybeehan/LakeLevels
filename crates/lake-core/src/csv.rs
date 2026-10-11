//! A small CSV reader (quoted fields, doubled quotes, CRLF, a leading byte-order mark).

pub struct Csv {
    pub head: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Csv {
    pub fn parse(text: &str) -> Csv {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut records = Vec::new();
        let mut row = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if quoted {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        field.push('"');
                        chars.next();
                    } else {
                        quoted = false;
                    }
                } else {
                    field.push(c);
                }
                continue;
            }
            match c {
                '"' => quoted = true,
                ',' => row.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    if !(row.len() == 1 && row[0].is_empty()) {
                        records.push(std::mem::take(&mut row));
                    }
                    row.clear();
                }
                _ => field.push(c),
            }
        }
        if !field.is_empty() || !row.is_empty() {
            row.push(field);
            records.push(row);
        }
        let mut it = records.into_iter();
        let head = it.next().unwrap_or_default();
        Csv { head, rows: it.collect() }
    }

    /// Column number for a header name.
    pub fn col(&self, name: &str) -> Option<usize> {
        self.head.iter().position(|h| h.trim() == name)
    }

    /// Field `c` of `row`, trimmed ("" if missing).
    pub fn get<'a>(row: &'a [String], c: Option<usize>) -> &'a str {
        c.and_then(|c| row.get(c)).map(|s| s.trim()).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let c = Csv::parse("\u{feff}x,y,NAME\r\n1,2,\"A, \"\"B\"\"\"\r\n\r\n3,4,C");
        assert_eq!(c.head, vec!["x", "y", "NAME"]);
        assert_eq!(c.rows.len(), 2);
        assert_eq!(c.rows[0][2], "A, \"B\"");
        assert_eq!(Csv::get(&c.rows[1], c.col("NAME")), "C");
        assert_eq!(Csv::get(&c.rows[1], c.col("MISSING")), "");
    }
}
