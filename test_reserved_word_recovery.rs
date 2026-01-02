use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

fn main() {
    // Test 1: Reserved word as variable name
    println!("=== Test 1: Reserved word as variable name ===");
    let source = r#"
        let import = 5;
        let x = 10;
    "#;

    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let options = ParseOptions {
        recover_from_errors: true,
        ..ParseOptions::default()
    };
    let result = Parser::new(&allocator, source, source_type).with_options(options).parse();

    println!("Errors: {}", result.errors.len());
    for error in &result.errors {
        println!("  - {}", error.message);
    }
    println!("Program body length: {}", result.program.body.len());
    println!();

    // Test 2: Multiple reserved words
    println!("=== Test 2: Multiple reserved words ===");
    let source2 = r#"
        let class = 1;
        const enum = 2;
        var return = 3;
        let valid = 4;
    "#;

    let allocator2 = Allocator::default();
    let result2 = Parser::new(&allocator2, source2, source_type).with_options(options).parse();

    println!("Errors: {}", result2.errors.len());
    for error in &result2.errors {
        println!("  - {}", error.message);
    }
    println!("Program body length: {}", result2.program.body.len());
    println!();

    // Test 3: Reserved word in expression
    println!("=== Test 3: Reserved word in expression ===");
    let source3 = r#"
        let x = import + 5;
        let y = 10;
    "#;

    let allocator3 = Allocator::default();
    let result3 = Parser::new(&allocator3, source3, source_type).with_options(options).parse();

    println!("Errors: {}", result3.errors.len());
    for error in &result3.errors {
        println!("  - {}", error.message);
    }
    println!("Program body length: {}", result3.program.body.len());
}
