//! Compact source summaries; the bounded syn AST is discarded after each file.
use super::*;
use std::collections::BTreeMap;
use syn::{spanned::Spanned, Item};

#[derive(Clone, Default)]
pub(super) struct Unit {
    pub modules: Vec<Decl>,
    pub uses: Vec<imports::UseSpec>,
    pub names: BTreeMap<String, Vec<Vec<String>>>,
    pub bindings: BTreeSet<String>,
    pub opaque: Vec<(usize, String)>,
    pub conditions: Vec<String>,
    pub(super) locals: Vec<usize>,
    pub(super) budgets: Vec<usize>,
}
#[derive(Clone)]
pub(super) struct Decl {
    pub(super) name: String,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) attrs: Vec<String>,
    pub(super) path: Option<String>,
    pub(super) invalid: bool,
    pub(super) inline: Option<Box<Unit>>,
}
pub(super) fn conditions(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut iter = attrs.iter().filter_map(|a| match &a.meta {
        syn::Meta::List(l) if l.path.is_ident("cfg") || l.path.is_ident("cfg_attr") => {
            let text = format!("{}({})", l.path.segments.last()?.ident, l.tokens);
            Some(if text.len() > MAX_PATH_BYTES {
                format!(
                    "{} [truncated:{}]",
                    text.chars().take(256).collect::<String>(),
                    declaration_id("cfg", &text, "", "")
                )
            } else {
                text
            })
        }
        _ => None,
    });
    let mut result: Vec<_> = iter.by_ref().take(MAX_CONDITIONS).collect();
    if iter.next().is_some() {
        result.push("<additional cfg predicates truncated by budget>".into());
    }
    result
}
pub(super) fn conditions_limited(attrs: &[syn::Attribute]) -> bool {
    let cfg: Vec<_> = attrs
        .iter()
        .filter_map(|a| match &a.meta {
            syn::Meta::List(l) if l.path.is_ident("cfg") || l.path.is_ident("cfg_attr") => Some(l),
            _ => None,
        })
        .collect();
    cfg.len() > MAX_CONDITIONS
        || cfg
            .iter()
            .any(|l| l.tokens.to_string().len() + 16 > MAX_PATH_BYTES)
}
pub(super) fn unit(items: &[Item], source: &str) -> Unit {
    let mut u = Unit::default();
    for item in items {
        match item {
            Item::Mod(m) => {
                if conditions_limited(&m.attrs) {
                    u.budgets.push(m.span().start().line);
                }
                if m.ident.to_string().len() > MAX_NAME_BYTES {
                    u.budgets.push(m.span().start().line);
                    continue;
                }
                let mut path = None;
                let mut invalid = false;
                for a in &m.attrs {
                    if a.path().is_ident("path") {
                        if let syn::Meta::NameValue(v) = &a.meta {
                            if let syn::Expr::Lit(l) = &v.value {
                                if let syn::Lit::Str(s) = &l.lit {
                                    let value = s.value();
                                    if value.len() > MAX_PATH_BYTES {
                                        invalid = true;
                                        u.budgets.push(m.span().start().line);
                                        continue;
                                    }
                                    if path.replace(value).is_some() {
                                        invalid = true;
                                    }
                                    continue;
                                }
                            }
                        }
                        invalid = true;
                    } else if ![
                        "cfg",
                        "allow",
                        "warn",
                        "deny",
                        "forbid",
                        "doc",
                        "deprecated",
                        "expect",
                    ]
                    .iter()
                    .any(|s| a.path().is_ident(s))
                    {
                        invalid = true;
                    }
                }
                let pos = m.ident.span().start();
                let column = source
                    .lines()
                    .nth(pos.line.saturating_sub(1))
                    .map_or(0, |l| {
                        let bom = usize::from(pos.line == 1 && l.starts_with('\u{feff}'));
                        let l = if bom == 1 {
                            l.trim_start_matches('\u{feff}')
                        } else {
                            l
                        };
                        bom + l
                            .chars()
                            .take(pos.column)
                            .map(char::len_utf16)
                            .sum::<usize>()
                    })
                    + 1;
                u.modules.push(Decl {
                    name: m.ident.to_string().trim_start_matches("r#").into(),
                    line: pos.line,
                    column,
                    attrs: conditions(&m.attrs),
                    path,
                    invalid,
                    inline: m
                        .content
                        .as_ref()
                        .map(|(_, items)| Box::new(unit(items, source))),
                });
            }
            Item::Use(i) => {
                if u.uses.len() >= MAX_USE_LEAVES {
                    u.budgets.push(i.span().start().line);
                    continue;
                }
                let mut specs = imports::flatten(i);
                let remaining = MAX_USE_LEAVES - u.uses.len();
                if specs.len() > remaining {
                    specs.truncate(remaining);
                    u.budgets.push(i.span().start().line);
                }
                for s in &specs {
                    if let Some(b) = s.alias.as_ref().or(s.segments.last()) {
                        u.bindings.insert(b.clone());
                    }
                    if s.glob {
                        u.bindings.insert("*".into());
                    }
                }
                u.uses.extend(specs);
            }
            Item::ExternCrate(i) => {
                u.bindings.insert(
                    i.rename
                        .as_ref()
                        .map_or_else(|| i.ident.to_string(), |(_, name)| name.to_string()),
                );
            }
            Item::Macro(m) => u.opaque.push((
                m.span().start().line,
                "module-scope macro not expanded (including include!/macro_rules!)".into(),
            )),
            _ => {
                struct Local(Vec<usize>);
                impl<'a> syn::visit::Visit<'a> for Local {
                    fn visit_item_mod(&mut self, m: &'a syn::ItemMod) {
                        self.0.push(m.span().start().line);
                    }
                    fn visit_item_use(&mut self, m: &'a syn::ItemUse) {
                        self.0.push(m.span().start().line);
                    }
                }
                let mut local = Local(vec![]);
                syn::visit::Visit::visit_item(&mut local, item);
                u.locals.extend(local.0);
                let name = match item {
                    Item::Fn(v) => Some((&v.sig.ident, &v.attrs)),
                    Item::Struct(v) => Some((&v.ident, &v.attrs)),
                    Item::Enum(v) => Some((&v.ident, &v.attrs)),
                    Item::Trait(v) => Some((&v.ident, &v.attrs)),
                    Item::Type(v) => Some((&v.ident, &v.attrs)),
                    Item::Const(v) => Some((&v.ident, &v.attrs)),
                    Item::Static(v) => Some((&v.ident, &v.attrs)),
                    Item::Union(v) => Some((&v.ident, &v.attrs)),
                    _ => None,
                };
                if let Some((name, attrs)) = name {
                    if conditions_limited(attrs) {
                        u.budgets.push(name.span().start().line);
                    }
                    if name.to_string().len() > MAX_NAME_BYTES {
                        u.budgets.push(name.span().start().line);
                        continue;
                    }
                    if attrs.iter().any(|a| {
                        ![
                            "cfg",
                            "doc",
                            "allow",
                            "warn",
                            "deny",
                            "forbid",
                            "expect",
                            "deprecated",
                            "derive",
                            "repr",
                            "non_exhaustive",
                            "must_use",
                            "inline",
                            "cold",
                            "track_caller",
                            "target_feature",
                            "export_name",
                            "link_name",
                            "no_mangle",
                            "test",
                            "ignore",
                            "should_panic",
                        ]
                        .iter()
                        .any(|n| a.path().is_ident(n))
                    }) {
                        u.bindings
                            .insert(name.to_string().trim_start_matches("r#").into());
                        u.opaque.push((
                            name.span().start().line,
                            "item macro/conditional attribute not expanded; no leaf proof".into(),
                        ));
                    }
                    u.names
                        .entry(name.to_string().trim_start_matches("r#").into())
                        .or_default()
                        .push(conditions(attrs));
                }
            }
        }
    }
    u
}
impl Unit {
    pub(super) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .names
                .iter()
                .map(|(k, v)| {
                    k.len() + v.iter().flatten().map(|s| s.len() + 32).sum::<usize>() + 64
                })
                .sum::<usize>()
            + self.bindings.iter().map(|s| s.len() + 32).sum::<usize>()
            + self.opaque.iter().map(|(_, s)| s.len() + 32).sum::<usize>()
            + self.conditions.iter().map(String::len).sum::<usize>()
            + (self.locals.len() + self.budgets.len()) * 8
            + self
                .uses
                .iter()
                .map(|u| {
                    128 + u.segments.iter().map(|s| s.len() + 32).sum::<usize>()
                        + u.alias.as_ref().map_or(0, String::len)
                        + u.conditions.iter().map(String::len).sum::<usize>()
                })
                .sum::<usize>()
            + self
                .modules
                .iter()
                .map(|m| {
                    128 + m.name.len()
                        + m.attrs.iter().map(String::len).sum::<usize>()
                        + m.path.as_ref().map_or(0, String::len)
                        + m.inline.as_ref().map_or(0, |u| u.bytes())
                })
                .sum::<usize>()
    }
    /// Keep only proof tables, not repeated bodies/use trees per target context.
    pub(super) fn namespace(&self) -> Self {
        Self {
            budgets: self.budgets.first().copied().into_iter().collect(),
            names: self.names.clone(),
            bindings: self.bindings.clone(),
            modules: self
                .modules
                .iter()
                .map(|m| Decl {
                    name: m.name.clone(),
                    line: 0,
                    column: 0,
                    attrs: vec![],
                    path: None,
                    invalid: false,
                    inline: None,
                })
                .collect(),
            ..Default::default()
        }
    }
}
