mod imports_exports;

pub use imports_exports::{
    collect_exports, collect_imports, module_dependencies, Export, ExportKind, Import,
    ImportSpecifier,
};

use swc_common::{sync::Lrc, FileName, SourceMap};
use swc_ecma_ast::Module;
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, TsSyntax};

#[derive(Debug)]
pub struct SourceModule {
    pub id: String,
    pub source: String,
}

#[derive(Debug)]
pub struct ParsedModule {
    pub id: String,
    pub source: String,
    pub ast: Module,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

pub fn parse(source: &str) -> Result<Module, String> {
    Ok(parse_module("<entry>", source)?.ast)
}

pub fn parse_module(
    id: impl Into<String>,
    source: impl Into<String>,
) -> Result<ParsedModule, String> {
    let id = id.into();
    let source = source.into();

    let source_map: Lrc<SourceMap> = Default::default();

    let source_file =
        source_map.new_source_file(FileName::Custom(id.clone()).into(), source.clone());

    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: true,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*source_file),
        None,
    );

    let mut parser = Parser::new_from(lexer);

    let ast = parser
        .parse_module()
        .map_err(|error| format!("{error:?}"))?;

    let errors = parser.take_errors();

    if !errors.is_empty() {
        return Err(errors
            .into_iter()
            .map(|error| format!("{error:?}"))
            .collect::<Vec<_>>()
            .join("\n"));
    }

    let imports = collect_imports(&ast);
    let exports = collect_exports(&ast);

    Ok(ParsedModule {
        id,
        source,
        ast,
        imports,
        exports,
    })
}

pub fn parse_modules(
    modules: impl IntoIterator<Item = SourceModule>,
) -> Result<Vec<ParsedModule>, String> {
    modules
        .into_iter()
        .map(|module| parse_module(module.id, module.source))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_JSX: &str = r#"
        const MyComponent = () => {
            return (
                <div>
                    <h1>Hello, World!</h1>
                    <p>This is a test component.</p>
                </div>
            );
        };
    "#;

    const TEST_FUNCTIONAL_JSX: &str = r#"
        function MyComponent() {
            return (
                <div>
                    <h1>Hello, World!</h1>
                    <p>This is a test component.</p>
                </div>
            );
        }
    "#;

    const TEST_JSX_EXPORT: &str = r#"
        export const MyComponent = () => {
            return (
                <div>
                    <h1>Hello, World!</h1>
                    <p>This is a test component.</p>
                </div>
            );
        };
    "#;

    const TEST_JSX_EXPORT_DEFAULT: &str = r#"
        const MyComponent = () => {
            return <div>Hello</div>;
        }

        export default MyComponent;
    "#;

    const TEST_FUNCTIONAL_JSX_EXPORT: &str = r#"
        export function MyComponent() {
            return <div>Hello</div>;
        }
    "#;

    const TEST_FUNCTIONAL_JSX_EXPORT_DEFAULT: &str = r#"
        function MyComponent() {
            return <div>Hello</div>;
        }

        export default MyComponent;
    "#;

    #[test]
    fn parses_arrow_function_jsx() {
        assert!(parse(TEST_JSX).is_ok());
    }

    #[test]
    fn parses_function_declaration_jsx() {
        assert!(parse(TEST_FUNCTIONAL_JSX).is_ok());
    }

    #[test]
    fn parses_exported_arrow_component() {
        assert!(parse(TEST_JSX_EXPORT).is_ok());
    }

    #[test]
    fn parses_default_exported_arrow_component() {
        assert!(parse(TEST_JSX_EXPORT_DEFAULT).is_ok());
    }

    #[test]
    fn parses_exported_function_component() {
        assert!(parse(TEST_FUNCTIONAL_JSX_EXPORT).is_ok());
    }

    #[test]
    fn parses_default_exported_function_component() {
        assert!(parse(TEST_FUNCTIONAL_JSX_EXPORT_DEFAULT).is_ok());
    }

    #[test]
    fn parses_typescript_props() {
        let source = r#"
            interface Props {
                name: string;
                count?: number;
            }

            function MyComponent({ name, count = 0 }: Props) {
                return <div>{name}: {count}</div>;
            }
        "#;

        assert!(parse(source).is_ok());
    }

    #[test]
    fn parses_tsx_generics_and_types() {
        let source = r#"
            type Item<T> = {
                value: T;
            };

            const MyComponent = <T,>({ value }: Item<T>) => {
                return <div>{String(value)}</div>;
            };
        "#;

        assert!(parse(source).is_ok());
    }

    #[test]
    fn parses_fragments() {
        let source = r#"
            function MyComponent() {
                return (
                    <>
                        <div>One</div>
                        <div>Two</div>
                    </>
                );
            }
        "#;

        assert!(parse(source).is_ok());
    }

    #[test]
    fn parses_jsx_expressions() {
        let source = r#"
            function MyComponent() {
                const visible = true;
                const items = [1, 2, 3];

                return (
                    <div>
                        {visible && <span>Visible</span>}
                        {items.map(item => <span key={item}>{item}</span>)}
                    </div>
                );
            }
        "#;

        assert!(parse(source).is_ok());
    }

    #[test]
    fn parses_named_module() {
        let module =
            parse_module("src/components/MyComponent.tsx", TEST_JSX).expect("module should parse");

        assert_eq!(module.id, "src/components/MyComponent.tsx");
        assert_eq!(module.source, TEST_JSX);
        assert!(!module.ast.body.is_empty());
    }

    #[test]
    fn parses_multiple_modules() {
        let modules = vec![
            SourceModule {
                id: "src/App.tsx".into(),
                source: TEST_JSX.into(),
            },
            SourceModule {
                id: "src/Child.tsx".into(),
                source: TEST_FUNCTIONAL_JSX.into(),
            },
        ];

        let parsed = parse_modules(modules).expect("modules should parse");

        assert_eq!(parsed.len(), 2);

        assert_eq!(parsed[0].id, "src/App.tsx");
        assert_eq!(parsed[0].source, TEST_JSX);
        assert!(!parsed[0].ast.body.is_empty());

        assert_eq!(parsed[1].id, "src/Child.tsx");
        assert_eq!(parsed[1].source, TEST_FUNCTIONAL_JSX);
        assert!(!parsed[1].ast.body.is_empty());
    }

    #[test]
    fn fails_entire_module_batch_when_one_module_is_invalid() {
        let modules = vec![
            SourceModule {
                id: "src/App.tsx".into(),
                source: TEST_JSX.into(),
            },
            SourceModule {
                id: "src/Broken.tsx".into(),
                source: r#"
                    function Broken() {
                        return <div>
                    }
                "#
                .into(),
            },
        ];

        assert!(parse_modules(modules).is_err());
    }

    #[test]
    fn rejects_invalid_tsx() {
        let source = r#"
            function MyComponent() {
                return <div>
            }
        "#;

        assert!(parse(source).is_err());
    }

    #[test]
    fn rejects_invalid_typescript() {
        let source = r#"
            const value: = 123;
        "#;

        assert!(parse(source).is_err());
    }

    #[test]
    fn returns_a_non_empty_module() {
        let module = parse(TEST_JSX).expect("source should parse");

        assert!(!module.body.is_empty());
    }
}
