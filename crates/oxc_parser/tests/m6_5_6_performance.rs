// M6.5.6 Phase 4: Performance regression tests
//
// These tests verify that error recovery features don't significantly
// impact parse time for valid code (which is the common case).

use oxc_allocator::Allocator;
use oxc_parser::{Parser, ParseOptions};
use oxc_span::SourceType;
use std::time::Instant;

/// Test that recovery mode has minimal overhead for valid code
#[test]
fn test_recovery_mode_overhead_on_valid_code() {
    let allocator = Allocator::default();

    // Large valid program to measure overhead
    let source = r#"
"use strict";
const x = 1;
const y = 2;
const z = 3;
function add(a, b) {
    return a + b;
}
function multiply(a, b) {
    return a * b;
}
class Calculator {
    constructor() {
        this.result = 0;
    }
    add(x) {
        this.result += x;
        return this;
    }
    multiply(x) {
        this.result *= x;
        return this;
    }
}
const calc = new Calculator();
calc.add(5).multiply(3);
for (let i = 0; i < 10; i++) {
    console.log(i);
}
const arr = [1, 2, 3, 4, 5];
const [first, ...rest] = arr;
const obj = {a: 1, b: 2, c: 3};
const {a, b, c} = obj;
"#.repeat(100); // Repeat to make it larger

    // Warm up
    for _ in 0..5 {
        let allocator = Allocator::default();
        Parser::new(&allocator, &source, SourceType::default())
            .parse();
    }

    // Benchmark without recovery
    let start = Instant::now();
    for _ in 0..50 {
        let allocator = Allocator::default();
        Parser::new(&allocator, &source, SourceType::default())
            .parse();
    }
    let duration_no_recovery = start.elapsed();

    // Benchmark with recovery
    let start = Instant::now();
    for _ in 0..50 {
        let allocator = Allocator::default();
        Parser::new(&allocator, &source, SourceType::default())
            .with_options(ParseOptions {
                recover_from_errors: true,
                ..Default::default()
            })
            .parse();
    }
    let duration_with_recovery = start.elapsed();

    // Calculate overhead percentage
    let overhead = (duration_with_recovery.as_nanos() as f64 / duration_no_recovery.as_nanos() as f64 - 1.0) * 100.0;

    println!("Without recovery: {:?}", duration_no_recovery);
    println!("With recovery:    {:?}", duration_with_recovery);
    println!("Overhead:         {:.2}%", overhead);

    // Assert overhead is reasonable (< 10%)
    // Note: This is lenient since recovery mode adds minimal checks
    assert!(overhead < 10.0, "Recovery mode overhead too high: {:.2}%", overhead);
}

/// Test that recovery features work correctly
#[test]
fn test_recovery_features_functional() {
    let allocator = Allocator::default();

    // Test with errors
    let source = r#"
"use strict";
let implements = 1;
const [...rest, x] = arr;
let x = (a + b;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect multiple errors
    assert!(ret.errors.len() >= 2, "Expected at least 2 errors");

    // Should not panic
    assert!(!ret.panicked, "Should not panic in recovery mode");

    // Program structure should be valid
    assert!(ret.program.source_type.is_javascript(), "Should return valid program");
}
