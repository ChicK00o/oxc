use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

fn main() {
    println!("=== Testing Class Property Semicolon Recovery (ASI) ===\n");

    // Test 1: Missing semicolon between properties
    println!("Test 1: Missing semicolon (x = 5 y = 6)");
    test_case("class C { x = 5 y = 6 }");

    // Test 2: Multiple properties without semicolons
    println!("\nTest 2: Multiple properties without semicolons");
    test_case("class C { x = 1 y = 2 z = 3 }");

    // Test 3: Valid with semicolons
    println!("\nTest 3: Valid with semicolons");
    test_case("class C { x = 5; y = 6; }");

    // Test 4: Mixed (some with, some without semicolons)
    println!("\nTest 4: Mixed semicolons");
    test_case("class C { x = 5; y = 6 z = 7; }");

    // Test 5: Property followed by method
    println!("\nTest 5: Property without semicolon before method");
    test_case("class C { x = 5 method() {} }");

    println!("\n=== All tests completed ===");
}

fn test_case(source: &str) {
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let options = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
    let result = Parser::new(&allocator, source, source_type).with_options(options).parse();

    println!("  Source: {}", source.trim());
    println!("  Errors: {}", result.errors.len());
    for (i, error) in result.errors.iter().enumerate() {
        println!("    {}: {}", i + 1, error.message);
    }
    println!("  Statements: {}", result.program.body.len());

    if !result.errors.is_empty() && !result.program.body.is_empty() {
        println!("  ✓ PASS (recovered)");
    } else if result.errors.is_empty() {
        println!("  ✓ PASS (no errors)");
    } else {
        println!("  ✗ FAIL");
    }
}
