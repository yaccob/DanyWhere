#import "DWTestSupport.h"

#include <errno.h>
#include <poll.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

NSString *DWTestSocketPath(NSString *tag) {
    static int counter = 0;
    NSString *path = [NSString stringWithFormat:@"/tmp/dwim-%d-%@-%d.sock", getpid(), tag, ++counter];
    unlink(path.fileSystemRepresentation);
    return path;
}

static BOOL FillAddress(struct sockaddr_un *address, NSString *path) {
    memset(address, 0, sizeof *address);
    address->sun_family = AF_UNIX;
    const char *raw = path.fileSystemRepresentation;
    if (strlen(raw) >= sizeof address->sun_path) return NO;
    strlcpy(address->sun_path, raw, sizeof address->sun_path);
    address->sun_len = (unsigned char)SUN_LEN(address);
    return YES;
}

static int Listen(NSString *path) {
    struct sockaddr_un address;
    if (!FillAddress(&address, path)) return -1;
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    if (bind(fd, (struct sockaddr *)&address, sizeof address) != 0 || listen(fd, 4) != 0) {
        close(fd);
        return -1;
    }
    return fd;
}

static BOOL WaitReadable(int fd, NSTimeInterval timeout) {
    struct pollfd p = {.fd = fd, .events = POLLIN};
    int r;
    do {
        r = poll(&p, 1, (int)(timeout * 1000));
    } while (r < 0 && errno == EINTR);
    return r > 0;
}

@implementation DWSocketPair

+ (instancetype)connectedPair {
    NSString *path = DWTestSocketPath(@"pair");
    int listener = Listen(path);
    if (listener < 0) return nil;
    struct sockaddr_un address;
    FillAddress(&address, path);
    int client = socket(AF_UNIX, SOCK_STREAM, 0);
    if (connect(client, (struct sockaddr *)&address, sizeof address) != 0) {
        close(client);
        close(listener);
        unlink(path.fileSystemRepresentation);
        return nil;
    }
    int server = accept(listener, NULL, NULL);
    close(listener);
    unlink(path.fileSystemRepresentation);
    if (server < 0) {
        close(client);
        return nil;
    }
    DWSocketPair *pair = [[self alloc] init];
    pair->_serverFD = server;
    pair->_clientFD = client;
    return pair;
}

- (void)close {
    if (_serverFD >= 0) close(_serverFD);
    if (_clientFD >= 0) close(_clientFD);
    _serverFD = _clientFD = -1;
}

@end

@implementation DWTestServer {
    int _listener;
    int _client;
    NSMutableData *_pending;
}

- (instancetype)initWithPath:(NSString *)path {
    if ((self = [super init])) {
        _path = [path copy];
        _client = -1;
        _pending = [NSMutableData data];
        _listener = Listen(path);
        if (_listener < 0) return nil;
    }
    return self;
}

- (void)dealloc {
    [self stop];
}

- (BOOL)acceptWithin:(NSTimeInterval)timeout {
    if (_listener < 0 || !WaitReadable(_listener, timeout)) return NO;
    int fd = accept(_listener, NULL, NULL);
    if (fd < 0) return NO;
    int on = 1;
    setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, sizeof on);
    [self dropClient];
    _client = fd;
    return YES;
}

- (nullable NSString *)takeLine {
    const char *bytes = _pending.bytes;
    for (NSUInteger i = 0; i < _pending.length; i++) {
        if (bytes[i] == '\n') {
            NSData *line = [_pending subdataWithRange:NSMakeRange(0, i)];
            [_pending replaceBytesInRange:NSMakeRange(0, i + 1) withBytes:NULL length:0];
            return [[NSString alloc] initWithData:line encoding:NSUTF8StringEncoding];
        }
    }
    return nil;
}

- (NSString *)readLineWithin:(NSTimeInterval)timeout {
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:timeout];
    while (YES) {
        NSString *line = [self takeLine];
        if (line) return line;
        if (_client < 0) return nil;
        NSTimeInterval left = [deadline timeIntervalSinceNow];
        if (left <= 0 || !WaitReadable(_client, left)) return nil;
        char buffer[4096];
        ssize_t n = read(_client, buffer, sizeof buffer);
        if (n <= 0) return nil;
        [_pending appendBytes:buffer length:(NSUInteger)n];
    }
}

- (BOOL)clientClosedWithin:(NSTimeInterval)timeout {
    if (_client < 0) return NO;
    if (_pending.length > 0) return NO;
    if (!WaitReadable(_client, timeout)) return NO;
    char buffer[64];
    ssize_t n = read(_client, buffer, sizeof buffer);
    if (n > 0) [_pending appendBytes:buffer length:(NSUInteger)n];
    return n == 0;
}

- (BOOL)sendLine:(NSString *)line {
    return [self sendRaw:[line stringByAppendingString:@"\n"]];
}

- (BOOL)sendRaw:(NSString *)text {
    if (_client < 0) return NO;
    NSData *data = [text dataUsingEncoding:NSUTF8StringEncoding];
    return write(_client, data.bytes, data.length) == (ssize_t)data.length;
}

- (void)dropClient {
    if (_client >= 0) close(_client);
    _client = -1;
    [_pending setLength:0];
}

- (void)stop {
    [self dropClient];
    if (_listener >= 0) {
        close(_listener);
        unlink(_path.fileSystemRepresentation);
    }
    _listener = -1;
}

@end

@implementation DWFakeClient {
    NSMutableArray<NSString *> *_inserted;
}

- (instancetype)initWithBundleID:(NSString *)bundleID {
    if ((self = [super init])) {
        _clientBundleID = [bundleID copy];
        _inserted = [NSMutableArray array];
    }
    return self;
}

- (NSString *)insertText:(NSString *)text {
    @synchronized(self) {
        _insertCalls++;
        if (self.failure) return self.failure;
        [_inserted addObject:[text copy]];
        return nil;
    }
}

- (NSArray<NSString *> *)inserted {
    @synchronized(self) {
        return [_inserted copy];
    }
}

@end

@implementation DWFakeSystem {
    NSMutableArray<NSString *> *_selected;
    NSMutableArray *_overrides;
}

- (instancetype)init {
    if ((self = [super init])) {
        _selected = [NSMutableArray array];
        _overrides = [NSMutableArray array];
    }
    return self;
}

- (NSString *)currentInputSourceID {
    return self.current;
}

- (BOOL)selectInputSourceWithID:(NSString *)sourceID reason:(NSString **)reason {
    @synchronized(self) {
        [_selected addObject:sourceID];
    }
    self.current = sourceID;
    return YES;
}

- (BOOL)setKeyboardLayoutOverrideWithID:(NSString *)sourceID reason:(NSString **)reason {
    @synchronized(self) {
        [_overrides addObject:sourceID ?: (id)[NSNull null]];
    }
    return YES;
}

- (NSArray<NSString *> *)selected {
    @synchronized(self) {
        return [_selected copy];
    }
}

- (NSArray *)overrides {
    @synchronized(self) {
        return [_overrides copy];
    }
}

@end
