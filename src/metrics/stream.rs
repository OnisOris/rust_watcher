//! Fixed-memory UTF-8 line counting, including lines longer than the buffer.
use std::io::Read;
use std::path::Path;

pub const MAX_METRIC_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TOTAL_READ_BYTES: u64 = 512 * 1024 * 1024;
pub const READ_BUFFER: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadFailure {
    Binary,
    InvalidUtf8,
    PerFileBudget,
    TotalReadBudget,
    Unreadable,
    Cancelled,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Lines {
    pub physical: u64,
    pub blank: u64,
}

pub fn count(
    path: &Path,
    budget: &mut u64,
    cancelled: &impl Fn() -> bool,
) -> Result<Lines, ReadFailure> {
    let mut file = std::fs::File::open(path).map_err(|_| ReadFailure::Unreadable)?;
    let size = file.metadata().map_err(|_| ReadFailure::Unreadable)?.len();
    count_reader(
        &mut file,
        Some(size),
        budget,
        MAX_METRIC_FILE_BYTES,
        cancelled,
    )
}

fn count_reader(
    reader: &mut impl Read,
    size: Option<u64>,
    budget: &mut u64,
    limit: u64,
    cancelled: &impl Fn() -> bool,
) -> Result<Lines, ReadFailure> {
    if size.is_some_and(|n| n > limit) {
        return Err(ReadFailure::PerFileBudget);
    }
    if size.is_some_and(|n| n > *budget) {
        return Err(ReadFailure::TotalReadBudget);
    }
    let mut buffer = [0_u8; READ_BUFFER + 4];
    let mut carry = 0;
    let mut read = 0;
    let mut lines = Lines::default();
    let mut has_line = false;
    let mut blank = true;
    loop {
        if cancelled() {
            return Err(ReadFailure::Cancelled);
        }
        let available = (*budget).min(limit - read).min(READ_BUFFER as u64) as usize;
        if available == 0 {
            // A file exactly filling a budget can finish at its observed size.
            if size == Some(read) {
                break;
            }
            return Err(if read == limit {
                ReadFailure::PerFileBudget
            } else {
                ReadFailure::TotalReadBudget
            });
        }
        let count = reader
            .read(&mut buffer[carry..carry + available])
            .map_err(|_| ReadFailure::Unreadable)?;
        *budget -= count as u64;
        read += count as u64;
        if count == 0 {
            break;
        }
        let end = carry + count;
        if buffer[..end]
            .iter()
            .any(|b| *b == 0 || (*b < 32 && !matches!(*b, b'\n' | b'\r' | b'\t' | 11 | 12)))
        {
            return Err(ReadFailure::Binary);
        }
        let (text, tail) = match std::str::from_utf8(&buffer[..end]) {
            Ok(text) => (text, 0),
            Err(error) if error.error_len().is_none() => {
                let valid = error.valid_up_to();
                (
                    std::str::from_utf8(&buffer[..valid]).expect("validated UTF-8 prefix"),
                    end - valid,
                )
            }
            Err(_) => return Err(ReadFailure::InvalidUtf8),
        };
        for c in text.chars() {
            if c == '\n' {
                lines.physical += 1;
                lines.blank += u64::from(blank);
                has_line = false;
                blank = true;
            } else {
                has_line = true;
                blank &= c.is_whitespace();
            }
        }
        buffer.copy_within(end - tail..end, 0);
        carry = tail;
    }
    if carry != 0 {
        return Err(ReadFailure::InvalidUtf8);
    }
    if has_line {
        lines.physical += 1;
        lines.blank += u64::from(blank);
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn measured(bytes: &[u8]) -> Result<Lines, ReadFailure> {
        let mut budget = MAX_TOTAL_READ_BYTES;
        count_reader(
            &mut Cursor::new(bytes),
            Some(bytes.len() as u64),
            &mut budget,
            MAX_METRIC_FILE_BYTES,
            &|| false,
        )
    }

    #[test]
    fn streaming_matches_text_lines_across_boundaries() {
        for text in [
            "".into(),
            "\n".into(),
            "a\n \nlast".into(),
            "a\r\n \r\nlast".into(),
            "\u{2003}\u{a0}\n\t\r\n".into(),
            "x".repeat(3 * 1024 * 1024),
            format!("{}\u{2003}\n", " ".repeat(READ_BUFFER - 1)),
        ] {
            let lines = measured(text.as_bytes()).unwrap();
            assert_eq!(lines.physical, text.lines().count() as u64);
            assert_eq!(
                lines.blank,
                text.lines().filter(|l| l.trim().is_empty()).count() as u64
            );
        }
        assert_eq!(measured(&[255]), Err(ReadFailure::InvalidUtf8));
        assert_eq!(measured(&[0]), Err(ReadFailure::Binary));
        assert_eq!(measured(&[0xe2, 0x80]), Err(ReadFailure::InvalidUtf8));
    }

    #[test]
    fn budgets_are_hard_bounds_even_without_a_known_length() {
        let mut total = 10;
        assert_eq!(
            count_reader(&mut Cursor::new(b"123456"), Some(6), &mut total, 5, &|| {
                false
            }),
            Err(ReadFailure::PerFileBudget)
        );
        assert_eq!(total, 10);
        let mut total = 3;
        assert_eq!(
            count_reader(&mut Cursor::new(b"12345"), None, &mut total, 10, &|| false),
            Err(ReadFailure::TotalReadBudget)
        );
        assert_eq!(total, 0);
        let mut total = 100;
        assert_eq!(
            count_reader(&mut Cursor::new(b"12345"), None, &mut total, 3, &|| false),
            Err(ReadFailure::PerFileBudget)
        );
        assert_eq!(total, 97);
        let mut total = 3;
        assert_eq!(
            count_reader(&mut Cursor::new(b"abc"), Some(3), &mut total, 3, &|| false)
                .unwrap()
                .physical,
            1
        );
    }
}
