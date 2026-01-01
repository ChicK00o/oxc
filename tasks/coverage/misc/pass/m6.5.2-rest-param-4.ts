// Test 9: All parameters still in AST despite rest error
function test9(...args: any[], x: number, y: string) {
    // All three parameters should be in AST
    console.log(args, x, y);
}
