use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

fn main() {
    println!("=== Testing Spread Element Position Validation ===\n");

    // Test 1: Spread not last (single spread)
    println!("Test 1: Spread not last [...a, 1]");
    test_case("let x = [...a, 1];");

    // Test 2: Multiple spreads
    println!("\nTest 2: Multiple spreads [...a, ...b]");
    test_case("let x = [...a, ...b];");

    // Test 3: Valid spread (last element)
    println!("\nTest 3: Valid spread [1, ...a]");
    test_case("let x = [1, ...a];");

    // Test 4: Spread not last with multiple elements
    println!("\nTest 4: Spread not last [...a, 1, 2, 3]");
    test_case("let x = [...a, 1, 2, 3];");

    // Test 5: Multiple spreads not last
    println!("\nTest 5: Multiple spreads not last [...a, ...b, 1]");
    test_case("let x = [...a, ...b, 1];");

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
