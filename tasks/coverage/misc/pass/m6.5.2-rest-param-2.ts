// Test 7: Rest parameter in middle of list
function test7(a: string, ...rest: number[], b: boolean) {
    return [a, ...rest, b];
}
