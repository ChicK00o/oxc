// Test 6: Rest parameter followed by regular parameter
function test6(...rest: any[], a: string) {
    return rest.concat(a);
}
