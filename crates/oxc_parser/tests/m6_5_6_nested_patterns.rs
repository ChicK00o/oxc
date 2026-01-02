// M6.5.6 Phase 3: Nested pattern error recovery tests

use oxc_allocator::Allocator;
use oxc_parser::{Parser, ParseOptions};
use oxc_span::SourceType;

#[test]
fn test_nested_destructuring_with_errors() {
    let allocator = Allocator::default();
    let source = r#"
const {a: {b: [...rest, x]}} = obj;
const y = 5;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect rest-not-last error in nested pattern
    assert!(!ret.errors.is_empty(), "Expected rest element error");

    // Parser should NOT panic in recovery mode
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    // Should continue parsing after error
    let program = ret.program;
    assert!(program.body.len() >= 1, "Should have statements despite nested errors");
}

#[test]
fn test_nested_array_patterns() {
    let allocator = Allocator::default();
    let source = r#"
const [[...rest, a], b] = arr;
const z = 10;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect error
    assert!(!ret.errors.is_empty(), "Expected pattern error");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    let program = ret.program;
    assert!(!program.body.is_empty(), "Should have statements");
}

#[test]
fn test_deeply_nested_patterns_with_strict_mode() {
    let allocator = Allocator::default();
    let source = r#"
"use strict";
const {a: {b: {c: implements}}} = obj;
function test() { return 42; }
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect reserved word error in nested pattern
    assert!(!ret.errors.is_empty(), "Expected reserved word error");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    // Should parse the function after the error
    let program = ret.program;
    assert!(program.body.len() >= 2, "Should parse function after error");
}
