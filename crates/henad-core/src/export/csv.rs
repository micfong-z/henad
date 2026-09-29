//! Comma-separated values (CSV) fields, written and read as RFC 4180 describes them.

use std::fmt;

/// Integral values lose the trailing `.0`, everything else keeps full round-trip precision.
///
/// Non-finite values become empty cells, since `NaN` and `inf` are not valid numbers to most
/// readers and an empty cell is the conventional missing marker.
pub fn fmt_f64(value: f64) -> String {
    if !value.is_finite() {
        String::new()
    } else if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

/// Quote a CSV field if it contains a comma, quote, or newline, doubling any inner quotes.
///
/// Stat labels are `&'static str` from model source, so this is belt-and-braces. A label with a
/// comma in it would otherwise silently shift every column right of it.
pub fn escape_field(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// Text that is not valid CSV. Each line number counts from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvError {
    /// A quoted field opened on `line` is still open at the end of the text.
    UnterminatedQuote { line: usize },
    /// A quote inside an unquoted field, or text after a closing quote.
    MisplacedQuote { line: usize },
}

impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnterminatedQuote { line } => write!(f, "the quoted field opened on line {line} is never closed"),
            Self::MisplacedQuote { line } => write!(f, "misplaced quote on line {line}"),
        }
    }
}

impl std::error::Error for CsvError {}

/// Position of the reader within the current field.
#[derive(Clone, Copy)]
enum FieldState {
    Start,
    Unquoted,
    Quoted,
    QuoteClosed,
}

/// Splits `text` into records of fields, undoing [`escape_field`].
///
/// A record ends at a CRLF or an LF outside quotes, and the final line ending is optional. A
/// quoted field keeps its commas and line endings, and a doubled quote inside it reads as one.
/// Note that a blank line is a record holding one empty field.
///
/// # Errors
///
/// Returns [`CsvError`] when a quote is never closed or sits anywhere but around a whole field.
pub fn parse_records(text: &str) -> Result<Vec<Vec<String>>, CsvError> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut state = FieldState::Start;
    let mut line = 1;
    let mut quote_line = 1;
    let mut chars = text.chars().peekable();

    while let Some(character) = chars.next() {
        match (state, character) {
            (FieldState::Quoted, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (FieldState::Quoted, '"') => state = FieldState::QuoteClosed,
            (FieldState::Quoted, _) => {
                if character == '\n' {
                    line += 1;
                }
                field.push(character);
            }
            (FieldState::Start, '"') => {
                state = FieldState::Quoted;
                quote_line = line;
            }
            (_, ',') => {
                record.push(std::mem::take(&mut field));
                state = FieldState::Start;
            }
            (_, '\n') => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                state = FieldState::Start;
                line += 1;
            }
            (_, '\r') if chars.peek() == Some(&'\n') => {
                chars.next();
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                state = FieldState::Start;
                line += 1;
            }
            (FieldState::Unquoted | FieldState::QuoteClosed, '"') | (FieldState::QuoteClosed, _) => {
                return Err(CsvError::MisplacedQuote { line });
            }
            (FieldState::Start | FieldState::Unquoted, _) => {
                field.push(character);
                state = FieldState::Unquoted;
            }
        }
    }

    match state {
        FieldState::Quoted => return Err(CsvError::UnterminatedQuote { line: quote_line }),
        // The text is empty or ends on a line ending, so no record is open.
        FieldState::Start if record.is_empty() => {}
        FieldState::Start | FieldState::Unquoted | FieldState::QuoteClosed => {
            record.push(field);
            records.push(record);
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::{CsvError, escape_field, fmt_f64, parse_records};

    fn records(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| row.iter().map(|&field| field.to_owned()).collect())
            .collect()
    }

    #[test]
    fn plain_fields_split_at_commas_and_line_endings() {
        let expected = records(&[&["tick", "A", "B"], &["0", "1", "2"]]);
        assert_eq!(parse_records("tick,A,B\n0,1,2\n"), Ok(expected.clone()));
        assert_eq!(parse_records("tick,A,B\r\n0,1,2\r\n"), Ok(expected.clone()), "CRLF");
        assert_eq!(parse_records("tick,A,B\r\n0,1,2\n"), Ok(expected), "mixed endings");
    }

    #[test]
    fn the_final_line_ending_is_optional() {
        let expected = records(&[&["a", "b"], &["1", "2"]]);
        assert_eq!(parse_records("a,b\n1,2"), Ok(expected.clone()));
        assert_eq!(parse_records("a,b\n1,2\n"), Ok(expected.clone()));
        assert_eq!(parse_records("a,b\r\n1,2\r\n"), Ok(expected));
        assert_eq!(parse_records(""), Ok(Vec::new()), "no text, no records");
    }

    #[test]
    fn empty_fields_and_blank_lines_are_kept() {
        assert_eq!(parse_records("a,,c\n"), Ok(records(&[&["a", "", "c"]])));
        assert_eq!(parse_records("a,\n"), Ok(records(&[&["a", ""]])), "a trailing comma");
        assert_eq!(parse_records("a\n\nb\n"), Ok(records(&[&["a"], &[""], &["b"]])));
        assert_eq!(parse_records("\"\"\n"), Ok(records(&[&[""]])), "a quoted empty field");
    }

    #[test]
    fn quoted_fields_keep_commas_quotes_and_line_endings() {
        let text = "\"Susceptible, count\",\"say \"\"hi\"\"\",\"two\nlines\",\"crlf\r\nkept\"\n1,2,3,4\n";
        let expected = records(&[
            &["Susceptible, count", "say \"hi\"", "two\nlines", "crlf\r\nkept"],
            &["1", "2", "3", "4"],
        ]);
        assert_eq!(parse_records(text), Ok(expected));
    }

    #[test]
    fn an_escaped_field_reads_back_as_itself() {
        let fields = [
            "plain",
            "a,b",
            "say \"hi\"",
            "two\nlines",
            "cr\ronly",
            "",
            "\"",
            "Speed.[0, 1)",
        ];
        let line: Vec<String> = fields.iter().map(|&field| escape_field(field)).collect();
        let text = format!("{}\n", line.join(","));
        assert_eq!(parse_records(&text), Ok(records(&[&fields])));
    }

    #[test]
    fn an_unterminated_quote_is_refused() {
        assert_eq!(
            parse_records("a,b\n1,\"open\n2,3\n"),
            Err(CsvError::UnterminatedQuote { line: 2 })
        );
    }

    #[test]
    fn a_misplaced_quote_is_refused() {
        assert_eq!(parse_records("a\"b\n"), Err(CsvError::MisplacedQuote { line: 1 }));
        assert_eq!(
            parse_records("x\n\"closed\"early\n"),
            Err(CsvError::MisplacedQuote { line: 2 })
        );
    }

    #[test]
    fn numbers_format_as_the_stats_file_writes_them() {
        assert_eq!(fmt_f64(3.0), "3");
        assert_eq!(fmt_f64(-0.5), "-0.5");
        assert_eq!(fmt_f64(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(fmt_f64(1e15), "1000000000000000");
        assert_eq!(fmt_f64(f64::NAN), "");
        assert_eq!(fmt_f64(f64::NEG_INFINITY), "");
    }
}
