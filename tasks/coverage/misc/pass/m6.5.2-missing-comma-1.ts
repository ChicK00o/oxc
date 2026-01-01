// Test 1: Missing comma between two parameters
function test1(a: string b: number) {
    return a + b;
}

// Subsequent function should parse correctly
function next1() {
    return 42;
}
