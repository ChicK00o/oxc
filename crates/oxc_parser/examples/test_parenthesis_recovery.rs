use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

fn main() {
    println!("=== Testing Parenthesized Expression Error Recovery ===\n");

    // Test 1: Trailing comma in parentheses
    println!("Test 1: Trailing comma (1, 2,)");
    test_case("let x = (1, 2,);");

    // Test 2: Empty parentheses
    println!("\nTest 2: Empty parentheses ()");
    test_case("let x = ();");

    // Test 3: Valid parenthesized expression
    println!("\nTest 3: Valid parentheses (1, 2)");
    test_case("let x = (1, 2);");

    // Test 4: Multiple errors
    println!("\nTest 4: Multiple errors");
    test_case("let a = (1, 2,); let b = (); let c = (3);");

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
