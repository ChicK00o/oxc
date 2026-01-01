// Test 15: Arrow body still parsed after param error
const fn3 = (a: string b: number) => {
    const result = a.length + b;
    if (result > 10) {
        return true;
    }
    return false;
};
