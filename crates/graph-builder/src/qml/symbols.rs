use super::parser::parse_qml_source;
use super::{QmlFile, QmlImport, QmlRelationshipFact, QmlSymbol};

pub(crate) fn discover_qml_symbols(file: &QmlFile) -> Vec<QmlSymbol> {
    let (symbols, _, _) = parse_qml_source(&file.relative_path, &file.source);
    symbols
}

pub(crate) fn analyze_qml_file(
    file: &QmlFile,
) -> (Vec<QmlSymbol>, Vec<QmlImport>, Vec<QmlRelationshipFact>) {
    parse_qml_source(&file.relative_path, &file.source)
}
