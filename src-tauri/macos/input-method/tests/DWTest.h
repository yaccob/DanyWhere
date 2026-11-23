// A minimal test runner (the Command Line Tools have no XCTest).
//
//   DW_TEST(name) { CHECK(...); CHECK_EQ_OBJ(a, b); CHECK_EQ_INT(a, b); }
//
// Every DW_TEST registers itself; main() in DWTest.m runs them in file
// order and exits 1 if any check failed. DW_TEST_FILTER=<substring> runs
// only the tests whose name contains it.

#import <Foundation/Foundation.h>

typedef void (*DWTestFunction)(void);

void DWTestRegister(const char *name, DWTestFunction function);
void DWTestCheck(BOOL ok, const char *expression, const char *file, int line, NSString *detail);
/// Marks the running test as skipped (its checks so far still count).
void DWTestSkip(NSString *why);
/// Runs the run loop and polls `condition` until it holds or `timeout` passes.
BOOL DWWaitUntil(NSTimeInterval timeout, BOOL (^condition)(void));

#define DW_TEST(name)                                                         \
    static void name(void);                                                   \
    __attribute__((constructor)) static void name##_register(void) {          \
        DWTestRegister(#name, name);                                          \
    }                                                                         \
    static void name(void)

#define CHECK(cond) DWTestCheck(!!(cond), #cond, __FILE__, __LINE__, nil)

#define CHECK_EQ_OBJ(a, b)                                                    \
    do {                                                                      \
        id _dw_a = (a);                                                       \
        id _dw_b = (b);                                                       \
        BOOL _dw_ok = (_dw_a == _dw_b) || [_dw_a isEqual:_dw_b];              \
        DWTestCheck(_dw_ok, #a " == " #b, __FILE__, __LINE__,                 \
                    _dw_ok ? nil : [NSString stringWithFormat:@"got <%@>, expected <%@>", _dw_a, _dw_b]); \
    } while (0)

#define CHECK_EQ_INT(a, b)                                                    \
    do {                                                                      \
        long long _dw_a = (long long)(a);                                     \
        long long _dw_b = (long long)(b);                                     \
        DWTestCheck(_dw_a == _dw_b, #a " == " #b, __FILE__, __LINE__,         \
                    _dw_a == _dw_b ? nil : [NSString stringWithFormat:@"got %lld, expected %lld", _dw_a, _dw_b]); \
    } while (0)

/// UTF-8 data of a string literal.
static inline NSData *DWData(NSString *string) {
    return [string dataUsingEncoding:NSUTF8StringEncoding];
}

/// A string from UTF-8 data.
static inline NSString *DWString(NSData *data) {
    return [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
}
