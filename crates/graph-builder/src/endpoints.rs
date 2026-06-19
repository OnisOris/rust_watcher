pub(crate) fn extract_route_handlers(line: &str) -> Vec<(String, String)> {
    const METHODS: [&str; 7] = ["get", "post", "put", "patch", "delete", "head", "options"];
    let mut handlers = Vec::new();
    for method in METHODS {
        let pattern = format!("{method}(");
        let mut search_from = 0usize;
        while let Some(offset) = line[search_from..].find(&pattern) {
            let start = search_from + offset + pattern.len();
            if let Some(handler) = first_ident(&line[start..]) {
                handlers.push((method.to_string(), handler));
            }
            search_from = start;
        }
    }
    handlers
}

pub(crate) fn first_ident(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let ident = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == ':')
        .collect::<String>();
    if ident.is_empty() {
        None
    } else {
        Some(ident.rsplit("::").next().unwrap_or(&ident).to_string())
    }
}

pub(crate) fn endpoint_id(file: &str, method: &str, path: &str, line: u32) -> String {
    let safe_path = path
        .trim_matches('/')
        .replace(['/', ':'], "_")
        .replace(['{', '}', '$'], "");
    format!("endpoint:{file}::{method}:{safe_path}@{line}")
}
