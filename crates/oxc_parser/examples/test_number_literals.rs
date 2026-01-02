use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

fn main() {
    println!("=== Testing Number Literal Error Recovery ===\n");

    // Test 1: Invalid binary literal
    println!("Test 1: Invalid binary (0b2)");
    test_case("let a = 0b2; let b = 10;");

    // Test 2: Invalid octal literal
    println!("\nTest 2: Invalid octal (0o888)");
    test_case("let a = 0o888; let b = 10;");

    // Test 3: Invalid hex literal
    println!("\nTest 3: Invalid hex (0xGGG)");
    test_case("let a = 0xGGG; let b = 10;");

    // Test 4: Multiple invalid numbers
    println!("\nTest 4: Multiple invalid numbers");
    test_case("let bin = 0b2; let oct = 0o9; let hex = 0xZ; let valid = 42;");

    // Test 5: Invalid number in expression
    println!("\nTest 5: Invalid number in expression");
    test_case("let x = 0b101 + 0b2;");

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
