//! Tests for M6.5.6: Identifier & Expression Error Recovery
//! Tests reserved words, number literals, parentheses, spread elements, class properties, and binding patterns

use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser, ParserReturn};
use oxc_span::SourceType;

fn parse_with_recovery(source: &str) -> ParserReturn {
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let options = ParseOptions {
        recover_from_errors: true,
        ..ParseOptions::default()
    };
    Parser::new(&allocator, source, source_type).with_options(options).parse()
}

// ==================== Phase 1: Reserved Word Identifier Tests ====================

#[test]
fn test_reserved_word_as_variable_name() {
    let source = r#"
        let import = 5;
        let x = 10;
    "#;

    let result = parse_with_recovery(source);

    // Should have 1 error for reserved word
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("import"));
    assert!(result.errors[0].message.contains("reserved"));

    // Both declarations should be in AST
    assert_eq!(result.program.body.len(), 2);
}

#[test]
fn test_multiple_reserved_words() {
    let source = r#"
        let class = 1;
        const enum = 2;
        var return = 3;
        let valid = 4;
    "#;

    let result = parse_with_recovery(source);

    // Should have 3 errors (class, enum, return)
    assert_eq!(result.errors.len(), 3);

    // All 4 declarations should be parsed
    assert_eq!(result.program.body.len(), 4);
}

#[test]
fn test_reserved_word_as_function_name() {
    let source = r#"
        function class() {
            return 42;
        }
        class();
    "#;

    let result = parse_with_recovery(source);

    // Should have at least 1 error for function name
    assert!(!result.errors.is_empty());
    assert!(result.errors.iter().any(|e| e.message.contains("class")));

    // Function and call should be in AST
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_reserved_word_in_expression() {
    let source = r#"
        let x = import + 5;
        let y = 10;
    "#;

    let result = parse_with_recovery(source);

    // Should have error for using 'import' as identifier in expression
    assert!(!result.errors.is_empty());

    // Both declarations should be parsed
    assert_eq!(result.program.body.len(), 2);
}

#[test]
fn test_reserved_word_multiple_in_same_statement() {
    let source = r#"
        let class = import + export;
        let valid = 42;
    "#;

    let result = parse_with_recovery(source);

    // Should have errors for class, import, export
    assert!(result.errors.len() >= 3);

    // Both declarations should be parsed
    assert_eq!(result.program.body.len(), 2);
}

#[test]
fn test_reserved_word_with_subsequent_code() {
    let source = r#"
        let import = 5;
        let x = 10;
        function test() {
            return x + import;
        }
        test();
    "#;

    let result = parse_with_recovery(source);

    // Should have errors for 'import' usage
    assert!(!result.errors.is_empty());

    // All statements should be parsed
    assert_eq!(result.program.body.len(), 4);
}

#[test]
fn test_reserved_word_in_object() {
    let source = r#"
        let obj = {
            class: 1,
            import: 2
        };
    "#;

    let result = parse_with_recovery(source);

    // Property names can be reserved words - should be no errors
    assert_eq!(result.errors.len(), 0);
    assert_eq!(result.program.body.len(), 1);
}

#[test]
fn test_break_as_identifier() {
    let source = r#"
        let break = 5;
        let x = break + 10;
    "#;

    let result = parse_with_recovery(source);

    // Should have errors for 'break' usage
    assert!(result.errors.len() >= 2);

    // Both statements should be parsed
    assert_eq!(result.program.body.len(), 2);
}

#[test]
fn test_yield_as_identifier() {
    let source = r#"
        let yield = 5;
        let x = yield + 10;
    "#;

    let result = parse_with_recovery(source);

    // Should have errors for 'yield' usage
    assert!(!result.errors.is_empty());

    // Both statements should be parsed
    assert_eq!(result.program.body.len(), 2);
}

#[test]
fn test_await_as_identifier() {
    let source = r#"
        let await = 5;
        let x = await + 10;
    "#;

    let result = parse_with_recovery(source);

    // Should have errors for 'await' usage
    assert!(!result.errors.is_empty());

    // Both statements should be parsed
    assert_eq!(result.program.body.len(), 2);
}
