// Test 8: Multiple rest parameters (invalid but should parse)
function test8(...first: string[], ...second: number[]) {
    return first.length + second.length;
}
