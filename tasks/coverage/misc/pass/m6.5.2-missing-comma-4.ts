// Test 4: Function body still parsed despite parameter error
function test4(x y z) {
    const result = x + y + z;
    if (result > 0) {
        return result;
    }
    return -result;
}
