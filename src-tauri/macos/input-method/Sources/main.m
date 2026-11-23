// Entry point of DanyWhere's input method. macOS starts this process when
// its input source is selected (or registered); it runs until logout, until
// DanyWhere sends `quit`, or until macOS stops it. See ../PROTOCOL.md.

#import <Cocoa/Cocoa.h>
#import <InputMethodKit/InputMethodKit.h>
#include <sys/sysctl.h>

#import "DWCore.h"
#import "DWInputController.h"
#import "DWLink.h"
#import "DWLog.h"
#import "DWNames.h"
#import "DWPeerCheck.h"
#import "DWSystem.h"

static const unsigned long long kLogMaxBytes = 256 * 1024;

// Kept for the life of the process: IMK serves through it.
static IMKServer *gServer;

static NSString *ProcessArchitecture(void) {
#if defined(__arm64__)
    return @"arm64";
#else
    int translated = 0;
    size_t size = sizeof translated;
    if (sysctlbyname("sysctl.proc_translated", &translated, &size, NULL, 0) == 0 && translated) {
        return @"x86_64 (Rosetta)";
    }
    return @"x86_64";
#endif
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSBundle *bundle = NSBundle.mainBundle;
        NSDictionary *info = bundle.infoDictionary;
        NSString *appID = info[DWInfoKeyAppID];
        NSString *inputSourceID = bundle.bundleIdentifier;
        NSString *connectionName = info[@"InputMethodConnectionName"];
        NSString *version = info[@"CFBundleShortVersionString"] ?: @"?";
        if (appID.length == 0 || inputSourceID.length == 0 || connectionName.length == 0) {
            NSLog(@"DanyWhere input method: Info.plist lacks %@, CFBundleIdentifier or InputMethodConnectionName",
                  DWInfoKeyAppID);
            return 1;
        }

        NSString *home = NSHomeDirectory();
        [[DWLog shared] configureWithPath:DWLogPathForAppID(appID, home) maxBytes:kLogMaxBytes];
        DWLogEvent(@"start: %@ %@, pid %d, %@, macOS %@", inputSourceID, version, getpid(), ProcessArchitecture(),
                   NSProcessInfo.processInfo.operatingSystemVersionString);

        [NSApplication sharedApplication];
        // IMKServer finds the controller class by its name in Info.plist.
        [DanyWhereInputController class];
        gServer = [[IMKServer alloc] initWithName:connectionName bundleIdentifier:inputSourceID];
        if (!gServer) {
            DWLogEvent(@"IMKServer could not start (connection %@)", connectionName);
            [[DWLog shared] flush];
            return 1;
        }

        NSString *socketPath = DWSocketPathForAppID(appID, home, DWUserTempDir());
        DWLink *link = [[DWLink alloc] initWithSocketPath:socketPath
                                                    hello:[DWOutgoing helloWithVersion:version pid:getpid()]
                                                peerCheck:DWMakeSignaturePeerCheck(appID)];
        DWCore *core = [[DWCore alloc] initWithInputSourceID:inputSourceID link:link system:[[DWSystemTIS alloc] init]];
        DWCore.shared = core;
        DWLogEvent(@"IMKServer up (connection %@), link %@", connectionName, socketPath);
        [core start];

        [NSApp run];
    }
    return 0;
}
