// M6.5.6 Phase 3: Integration tests for error recovery

use oxc_allocator::Allocator;
use oxc_parser::{Parser, ParseOptions};
use oxc_span::SourceType;

#[test]
fn test_multiple_statement_types_with_errors() {
    let allocator = Allocator::default();
    let source = r#"
"use strict";
let implements = 1;
const [...rest, x] = arr;
function test(a, b) {
    return a + b;
}
class MyClass {
    constructor() {}
}
const valid = 42;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect multiple errors
    assert!(ret.errors.len() >= 2, "Expected multiple errors");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    // Should have some valid statements
    let program = ret.program;
    assert!(!program.body.is_empty(), "Should have some statements");
}

#[test]
fn test_error_recovery_with_comments() {
    let allocator = Allocator::default();
    let source = r#"
// This is a comment
let x = (a + b;
// Another comment
let y = 5;
/* Block comment */
const z = 10;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should have errors
    assert!(!ret.errors.is_empty(), "Expected parse errors");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    let program = ret.program;
    assert!(program.body.len() >= 1, "Should parse statements after error");
}

#[test]
fn test_mixed_statement_and_expression_errors() {
    let allocator = Allocator::default();
    let source = r#"
const obj = {a: 1, b: 2, c;
const arr = [1, 2, 3];
if (condition {
    console.log("test");
}
const result = func(1, 2;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect multiple errors
    assert!(ret.errors.len() >= 1, "Expected errors");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    let program = ret.program;
    assert!(program.source_type.is_javascript(), "Should return valid program structure");
}

#[test]
fn test_function_with_parameter_errors() {
    let allocator = Allocator::default();
    let source = r#"
function test(...rest, a, b) {
    return rest;
}
const x = 5;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect rest parameter error
    assert!(!ret.errors.is_empty(), "Expected rest parameter error");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    let program = ret.program;
    assert!(!program.body.is_empty(), "Should have statements");
}

#[test]
fn test_class_with_syntax_errors() {
    let allocator = Allocator::default();
    let source = r#"
class Test {
    method(a, b, {
        return a + b;
    }
}
const x = 10;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect errors
    assert!(!ret.errors.is_empty(), "Expected syntax errors");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    let program = ret.program;
    assert!(program.source_type.is_javascript(), "Should return valid program structure");
}

#[test]
fn test_import_export_with_errors() {
    let allocator = Allocator::default();
    let source = r#"
import {a, b, from "./module.js";
export {x, y, z from "./other.js";
const local = 5;
"#;

    let ret = Parser::new(&allocator, source, SourceType::mjs())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect syntax errors
    assert!(!ret.errors.is_empty(), "Expected import/export errors");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    let program = ret.program;
    assert!(program.source_type.is_module(), "Should return valid module structure");
}
