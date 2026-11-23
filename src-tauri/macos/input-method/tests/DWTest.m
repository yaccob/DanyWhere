#import "DWTest.h"

#import "DWLog.h"

typedef struct {
    const char *name;
    DWTestFunction function;
} DWRegisteredTest;

static DWRegisteredTest gTests[256];
static int gTestCount = 0;
static int gChecks = 0;
static int gFailedChecks = 0;
static BOOL gCurrentFailed = NO;
static NSString *gCurrentSkip = nil;

void DWTestRegister(const char *name, DWTestFunction function) {
    if (gTestCount >= (int)(sizeof gTests / sizeof gTests[0])) {
        fprintf(stderr, "too many tests\n");
        exit(2);
    }
    gTests[gTestCount++] = (DWRegisteredTest){name, function};
}

void DWTestCheck(BOOL ok, const char *expression, const char *file, int line, NSString *detail) {
    gChecks++;
    if (ok) return;
    gFailedChecks++;
    gCurrentFailed = YES;
    const char *base = strrchr(file, '/');
    fprintf(stdout, "    FAILED %s:%d: %s%s%s\n", base ? base + 1 : file, line, expression,
            detail ? " - " : "", detail ? detail.UTF8String : "");
}

void DWTestSkip(NSString *why) {
    gCurrentSkip = [why copy];
}

BOOL DWWaitUntil(NSTimeInterval timeout, BOOL (^condition)(void)) {
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:timeout];
    while (!condition()) {
        if ([deadline timeIntervalSinceNow] <= 0) return NO;
        [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.005]];
    }
    return YES;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        // Tests log into a file of their own when they configure one; by
        // default the shared log writes nowhere.
        [[DWLog shared] configureWithPath:nil maxBytes:0];
        const char *filter = getenv("DW_TEST_FILTER");
        int passed = 0, failed = 0, skipped = 0;
        NSDate *start = [NSDate date];
        for (int i = 0; i < gTestCount; i++) {
            if (filter && *filter && !strstr(gTests[i].name, filter)) continue;
            gCurrentFailed = NO;
            gCurrentSkip = nil;
            int checksBefore = gChecks;
            NSDate *testStart = [NSDate date];
            @autoreleasepool {
                @try {
                    gTests[i].function();
                } @catch (NSException *exception) {
                    gCurrentFailed = YES;
                    gFailedChecks++;
                    fprintf(stdout, "    EXCEPTION %s: %s\n", exception.name.UTF8String,
                            exception.reason.UTF8String ?: "");
                }
            }
            int ms = (int)([[NSDate date] timeIntervalSinceDate:testStart] * 1000);
            if (gCurrentFailed) {
                failed++;
                fprintf(stdout, "FAIL %s (%d checks, %d ms)\n", gTests[i].name, gChecks - checksBefore, ms);
            } else if (gCurrentSkip) {
                skipped++;
                fprintf(stdout, "SKIP %s: %s\n", gTests[i].name, gCurrentSkip.UTF8String);
            } else {
                passed++;
                fprintf(stdout, "ok   %s (%d checks, %d ms)\n", gTests[i].name, gChecks - checksBefore, ms);
            }
            fflush(stdout);
        }
        int ms = (int)([[NSDate date] timeIntervalSinceDate:start] * 1000);
        fprintf(stdout, "\n%d tests: %d passed, %d failed, %d skipped; %d checks, %d failed (%d ms)\n",
                passed + failed + skipped, passed, failed, skipped, gChecks, gFailedChecks, ms);
        return failed > 0 ? 1 : 0;
    }
}
