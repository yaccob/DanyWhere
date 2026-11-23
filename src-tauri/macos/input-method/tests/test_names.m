// Names and the socket path rule of PROTOCOL.md.

#import "DWTest.h"

#import "DWNames.h"

static NSString *Repeat(NSString *s, NSUInteger n) {
    return [@"" stringByPaddingToLength:n withString:s startingAtIndex:0];
}

DW_TEST(names_follow_the_protocol_table) {
    CHECK_EQ_OBJ(DWInfoKeyAppID, @"DanyWhereAppID");
    CHECK_EQ_OBJ(DWInputMethodIDForAppID(@"com.yaccob.danywhere"), @"com.yaccob.danywhere.inputmethod.DanyWhere");
    CHECK_EQ_OBJ(DWInputMethodIDForAppID(@"com.pais.handy.dev"), @"com.pais.handy.dev.inputmethod.DanyWhere");
    CHECK_EQ_OBJ(DWConnectionNameForInputMethodID(@"com.pais.handy.noax.inputmethod.DanyWhere"),
                 @"com.pais.handy.noax.inputmethod.DanyWhere_Connection");
    CHECK_EQ_OBJ(DWLogPathForAppID(@"com.yaccob.danywhere", @"/Users/someone"),
                 @"/Users/someone/Library/Logs/com.yaccob.danywhere/input-method.log");
}

DW_TEST(socket_path_is_in_application_support_when_short_enough) {
    CHECK_EQ_INT(DWMaxSocketPathBytes, 103);
    NSString *path = DWSocketPathForAppID(@"com.yaccob.danywhere", @"/Users/someone", @"/var/folders/xy/abc/T/");
    CHECK_EQ_OBJ(path, @"/Users/someone/Library/Application Support/com.yaccob.danywhere/input-method.sock");
}

DW_TEST(socket_path_moves_to_the_temp_dir_beyond_103_bytes) {
    NSString *temp = @"/var/folders/xy/abc/T/";
    NSString *appID = @"com.yaccob.danywhere";
    // "<home>/Library/Application Support/com.yaccob.danywhere/input-method.sock"
    NSUInteger fixed = [@"/Library/Application Support/com.yaccob.danywhere/input-method.sock" lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
    NSString *home103 = [@"/" stringByAppendingString:Repeat(@"h", 103 - fixed - 1)];
    NSString *at103 = DWSocketPathForAppID(appID, home103, temp);
    CHECK_EQ_INT([at103 lengthOfBytesUsingEncoding:NSUTF8StringEncoding], 103);
    CHECK([at103 hasPrefix:home103]);
    NSString *home104 = [home103 stringByAppendingString:@"h"];
    CHECK_EQ_OBJ(DWSocketPathForAppID(appID, home104, temp), @"/var/folders/xy/abc/T/com.yaccob.danywhere.input-method.sock");
}

DW_TEST(socket_path_counts_bytes_not_characters) {
    NSString *temp = @"/tmp/t/";
    NSUInteger fixed = [@"/Library/Application Support/a.b/input-method.sock" lengthOfBytesUsingEncoding:NSUTF8StringEncoding];
    // Each "ü" is two bytes in UTF-8: 50 characters but 100 bytes would not fit.
    NSUInteger room = 103 - fixed - 1;
    NSString *home = [@"/" stringByAppendingString:Repeat(@"ü", room / 2 + 1)];
    CHECK([home length] <= 1 + room);
    CHECK_EQ_OBJ(DWSocketPathForAppID(@"a.b", home, temp), @"/tmp/t/a.b.input-method.sock");
}

DW_TEST(user_temp_dir_ends_with_a_slash) {
    NSString *temp = DWUserTempDir();
    CHECK(temp.length > 1);
    CHECK([temp hasPrefix:@"/"]);
    CHECK([temp hasSuffix:@"/"]);
}
