//! Presentation-only lexical colors for the bounded source preview, never semantics.
//! Context before the selected declaration is plain: its initial lexical state is unknown.
use super::theme;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

#[derive(Default)]
enum State {
    #[default]
    Normal,
    String {
        escape: bool,
    },
    Raw(usize),
    Comment(usize),
}
pub fn preview(source: &str) -> Vec<Line<'static>> {
    let mut state = State::Normal;
    let mut known = false;
    let safe = !source.contains("[line clipped]");
    source
        .lines()
        .map(|line| {
            let Some((prefix, code)) = line.split_once(" | ") else {
                return Line::raw(line.to_owned());
            };
            let selected = prefix.starts_with('>');
            if selected {
                known = safe
                    && [
                        "fn ", "pub ", "pub(", "async ", "struct ", "enum ", "trait ", "const ",
                        "static ", "type ", "impl ",
                    ]
                    .iter()
                    .any(|word| code.trim_start().starts_with(word));
                state = State::Normal;
            }
            let mut spans = vec![Span::styled(
                format!("{prefix} │ "),
                Style::default().fg(if selected {
                    theme::ACTIVE
                } else {
                    theme::MUTED
                }),
            )];
            if known {
                spans.extend(lex(code, &mut state));
            } else {
                spans.push(Span::raw(code.to_owned()));
            }
            let mut line = Line::from(spans);
            if selected {
                line = line.style(Style::default().add_modifier(Modifier::BOLD));
            }
            line
        })
        .collect()
}
fn lex(code: &str, state: &mut State) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < code.len() {
        let start = i;
        let color = match state {
            State::Comment(depth) => {
                while i < code.len() {
                    if code[i..].starts_with("/*") {
                        *depth += 1;
                        i += 2;
                    } else if code[i..].starts_with("*/") {
                        *depth -= 1;
                        i += 2;
                        if *depth == 0 {
                            *state = State::Normal;
                            break;
                        }
                    } else {
                        i += code[i..].chars().next().unwrap().len_utf8();
                    }
                }
                theme::MUTED
            }
            State::Raw(hashes) => {
                let end = format!("\"{}", "#".repeat(*hashes));
                if let Some(n) = code[i..].find(&end) {
                    i += n + end.len();
                    *state = State::Normal;
                } else {
                    i = code.len();
                }
                theme::SUCCESS
            }
            State::String { escape } => {
                while i < code.len() {
                    let c = code[i..].chars().next().unwrap();
                    i += c.len_utf8();
                    if *escape {
                        *escape = false;
                    } else if c == '\\' {
                        *escape = true;
                    } else if c == '"' {
                        *state = State::Normal;
                        break;
                    }
                }
                theme::SUCCESS
            }
            State::Normal => {
                let rest = &code[i..];
                if rest.starts_with("//") {
                    i = code.len();
                    theme::MUTED
                } else if rest.starts_with("/*") {
                    *state = State::Comment(0);
                    continue;
                } else if rest.starts_with('"') {
                    i += 1;
                    *state = State::String { escape: false };
                    theme::SUCCESS
                } else if let Some((hashes, length)) = raw_prefix(rest) {
                    i += length;
                    *state = State::Raw(hashes);
                    theme::SUCCESS
                } else if let Some(length) = char_literal(rest) {
                    i += length;
                    theme::SUCCESS
                } else {
                    let c = rest.chars().next().unwrap();
                    i += c.len_utf8();
                    if c.is_alphabetic() || c == '_' {
                        while i < code.len()
                            && code[i..]
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_alphanumeric() || c == '_')
                        {
                            i += code[i..].chars().next().unwrap().len_utf8();
                        }
                        let word = &code[start..i];
                        if KEYWORDS.contains(&word) {
                            theme::ACCENT
                        } else if c.is_uppercase() {
                            theme::NUMBER
                        } else {
                            theme::TEXT
                        }
                    } else {
                        theme::TEXT
                    }
                }
            }
        };
        result.push(Span::styled(
            code[start..i].to_owned(),
            Style::default().fg(color),
        ));
    }
    // A trailing escape consumes the physical newline, not the next line's quote.
    if matches!(state, State::String { escape: true }) {
        *state = State::String { escape: false };
    }
    result
}
fn raw_prefix(text: &str) -> Option<(usize, usize)> {
    let bytes = usize::from(text.starts_with('b'));
    let text = if bytes == 1 { &text[1..] } else { text };
    let text = text.strip_prefix('r')?;
    let count = text.bytes().take_while(|b| *b == b'#').count();
    (text.as_bytes().get(count) == Some(&b'"')).then_some((count, count + 2 + bytes))
}
fn char_literal(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('\'')?;
    let first = rest.chars().next()?;
    if first != '\\' {
        let end = 1 + first.len_utf8();
        return text
            .get(end..)
            .is_some_and(|s| s.starts_with('\''))
            .then_some(end + 1);
    }
    let mut escape = false;
    for (i, c) in rest.char_indices().take(16) {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
        } else if c == '\'' {
            return Some(i + 2);
        }
    }
    None
}
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_unicode_raw_multiline_strings_and_nested_comments_exactly() {
        let source="    1 | uncertain context\n>   2 | fn work() {\n    3 | let s=r##\"日本 //\n    4 | /* literal */\"##; /* outer\n    5 | /* nested */ end */\n    6 | let t=\"line\n    7 | two\";\n    8 | }";
        let lines = preview(source);
        for (old, new) in source.lines().zip(&lines) {
            assert_eq!(old.replace(" | ", " │ "), new.to_string());
        }
        assert!(lines[1]
            .spans
            .iter()
            .any(|s| s.content == "fn" && s.style.fg == Some(theme::ACCENT)));
        assert!(lines[3]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(theme::SUCCESS)));
        assert!(lines[4]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(theme::MUTED)));
        assert_eq!(
            preview("unformatted text")[0].to_string(),
            "unformatted text"
        );
    }
}
