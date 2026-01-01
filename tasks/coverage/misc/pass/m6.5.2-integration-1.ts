// Test 26: Nested functions with errors
function outer(a b) {
    function inner(x y) {
        return x + y;
    }
    return inner(a, b);
}
