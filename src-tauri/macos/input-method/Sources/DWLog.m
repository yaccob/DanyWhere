#import "DWLog.h"

#include <unistd.h>

@implementation DWLog {
    dispatch_queue_t _queue;
    NSString *_path;
    unsigned long long _maxBytes;
    NSDateFormatter *_formatter;
}

+ (instancetype)shared {
    static DWLog *log;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        log = [[DWLog alloc] init];
    });
    return log;
}

- (instancetype)init {
    if ((self = [super init])) {
        _queue = dispatch_queue_create("danywhere.inputmethod.log", DISPATCH_QUEUE_SERIAL);
        _formatter = [[NSDateFormatter alloc] init];
        _formatter.locale = [NSLocale localeWithLocaleIdentifier:@"en_US_POSIX"];
        _formatter.dateFormat = @"yyyy-MM-dd HH:mm:ss.SSS";
    }
    return self;
}

- (void)configureWithPath:(NSString *)path maxBytes:(unsigned long long)maxBytes {
    NSString *copy = [path copy];
    dispatch_sync(_queue, ^{
        self->_path = copy;
        self->_maxBytes = maxBytes;
        if (copy) {
            [[NSFileManager defaultManager] createDirectoryAtPath:[copy stringByDeletingLastPathComponent]
                                      withIntermediateDirectories:YES
                                                       attributes:nil
                                                            error:NULL];
        }
    });
}

- (void)event:(NSString *)message {
    NSDate *now = [NSDate date];
    dispatch_async(_queue, ^{
        NSString *path = self->_path;
        if (!path) return;
        NSString *line = [NSString stringWithFormat:@"%@ [%d] %@\n", [self->_formatter stringFromDate:now], getpid(), message];
        NSData *data = [line dataUsingEncoding:NSUTF8StringEncoding allowLossyConversion:YES];
        NSFileManager *files = [NSFileManager defaultManager];
        unsigned long long size = [[files attributesOfItemAtPath:path error:NULL] fileSize];
        if (size > 0 && size + data.length > self->_maxBytes) {
            NSString *old = [path stringByAppendingString:@".1"];
            [files removeItemAtPath:old error:NULL];
            [files moveItemAtPath:path toPath:old error:NULL];
        }
        NSFileHandle *handle = [NSFileHandle fileHandleForWritingAtPath:path];
        if (!handle) {
            [files createFileAtPath:path contents:nil attributes:nil];
            handle = [NSFileHandle fileHandleForWritingAtPath:path];
        }
        if (!handle) return;
        @try {
            [handle seekToEndOfFile];
            [handle writeData:data];
        } @catch (NSException *exception) {
            // Disk full or the file vanished: the log is best effort.
        }
        [handle closeFile];
    });
}

- (void)flush {
    dispatch_sync(_queue, ^{});
}

@end

void DWLogEvent(NSString *format, ...) {
    va_list arguments;
    va_start(arguments, format);
    NSString *message = [[NSString alloc] initWithFormat:format arguments:arguments];
    va_end(arguments);
    [[DWLog shared] event:message];
}
