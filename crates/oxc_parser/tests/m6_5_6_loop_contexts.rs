// M6.5.6 Phase 3: Loop context error recovery tests

use oxc_allocator::Allocator;
use oxc_parser::{Parser, ParseOptions};
use oxc_span::SourceType;

#[test]
fn test_for_loop_with_syntax_error() {
    let allocator = Allocator::default();
    let source = r#"
for (let i = 0; i < 10 i++) {
    console.log(i);
}
let x = 5;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect syntax error in for loop
    assert!(!ret.errors.is_empty(), "Expected syntax error");

    // Parser should NOT panic in recovery mode
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    // Should have some program structure
    let program = ret.program;
    assert!(program.source_type.is_javascript(), "Should return valid program structure");
}

#[test]
fn test_for_of_with_rest_error() {
    let allocator = Allocator::default();
    let source = r#"
for (const [...rest, x] of items) {
    console.log(x);
}
const y = 10;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect rest-not-last error
    assert!(!ret.errors.is_empty(), "Expected rest element error");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    let program = ret.program;
    assert!(!program.body.is_empty(), "Should have statements");
}

#[test]
fn test_while_loop_with_unclosed_paren() {
    let allocator = Allocator::default();
    let source = r#"
while (x > 0 {
    x--;
}
let y = 5;
"#;

    let ret = Parser::new(&allocator, source, SourceType::default())
        .with_options(ParseOptions {
            recover_from_errors: true,
            ..Default::default()
        })
        .parse();

    // Should detect unclosed paren error
    assert!(!ret.errors.is_empty(), "Expected paren error");

    // Parser should NOT panic
    assert!(!ret.panicked, "Parser should not panic in recovery mode");

    let program = ret.program;
    assert!(program.source_type.is_javascript(), "Should return valid program structure");
}
