use super::{FunctionKind, FunctionMetrics};
use crate::metrics::{classify, Category};
use proc_macro2::{TokenStream, TokenTree};
use std::path::Path;
use syn::visit::{self, Visit};

// Resource guard, using the real Rust tokenizer rather than a brace scanner.
const MAX_TOKEN_DEPTH: usize = 128;
const MAX_TOKENS: usize = 200_000;

#[derive(Debug)]
pub(super) enum Failure {
    Parse(String),
    Resource(&'static str),
}

pub(super) fn functions(
    source: &str,
    path: &Path,
    package: Option<String>,
    limit: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<FunctionMetrics>, Failure> {
    let token_source = source.trim_start_matches('\u{feff}');
    let token_source =
        if token_source.starts_with("#!") && !token_source[2..].trim_start().starts_with('[') {
            token_source.find('\n').map_or("", |i| &token_source[i..])
        } else {
            token_source
        };
    let tokens: TokenStream = token_source
        .parse()
        .map_err(|e: proc_macro2::LexError| Failure::Parse(e.to_string()))?;
    let mut stack = vec![(tokens.into_iter(), 0)];
    let mut count = 0;
    while let Some((mut iter, depth)) = stack.pop() {
        while let Some(token) = iter.next() {
            count += 1;
            if cancelled() {
                return Err(Failure::Resource("cancelled"));
            }
            if count > MAX_TOKENS || depth > MAX_TOKEN_DEPTH {
                return Err(Failure::Resource("AST token/depth budget exceeded"));
            }
            if let TokenTree::Group(group) = token {
                stack.push((iter, depth));
                stack.push((group.stream().into_iter(), depth + 1));
                break;
            }
        }
    }
    let file = syn::parse_file(source).map_err(|e| {
        Failure::Parse(format!(
            "{}:{}: {e}",
            e.span().start().line,
            e.span().start().column + 1
        ))
    })?;
    let mut collector = Collector {
        source_lines: source.lines().collect(),
        path,
        package,
        scope: vec![],
        test: classify(path).1 == Category::Tests,
        functions: vec![],
        limit,
        cancelled,
    };
    collector.visit_file(&file);
    if cancelled() {
        return Err(Failure::Resource("cancelled"));
    }
    Ok(collector.functions)
}

struct Collector<'a, F> {
    source_lines: Vec<&'a str>,
    path: &'a Path,
    package: Option<String>,
    scope: Vec<String>,
    test: bool,
    functions: Vec<FunctionMetrics>,
    limit: usize,
    cancelled: &'a F,
}

fn test_attributes(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg")
                && a.parse_args::<syn::Path>()
                    .is_ok_and(|p| p.is_ident("test")))
    })
}

impl<F: Fn() -> bool> Collector<'_, F> {
    fn record(
        &mut self,
        sig: &syn::Signature,
        body: &syn::Block,
        attrs: &[syn::Attribute],
        kind: FunctionKind,
    ) {
        if self.functions.len() >= self.limit || (self.cancelled)() {
            return;
        }
        let mut control = Control::default();
        control.visit_block(body);
        let start_line = sig.fn_token.span.start().line;
        let end_line = body.brace_token.span.close().end().line;
        let pos = sig.ident.span().start();
        let name = sig.ident.to_string().trim_start_matches("r#").to_owned();
        let mut qualified = self.scope.clone();
        qualified.push(name.clone());
        let identifier_column = self
            .source_lines
            .get(pos.line.saturating_sub(1))
            .map_or(0, |l| {
                let bom = usize::from(pos.line == 1 && l.starts_with('\u{feff}'));
                let line = if bom == 1 {
                    l.trim_start_matches('\u{feff}')
                } else {
                    l
                };
                bom + line
                    .chars()
                    .take(pos.column)
                    .map(char::len_utf16)
                    .sum::<usize>()
            })
            + 1;
        self.functions.push(FunctionMetrics {
            path: self.path.into(),
            name: name.clone(),
            qualified_name: qualified.join("::"),
            kind,
            start_line,
            end_line,
            identifier_line: pos.line,
            identifier_column,
            physical_lines: end_line.saturating_sub(start_line) + 1,
            max_control_depth: control.max,
            control_flow_nodes: control.nodes,
            package: self.package.clone(),
            test_classified: self.test || test_attributes(attrs),
        });
        let test = self.test;
        self.test |= test_attributes(attrs);
        self.scope.push(name);
        self.visit_block(body);
        self.scope.pop();
        self.test = test;
    }
}

impl<'ast, F: Fn() -> bool> Visit<'ast> for Collector<'_, F> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.record(&node.sig, &node.block, &node.attrs, FunctionKind::Function);
    }
    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let kind = if node.sig.receiver().is_some() {
            FunctionKind::Method
        } else {
            FunctionKind::AssociatedFunction
        };
        self.record(&node.sig, &node.block, &node.attrs, kind);
    }
    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if let Some(body) = &node.default {
            self.record(&node.sig, body, &node.attrs, FunctionKind::TraitDefault);
        }
    }
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let test = self.test;
        self.test |= test_attributes(&node.attrs);
        self.scope.push(node.ident.to_string());
        visit::visit_item_mod(self, node);
        self.scope.pop();
        self.test = test;
    }
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let name = if let syn::Type::Path(p) = node.self_ty.as_ref() {
            p.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::")
        } else {
            "impl".into()
        };
        let test = self.test;
        self.test |= test_attributes(&node.attrs);
        self.scope.push(name);
        visit::visit_item_impl(self, node);
        self.scope.pop();
        self.test = test;
    }
    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let test = self.test;
        self.test |= test_attributes(&node.attrs);
        self.scope.push(node.ident.to_string());
        visit::visit_item_trait(self, node);
        self.scope.pop();
        self.test = test;
    }
}

#[derive(Default)]
struct Control {
    depth: usize,
    max: usize,
    nodes: usize,
}

impl<'ast> Visit<'ast> for Control {
    fn visit_item(&mut self, _: &'ast syn::Item) {}
    fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {}
    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        let control = matches!(
            expr,
            syn::Expr::If(_)
                | syn::Expr::Match(_)
                | syn::Expr::ForLoop(_)
                | syn::Expr::While(_)
                | syn::Expr::Loop(_)
        );
        if control {
            self.nodes += 1;
            self.depth += 1;
            self.max = self.max.max(self.depth);
        }
        if let syn::Expr::If(node) = expr {
            self.visit_expr(&node.cond);
            self.visit_block(&node.then_branch);
            if let Some((_, other)) = &node.else_branch {
                if matches!(other.as_ref(), syn::Expr::If(_)) {
                    self.depth -= 1;
                    self.visit_expr(other);
                    self.depth += 1;
                } else {
                    self.visit_expr(other);
                }
            }
        } else {
            visit::visit_expr(self, expr);
        }
        if control {
            self.depth -= 1;
        }
    }
}
