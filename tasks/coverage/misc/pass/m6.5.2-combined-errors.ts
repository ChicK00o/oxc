// Test 37+: Combined error scenarios
function broken1(a b c)
function broken2(...rest: any[] x: string) { return rest + x; }
const arrow = (p q r) => p + q + r;
class Broken {
    method1(x y) { return x + y; }
    method2(...args: number[] z: string) { return args.length + z; }
}
function working() {
    return "This should parse correctly!";
}
