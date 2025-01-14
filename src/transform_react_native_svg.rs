use std::{
    collections::HashMap,
    rc::Rc,
    cell::RefCell
};

use swc_core::{
    common::{DUMMY_SP, Span, comments::{Comment, CommentKind, Comments}},
    ecma::{
        ast::*,
        visit::{VisitMut, VisitMutWith}
    },
};
use linked_hash_set::LinkedHashSet;

pub struct Visitor<'a> {
    replaced_components: Rc<RefCell<LinkedHashSet<String>>>,
    unsupported_components: Rc<RefCell<LinkedHashSet<String>>>,
    comments: &'a dyn Comments,
}

impl<'a> Visitor<'a> {
    pub fn new(comments: &'a dyn Comments) -> Self {
        Visitor {
            replaced_components: Rc::new(RefCell::new(LinkedHashSet::new())),
            unsupported_components: Rc::new(RefCell::new(LinkedHashSet::new())),
            comments,
        }
    }
}

impl VisitMut for Visitor<'_> {
    fn visit_mut_module(&mut self, n: &mut Module) {
        let mut svg_element_visitor = SvgElementVisitor::new(
            self.replaced_components.clone(),
            self.unsupported_components.clone()
        );
        n.visit_mut_with(&mut svg_element_visitor);

        let mut import_decl_visitor = ImportDeclVisitor::new(self.replaced_components.clone());
        n.visit_mut_with(&mut import_decl_visitor);

        if let Some(span) = import_decl_visitor.import_decl_span {
            let component_list = self.unsupported_components.borrow().clone().into_iter().collect::<Vec<String>>().join(", ");
            self.comments.add_trailing_comments(span.hi, vec![
                Comment {
                    kind: CommentKind::Block,
                    span: DUMMY_SP,
                    text: format!(" SVGR has dropped some elements not supported by react-native-svg: {} ", component_list).into(),
                }
            ]);
        }
    }
}

struct SvgElementVisitor {
    replaced_components: Rc<RefCell<LinkedHashSet<String>>>,
    unsupported_components: Rc<RefCell<LinkedHashSet<String>>>,
}

impl SvgElementVisitor {
    fn new(
        replaced_components: Rc<RefCell<LinkedHashSet<String>>>,
        unsupported_components: Rc<RefCell<LinkedHashSet<String>>>
    ) -> Self {
        SvgElementVisitor {
            replaced_components,
            unsupported_components,
        }
    }
}

impl VisitMut for SvgElementVisitor {
    fn visit_mut_jsx_element(&mut self, n: &mut JSXElement) {
        if let JSXElementName::Ident(ident) = &mut n.opening.name {
            if ident.sym.to_string() == "svg" {
                let mut jsx_element_visitor = JSXElementVisitor::new(
                    self.replaced_components.clone(),
                    self.unsupported_components.clone(),
                );
                ident.sym = "Svg".into();
                if let Some(closing) = &mut n.closing {
                    if let JSXElementName::Ident(ident) = &mut closing.name {
                        ident.sym = "Svg".into();
                    }
                }
                n.visit_mut_with(&mut jsx_element_visitor);
                return;
            }
        }
    }
}

struct JSXElementVisitor {
    element_to_component: HashMap<&'static str, &'static str>,

    replaced_components: Rc<RefCell<LinkedHashSet<String>>>,
    unsupported_components: Rc<RefCell<LinkedHashSet<String>>>,
}

impl JSXElementVisitor {
    fn new(
        replaced_components: Rc<RefCell<LinkedHashSet<String>>>,
        unsupported_components: Rc<RefCell<LinkedHashSet<String>>>
    ) -> Self {
        JSXElementVisitor {
            element_to_component: get_element_to_component(),
            replaced_components,
            unsupported_components,
        }
    }

    fn replace_element(&self, n: &mut JSXElement) -> bool {
        if let JSXElementName::Ident(ident) = &mut n.opening.name {
            let element = ident.sym.to_string();
            if let Some(component) = self.element_to_component.get(&element.as_str()) {
                self.replaced_components.borrow_mut().insert(component.to_string());
                ident.sym = component.clone().into();
                if let Some(closing) = &mut n.closing {
                    if let JSXElementName::Ident(ident) = &mut closing.name {
                        ident.sym = component.clone().into();
                    }
                }
            } else {
                // Remove element if not supported
                self.unsupported_components.borrow_mut().insert(element);
                return true;
            }
        }
        false
    }
}

impl VisitMut for JSXElementVisitor {
    fn visit_mut_jsx_element(&mut self, n: &mut JSXElement) {
        n.visit_mut_children_with(self);

        let mut i = n.children.len();
        while i > 0 {
            i -= 1;
            if let JSXElementChild::JSXElement(jsx_element) = &mut n.children[i] {
                let unsupported = self.replace_element(jsx_element);
                if unsupported {
                    n.children.remove(i);
                }
            }
        }
    }
}

fn get_element_to_component() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("svg", "Svg"),
        ("circle", "Circle"),
        ("clipPath", "ClipPath"),
        ("ellipse", "Ellipse"),
        ("g", "G"),
        ("linearGradient", "LinearGradient"),
        ("radialGradient", "RadialGradient"),
        ("line", "Line"),
        ("path", "Path"),
        ("pattern", "Pattern"),
        ("polygon", "Polygon"),
        ("polyline", "Polyline"),
        ("rect", "Rect"),
        ("symbol", "Symbol"),
        ("text", "Text"),
        ("textPath", "TextPath"),
        ("tspan", "TSpan"),
        ("use", "Use"),
        ("defs", "Defs"),
        ("stop", "Stop"),
        ("mask", "Mask"),
        ("image", "Image"),
        ("foreignObject", "ForeignObject"),
    ])
}

struct ImportDeclVisitor {
    replaced_components: Rc<RefCell<LinkedHashSet<String>>>,
    import_decl_span: Option<Span>,
}

impl ImportDeclVisitor {
    fn new(replaced_components: Rc<RefCell<LinkedHashSet<String>>>) -> Self {
        ImportDeclVisitor {
            replaced_components,
            import_decl_span: None,
        }
    }
}

impl VisitMut for ImportDeclVisitor {
    fn visit_mut_import_decl(&mut self, n: &mut ImportDecl) {
        if n.src.value.to_string() == "react-native-svg" {
            for component in self.replaced_components.borrow().iter() {
                if n.specifiers.iter().any(|specifier| {
                    if let ImportSpecifier::Named(named) = specifier {
                        if named.local.sym.to_string() == component.to_string() {
                            return true;
                        }
                    }
                    false
                }) {
                    break;
                }

                n.specifiers.push(ImportSpecifier::Named(ImportNamedSpecifier {
                    local: Ident::new(
                        component.clone().into(),
                        DUMMY_SP
                    ),
                    imported: None,
                    span: DUMMY_SP,
                    is_type_only: false,
                }));
            }

            self.import_decl_span = Some(n.span);
        } else if n.src.value.to_string() == "expo" {
            n.specifiers.push(ImportSpecifier::Named(ImportNamedSpecifier {
                local: Ident::new(
                    "Svg".into(),
                    DUMMY_SP
                ),
                imported: None,
                span: DUMMY_SP,
                is_type_only: false,
            }));

            self.import_decl_span = Some(n.span);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use swc_core::{
        common::{SourceMap, FileName, comments::SingleThreadedComments},
        ecma::{
            ast::*,
            parser::{lexer::Lexer, Parser, StringInput, Syntax, EsConfig},
            codegen::{text_writer::JsWriter, Emitter, Config},
            visit::{FoldWith, as_folder}
        },
    };

    use super::*;

    fn code_test(input: &str, expected: &str) {
        let cm = Arc::<SourceMap>::default();
        let fm = cm.new_source_file(FileName::Anon, input.to_string());

        let lexer = Lexer::new(
            Syntax::Es(EsConfig {
                decorators: true,
                jsx: true,
                ..Default::default()
            }),
            EsVersion::EsNext,
            StringInput::from(&*fm),
            None,
        );

        let mut parser = Parser::new_from(lexer);
        let module = parser.parse_module().unwrap();

        let comments = SingleThreadedComments::default();
        let module = module.fold_with(&mut as_folder(Visitor::new(&comments)));

        let mut buf = vec![];
        let mut emitter = Emitter {
            cfg: Config {
                ..Default::default()
            },
            cm: cm.clone(),
            comments: Some(&comments),
            wr: JsWriter::new(cm, "", &mut buf, None),
        };
        emitter.emit_module(&module).unwrap();
        let result = String::from_utf8_lossy(&buf).to_string();

        assert_eq!(result, expected);
    }

    #[test]
    fn should_transform_elements() {
        code_test(
            r#"<svg><div/></svg>;"#,
            r#"<Svg></Svg>;"#,
        );
    }

    #[test]
    fn should_add_import() {
        code_test(
            r#"import Svg from 'react-native-svg'; <svg><g/><div/></svg>;"#,
            r#"import Svg, { G } from 'react-native-svg'; /* SVGR has dropped some elements not supported by react-native-svg: div */ <Svg><G/></Svg>;"#,
        );
    }
}
