// Test 18: Method body parsed despite param error
class C2 {
    process(x: number y: string) {
        const result = x + y.length;
        return result > 0;
    }
}
