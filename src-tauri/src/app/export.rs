//! CSV and text rendering of what the client recorded.

use crate::app::trace::TraceEntry;

fn quote(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// RFC 4180 CSV: CRLF rows, quotes doubled, fields with a separator quoted.
pub fn to_csv(rows: &[Vec<(String, String)>], columns: &[&str]) -> String {
    let mut lines = vec![columns
        .iter()
        .map(|c| quote(c))
        .collect::<Vec<_>>()
        .join(",")];

    for row in rows {
        let line = columns
            .iter()
            .map(|c| {
                let value = row
                    .iter()
                    .find(|(k, _)| k == c)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("");
                quote(value)
            })
            .collect::<Vec<_>>()
            .join(",");
        lines.push(line);
    }

    lines.join("\r\n")
}

fn stamp(at: u64) -> String {
    // Milliseconds since the epoch, rendered as seconds and milliseconds. The
    // renderer formats for the reader; this is for a file.
    format!("{}.{:03}", at / 1000, at % 1000)
}

pub fn format_trace_line(e: &TraceEntry) -> String {
    let arrow = match e.direction {
        crate::app::trace::Direction::Out => "out >>",
        crate::app::trace::Direction::In => "in  <<",
    };
    let what = match &e.decode_error {
        Some(error) => format!("DECODE ERROR: {error}"),
        None => e.summary.clone(),
    };
    format!(
        "{} {arrow} seq={} {} status={} {what} [{}]",
        stamp(e.at),
        e.sequence_number,
        e.command_name,
        e.command_status,
        crate::smpp::summary::hex(&e.raw)
    )
}

pub fn trace_to_text(entries: &[TraceEntry]) -> String {
    entries
        .iter()
        .map(format_trace_line)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn writes_a_header_row_from_the_columns() {
        let csv = to_csv(&[row(&[("a", "1"), ("b", "2")])], &["a", "b"]);
        assert_eq!(csv.split("\r\n").next().unwrap(), "a,b");
    }

    #[test]
    fn writes_one_row_per_record_in_column_order() {
        let csv = to_csv(&[row(&[("b", "two"), ("a", "one")])], &["a", "b"]);
        assert_eq!(csv.split("\r\n").nth(1).unwrap(), "one,two");
    }

    #[test]
    fn quotes_a_field_containing_a_comma() {
        let csv = to_csv(&[row(&[("a", "x,y")])], &["a"]);
        assert_eq!(csv.split("\r\n").nth(1).unwrap(), "\"x,y\"");
    }

    #[test]
    fn doubles_a_quote_inside_a_quoted_field() {
        let csv = to_csv(&[row(&[("a", "say \"hi\"")])], &["a"]);
        assert_eq!(csv.split("\r\n").nth(1).unwrap(), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn quotes_a_field_containing_a_newline() {
        assert!(to_csv(&[row(&[("a", "line1\nline2")])], &["a"]).contains("\"line1\nline2\""));
    }

    #[test]
    fn writes_an_empty_field_for_a_missing_value() {
        let csv = to_csv(&[row(&[])], &["a", "b"]);
        assert_eq!(csv.split("\r\n").nth(1).unwrap(), ",");
    }

    #[test]
    fn writes_only_a_header_row_for_no_records() {
        assert_eq!(to_csv(&[], &["a", "b"]), "a,b");
    }
}
