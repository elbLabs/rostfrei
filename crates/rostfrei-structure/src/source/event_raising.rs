use proc_macro2::{Delimiter, TokenStream, TokenTree, token_stream::IntoIter};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use super::facts::EventRaise;
use super::visitor::line;

pub(super) fn collect(file: &syn::File) -> Vec<EventRaise> {
    let mut visitor = EventRaisingVisitor::default();
    visitor.visit_file(file);
    visitor.raises
}

#[derive(Default)]
struct EventRaisingVisitor {
    raises: Vec<EventRaise>,
    implementation: Option<(usize, usize)>,
}

impl EventRaisingVisitor {
    fn record(&mut self, span: proc_macro2::Span) {
        self.raises.push(EventRaise {
            line: line(span),
            implementation: self.implementation,
        });
    }

    fn visit_tokens(&mut self, tokens: TokenStream) {
        let mut previous = None;
        let mut before_previous = None;
        let mut tokens = tokens.into_iter();
        while let Some(token) = tokens.next() {
            match &token {
                TokenTree::Group(group) => self.visit_tokens(group.stream()),
                TokenTree::Ident(ident)
                    if ident.unraw() == "raise"
                        && ((previous == Some('.')
                            && before_previous != Some('.')
                            && followed_by_arguments(tokens.clone()))
                            || (before_previous == Some(':') && previous == Some(':'))) =>
                {
                    self.record(ident.span());
                }
                _ => {}
            }
            before_previous = previous;
            previous = match token {
                TokenTree::Punct(punct) => Some(punct.as_char()),
                _ => None,
            };
        }
    }
}

fn followed_by_arguments(mut tokens: IntoIter) -> bool {
    let first = tokens.next();
    if let Some(TokenTree::Group(arguments)) = first {
        return arguments.delimiter() == Delimiter::Parenthesis;
    }
    if !matches!(
        (first, tokens.next(), tokens.next()),
        (Some(TokenTree::Punct(first)), Some(TokenTree::Punct(second)), Some(TokenTree::Punct(open)))
            if first.as_char() == ':' && second.as_char() == ':' && open.as_char() == '<'
    ) {
        return false;
    }

    let mut depth = 1_usize;
    let mut previous = None;
    while let Some(token) = tokens.next() {
        let punctuation = match token {
            TokenTree::Punct(punct) => Some(punct.as_char()),
            _ => None,
        };
        match punctuation {
            Some('<') => depth = depth.saturating_add(1),
            Some('>') if previous != Some('-') => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 {
            return matches!(tokens.next(), Some(TokenTree::Group(arguments))
                if arguments.delimiter() == Delimiter::Parenthesis);
        }
        previous = punctuation;
    }
    false
}

impl<'ast> Visit<'ast> for EventRaisingVisitor {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let enclosing = self.implementation.take();
        if let syn::Item::Impl(implementation) = item {
            for attribute in &implementation.attrs {
                self.visit_attribute(attribute);
            }
            let start = implementation.span().start();
            for member in &implementation.items {
                self.implementation =
                    matches!(member, syn::ImplItem::Fn(_)).then_some((start.line, start.column));
                self.visit_impl_item(member);
            }
        } else {
            visit::visit_item(self, item);
        }
        self.implementation = enclosing;
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method.unraw() == "raise" {
            self.record(call.method.span());
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if (path.qself.is_some() || path.path.segments.len() > 1)
            && let Some(segment) = path.path.segments.last()
            && segment.ident.unraw() == "raise"
        {
            self.record(segment.ident.span());
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.visit_tokens(item.tokens.clone());
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if let syn::Meta::List(arguments) = &attribute.meta {
            self.visit_tokens(arguments.tokens.clone());
        } else {
            visit::visit_attribute(self, attribute);
        }
    }
}
