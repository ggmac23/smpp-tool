//! Where a burst's destinations come from: one number repeated, a generated
//! range, or a CSV the user loaded.

/// The sizing ceiling the spec sets: results are held in memory up to here.
pub const MAX_BURST: usize = 50_000;
pub const MAX_RATE: u32 = 500;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RecipientSource {
    Repeat {
        number: String,
        count: usize,
    },
    Range {
        from: String,
        count: usize,
    },
    Csv {
        text: String,
        column: Option<String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Resolved {
    pub numbers: Vec<String>,
    /// Problems worth telling the user about. Never a reason to drop a row.
    pub warnings: Vec<String>,
}

/// Column names that mean "this row is a header", whatever it contains.
const HEADER_WORDS: &[&str] = &[
    "msisdn",
    "number",
    "numero",
    "phone",
    "mobile",
    "destination",
    "destination_addr",
    "dest",
    "dst",
    "to",
    "address",
    "addr",
    "recipient",
    "subscriber",
];

/// An address the tool will send without comment. Letters are allowed,
/// because alphanumeric sender and destination addresses are legitimate.
fn plausible_address(s: &str) -> bool {
    !s.is_empty()
        && s.chars().count() <= 32
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "+._-".contains(c))
}

/// Split one CSV line, honouring double quotes.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let chars: Vec<char> = line.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if quoted {
            if ch == '"' {
                if chars.get(i + 1) == Some(&'"') {
                    field.push('"');
                    i += 1;
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' {
            quoted = true;
        } else if ch == ',' {
            out.push(std::mem::take(&mut field));
        } else {
            field.push(ch);
        }
        i += 1;
    }
    out.push(field);
    out.into_iter().map(|f| f.trim().to_string()).collect()
}

/// Increment the numeric tail of an address, keeping its width.
fn increment(address: &str, by: usize) -> Option<String> {
    let digits_start = address
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .map(|(i, _)| i)
        .last()?;
    let (head, digits) = address.split_at(digits_start);
    if digits.is_empty() {
        return None;
    }
    let value: u128 = digits.parse().ok()?;
    let next = (value + by as u128).to_string();
    Some(format!("{head}{next:0>width$}", width = digits.len()))
}

fn check_count(n: usize) -> Result<(), String> {
    if n > MAX_BURST {
        return Err(format!(
            "{n} recipients is above the {MAX_BURST}-message ceiling this build holds in memory"
        ));
    }
    if n < 1 {
        return Err("a burst needs at least one recipient".into());
    }
    Ok(())
}

pub fn resolve_recipients(source: &RecipientSource) -> Result<Resolved, String> {
    let mut warnings = Vec::new();

    match source {
        RecipientSource::Repeat { number, count } => {
            check_count(*count)?;
            Ok(Resolved {
                numbers: vec![number.clone(); *count],
                warnings,
            })
        }

        RecipientSource::Range { from, count } => {
            check_count(*count)?;
            if increment(from, 0).is_none() {
                warnings.push(format!(
                    "\"{from}\" has no numeric tail to increment; every message uses it as is"
                ));
                return Ok(Resolved {
                    numbers: vec![from.clone(); *count],
                    warnings,
                });
            }
            let numbers = (0..*count)
                .map(|i| increment(from, i).unwrap_or_else(|| from.clone()))
                .collect();
            Ok(Resolved { numbers, warnings })
        }

        RecipientSource::Csv { text, column } => {
            let rows: Vec<(usize, &str)> = text
                .lines()
                .enumerate()
                .map(|(i, l)| (i + 1, l.trim()))
                .filter(|(_, l)| !l.is_empty())
                .collect();
            if rows.is_empty() {
                return Err("the CSV has no rows".into());
            }

            let first = split_csv_line(rows[0].1);
            let mut column_index = 0usize;
            let mut start = 0usize;

            match column {
                Some(name) => {
                    // Naming a column says the file has a header row, whether
                    // or not the name is in it.
                    start = 1;
                    match first.iter().position(|h| h.eq_ignore_ascii_case(name)) {
                        Some(found) => column_index = found,
                        None => warnings.push(format!(
                            "no column named \"{name}\"; using the first column instead"
                        )),
                    }
                }
                None => {
                    // Row one is a header when its cell is a known header word,
                    // or when it holds no digits while a later row's does. A
                    // file of purely alphanumeric senders keeps every row.
                    let cell = first.first().cloned().unwrap_or_default();
                    let has_digits = |s: &str| s.chars().any(|c| c.is_ascii_digit());
                    let later_has_digits = rows[1..].iter().any(|(_, l)| {
                        has_digits(split_csv_line(l).first().map(|s| s.as_str()).unwrap_or(""))
                    });
                    if HEADER_WORDS.contains(&cell.to_ascii_lowercase().as_str())
                        || (!has_digits(&cell) && later_has_digits)
                    {
                        start = 1;
                    }
                }
            }

            let body = &rows[start..];
            check_count(body.len())?;

            let mut numbers = Vec::with_capacity(body.len());
            for (number, line) in body {
                let cells = split_csv_line(line);
                let value = cells.get(column_index).cloned().unwrap_or_default();
                if !plausible_address(&value) {
                    warnings.push(format!(
                        "row {number}: \"{value}\" does not look like an address; sending it anyway"
                    ));
                }
                numbers.push(value);
            }

            Ok(Resolved { numbers, warnings })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeat(number: &str, count: usize) -> RecipientSource {
        RecipientSource::Repeat {
            number: number.into(),
            count,
        }
    }

    #[test]
    fn repeats_one_number_count_times() {
        let r = resolve_recipients(&repeat("393331234567", 3)).unwrap();
        assert_eq!(r.numbers, vec!["393331234567"; 3]);
        assert!(r.warnings.is_empty());
    }

    #[test]
    fn increments_the_numeric_tail_for_a_range() {
        let r = resolve_recipients(&RecipientSource::Range {
            from: "393331234567".into(),
            count: 3,
        })
        .unwrap();
        assert_eq!(
            r.numbers,
            vec!["393331234567", "393331234568", "393331234569"]
        );
    }

    #[test]
    fn keeps_the_width_when_a_range_carries_over() {
        let r = resolve_recipients(&RecipientSource::Range {
            from: "393331234599".into(),
            count: 2,
        })
        .unwrap();
        assert_eq!(r.numbers, vec!["393331234599", "393331234600"]);
    }

    #[test]
    fn increments_only_the_numeric_tail_of_a_mixed_address() {
        let r = resolve_recipients(&RecipientSource::Range {
            from: "SENDER007".into(),
            count: 2,
        })
        .unwrap();
        assert_eq!(r.numbers, vec!["SENDER007", "SENDER008"]);
    }

    #[test]
    fn warns_when_a_range_has_no_numeric_tail() {
        let r = resolve_recipients(&RecipientSource::Range {
            from: "ALPHA".into(),
            count: 2,
        })
        .unwrap();
        assert!(r.warnings.join(" ").to_lowercase().contains("numeric"));
        assert_eq!(r.numbers, vec!["ALPHA", "ALPHA"]);
    }

    fn csv(text: &str, column: Option<&str>) -> Resolved {
        resolve_recipients(&RecipientSource::Csv {
            text: text.into(),
            column: column.map(|c| c.to_string()),
        })
        .unwrap()
    }

    #[test]
    fn reads_a_csv_column_by_name() {
        let r = csv(
            "name,msisdn\nalice,393331111111\nbob,393332222222\n",
            Some("msisdn"),
        );
        assert_eq!(r.numbers, vec!["393331111111", "393332222222"]);
    }

    #[test]
    fn reads_the_first_column_when_no_name_is_given() {
        let r = csv("393331111111,alice\n393332222222,bob\n", None);
        assert_eq!(r.numbers, vec!["393331111111", "393332222222"]);
    }

    #[test]
    fn skips_a_header_row() {
        let r = csv("msisdn\n393331111111\n393332222222\n", None);
        assert_eq!(r.numbers, vec!["393331111111", "393332222222"]);
    }

    #[test]
    fn keeps_a_numeric_first_row_that_is_not_a_header() {
        assert_eq!(csv("393331111111\n393332222222\n", None).numbers.len(), 2);
    }

    #[test]
    fn warns_about_a_malformed_row_and_keeps_it() {
        let r = csv("msisdn\n393331111111\nnot a number!\n393332222222\n", None);
        assert_eq!(
            r.numbers,
            vec!["393331111111", "not a number!", "393332222222"]
        );
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].to_lowercase().contains("row 3"));
    }

    #[test]
    fn ignores_blank_lines() {
        assert_eq!(
            csv("393331111111\n\n\n393332222222\n", None).numbers.len(),
            2
        );
    }

    #[test]
    fn handles_quoted_csv_fields() {
        let r = csv(
            "msisdn,note\n\"393331111111\",\"hello, world\"\n",
            Some("msisdn"),
        );
        assert_eq!(r.numbers, vec!["393331111111"]);
    }

    #[test]
    fn warns_when_a_named_column_is_missing_and_falls_back_to_the_first() {
        let r = csv("name,other\nalice,393331111111\n", Some("msisdn"));
        assert!(r.warnings.join(" ").contains("msisdn"));
        assert_eq!(r.numbers, vec!["alice"]);
    }

    #[test]
    fn refuses_a_count_above_the_ceiling() {
        let err = resolve_recipients(&repeat("1", MAX_BURST + 1)).unwrap_err();
        assert!(err.contains(&MAX_BURST.to_string()), "{err}");
    }

    #[test]
    fn accepts_a_count_at_exactly_the_ceiling() {
        assert_eq!(
            resolve_recipients(&repeat("1", MAX_BURST))
                .unwrap()
                .numbers
                .len(),
            MAX_BURST
        );
    }

    #[test]
    fn exposes_the_rate_ceiling() {
        assert_eq!(MAX_RATE, 500);
    }
}
