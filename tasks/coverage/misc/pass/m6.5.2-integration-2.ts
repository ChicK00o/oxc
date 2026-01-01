// Test 27: Function with multiple error types
function complex(...rest: any[] a: string b c = 10) {
    return rest.length + a.length + b + c;
}
