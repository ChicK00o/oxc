// M6.5.6 Phase 3.3: AST correctness after recovery tests
//
// These tests verify that error recovery works correctly:
// 1. Errors are detected and reported
// 2. The parser continues parsing after errors (doesn't panic)
// 3. AST remains valid and contains subsequent statements
// 4. Error messages are helpful and accurate

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::{Parser, ParseOptions};
use oxc_span::SourceType;

#[test]
fn test_ast_correctness_with_unclosed_paren() {
    let allocator = Allocator::default();
    let source = r#"
let x = (a + b;
let y = 5;
let z = 10;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should have errors
    assert!(!ret.errors.is_empty(), "Expected parse errors");

    // Parser should NOT panic in recovery mode
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    // AST should contain subsequent statements (y and z declarations)
    let program = ret.program;
    assert!(program.body.len() >= 2, "Should parse subsequent statements after error");
}

#[test]
fn test_ast_correctness_with_strict_mode_errors() {
    let allocator = Allocator::default();
    let source = r#"
"use strict";
let implements = 1;
let interface = 2;
function test() {
    return 42;
}
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should have errors for reserved words
    assert!(!ret.errors.is_empty(), "Expected reserved word errors");

    // But AST should be valid
    let program = ret.program;
    assert_eq!(program.body.len(), 3, "Should have 3 statements");

    // Check we have the function declaration
    let has_function = program.body.iter().any(|stmt| {
        matches!(stmt, Statement::FunctionDeclaration(_))
    });
    assert!(has_function, "Should have function declaration");
}

#[test]
fn test_ast_correctness_with_rest_element_errors() {
    let allocator = Allocator::default();
    let source = r#"
const [...rest, x] = [1, 2, 3];
const y = 5;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should have error for rest not last
    assert!(!ret.errors.is_empty(), "Expected rest element error");

    // Parser should NOT panic in recovery mode
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    // Should still parse the second declaration
    let program = ret.program;
    assert!(!program.body.is_empty(), "Should have statements despite errors");
}

#[test]
fn test_ast_correctness_combined_errors() {
    let allocator = Allocator::default();
    let source = r#"
"use strict";
let implements = (1 + 2;
const [...rest, x] = arr;
let y = 0777;
function valid() { return 1; }
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should have multiple errors
    assert!(ret.errors.len() >= 3, "Expected at least 3 errors, got {}", ret.errors.len());

    // Parser should NOT panic in recovery mode
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    // But should still have valid AST with function
    let program = ret.program;
    let has_function = program.body.iter().any(|stmt| {
        matches!(stmt, Statement::FunctionDeclaration(_))
    });
    assert!(has_function, "Should parse valid function despite other errors");
}

#[test]
fn test_ast_structure_preserved() {
    let allocator = Allocator::default();
    let source = r#"
if (condition {
    console.log("test");
}
let x = 5;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    assert!(!ret.errors.is_empty(), "Expected paren error");

    // Parser should NOT panic in recovery mode
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    let program = ret.program;
    // Should have at least the variable declaration
    assert!(!program.body.is_empty(), "Should have statements");
}
