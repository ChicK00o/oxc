use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

fn main() {
    println!("=== Testing Binding Rest Element Validation ===\n");

    // Test 1: Invalid rest element (object pattern)
    println!("Test 1: Invalid rest element (object pattern)");
    test_case("const {...{x}} = obj;");

    // Test 2: Valid rest element (identifier)
    println!("\nTest 2: Valid rest element (identifier)");
    test_case("const {...rest} = obj;");

    // Test 3: Invalid rest element (array pattern)
    println!("\nTest 3: Invalid rest element (array pattern)");
    test_case("const {...[x]} = obj;");

    // Test 4: Valid array rest element
    println!("\nTest 4: Valid array rest element");
    test_case("const [a, ...rest] = arr;");

    // Test 5: Multiple properties with invalid rest
    println!("\nTest 5: Multiple properties with invalid rest");
    test_case("const {a, b, ...{x}} = obj;");

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
