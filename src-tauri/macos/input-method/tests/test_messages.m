// Messages and framing, against the exact examples of PROTOCOL.md.

#import "DWTest.h"

#import "DWMessages.h"

static DWIncoming *Parse(NSString *line) {
    NSString *error = nil;
    DWIncoming *message = [DWIncoming messageFromLine:DWData(line) error:&error];
    return message;
}

static NSString *ParseError(NSString *line) {
    NSString *error = nil;
    DWIncoming *message = [DWIncoming messageFromLine:DWData(line) error:&error];
    return message ? nil : (error ?: @"(no reason)");
}

/// The JSON object a line parses to, for comparing without key order.
static id JSONObject(NSData *line) {
    if (!line) return nil;
    return [NSJSONSerialization JSONObjectWithData:line options:0 error:NULL];
}

// --- Input method → app: the lines match PROTOCOL.md byte for byte ---------

DW_TEST(outgoing_hello_matches_protocol) {
    CHECK_EQ_OBJ(DWString([DWOutgoing helloWithVersion:@"0.3.3" pid:123]),
                 @"{\"type\":\"hello\",\"version\":\"0.3.3\",\"pid\":123}\n");
}

DW_TEST(outgoing_active_and_inactive_match_protocol) {
    CHECK_EQ_OBJ(DWString([DWOutgoing activeWithClient:@"com.apple.TextEdit"]),
                 @"{\"type\":\"active\",\"client\":\"com.apple.TextEdit\"}\n");
    CHECK_EQ_OBJ(DWString([DWOutgoing inactiveWithClient:@"com.apple.TextEdit"]),
                 @"{\"type\":\"inactive\",\"client\":\"com.apple.TextEdit\"}\n");
}

DW_TEST(outgoing_inserted_matches_protocol) {
    CHECK_EQ_OBJ(DWString([DWOutgoing insertedOKWithID:7 client:@"com.apple.TextEdit"]),
                 @"{\"type\":\"inserted\",\"id\":7,\"ok\":true,\"client\":\"com.apple.TextEdit\"}\n");
    CHECK_EQ_OBJ(DWString([DWOutgoing insertedFailedWithID:7 reason:DWReasonNoClient]),
                 @"{\"type\":\"inserted\",\"id\":7,\"ok\":false,\"reason\":\"no_client\"}\n");
    CHECK_EQ_OBJ(DWString([DWOutgoing insertedFailedWithID:7 reason:DWReasonError]),
                 @"{\"type\":\"inserted\",\"id\":7,\"ok\":false,\"reason\":\"error\"}\n");
    // Without a known client the field is left out rather than invented.
    CHECK_EQ_OBJ(DWString([DWOutgoing insertedOKWithID:8 client:nil]),
                 @"{\"type\":\"inserted\",\"id\":8,\"ok\":true}\n");
}

DW_TEST(outgoing_caught_matches_protocol) {
    CHECK_EQ_OBJ(DWString([DWOutgoing caughtWithID:9 code:36 mods:0]),
                 @"{\"type\":\"caught\",\"id\":9,\"code\":36,\"mods\":0}\n");
    CHECK_EQ_OBJ(DWString([DWOutgoing caughtWithID:10 code:76 mods:131072]),
                 @"{\"type\":\"caught\",\"id\":10,\"code\":76,\"mods\":131072}\n");
}

DW_TEST(outgoing_ids_keep_64_bits) {
    CHECK_EQ_OBJ(DWString([DWOutgoing insertedFailedWithID:9007199254740993LL reason:DWReasonError]),
                 @"{\"type\":\"inserted\",\"id\":9007199254740993,\"ok\":false,\"reason\":\"error\"}\n");
}

DW_TEST(outgoing_lines_are_valid_json_with_one_newline) {
    NSArray<NSData *> *lines = @[
        [DWOutgoing helloWithVersion:@"0.3.3" pid:123],
        [DWOutgoing activeWithClient:@"com.apple.TextEdit"],
        [DWOutgoing inactiveWithClient:@"com.apple.TextEdit"],
        [DWOutgoing insertedOKWithID:7 client:@"com.apple.TextEdit"],
        [DWOutgoing insertedFailedWithID:7 reason:DWReasonNoClient],
        [DWOutgoing caughtWithID:9 code:36 mods:0],
    ];
    for (NSData *line in lines) {
        NSString *text = DWString(line);
        CHECK([text hasSuffix:@"\n"]);
        CHECK([text rangeOfString:@"\n"].location == text.length - 1);
        CHECK([JSONObject(line) isKindOfClass:[NSDictionary class]]);
    }
    NSDictionary *hello = JSONObject([DWOutgoing helloWithVersion:@"0.3.3" pid:123]);
    CHECK_EQ_OBJ(hello, (@{@"type": @"hello", @"version": @"0.3.3", @"pid": @123}));
}

DW_TEST(json_quote_escapes_only_what_json_needs) {
    CHECK_EQ_OBJ(DWJSONQuote(@"plain"), @"\"plain\"");
    CHECK_EQ_OBJ(DWJSONQuote(@"a\"b\\c"), @"\"a\\\"b\\\\c\"");
    CHECK_EQ_OBJ(DWJSONQuote(@"line\nnext\ttab\r"), @"\"line\\nnext\\ttab\\r\"");
    CHECK_EQ_OBJ(DWJSONQuote(@"\x01"), @"\"\\u0001\"");
    CHECK_EQ_OBJ(DWJSONQuote(@"a/b"), @"\"a/b\"");
    CHECK_EQ_OBJ(DWJSONQuote(@"Grüße ✓ 😀"), @"\"Grüße ✓ 😀\"");
    // Whatever it writes parses back to the same string.
    NSString *nasty = @"\"\\\n\r\t\b\f\x1f x/y Grüße 😀";
    NSString *json = [NSString stringWithFormat:@"[%@]", DWJSONQuote(nasty)];
    NSArray *parsed = [NSJSONSerialization JSONObjectWithData:DWData(json) options:0 error:NULL];
    CHECK_EQ_OBJ(parsed.firstObject, nasty);
}

// --- App → input method: the examples of PROTOCOL.md parse ------------------

DW_TEST(incoming_insert_parses) {
    DWIncoming *m = Parse(@"{\"type\":\"insert\",\"id\":7,\"text\":\"…\"}");
    CHECK(m != nil);
    CHECK_EQ_INT(m.kind, DWMessageInsert);
    CHECK_EQ_OBJ(m.type, @"insert");
    CHECK_EQ_INT(m.messageID, 7);
    CHECK_EQ_OBJ(m.text, @"…");
}

DW_TEST(incoming_layout_parses_with_source_and_null) {
    DWIncoming *m = Parse(@"{\"type\":\"layout\",\"source\":\"com.apple.keylayout.German\"}");
    CHECK_EQ_INT(m.kind, DWMessageLayout);
    CHECK_EQ_OBJ(m.source, @"com.apple.keylayout.German");
    DWIncoming *cleared = Parse(@"{\"type\":\"layout\",\"source\":null}");
    CHECK(cleared != nil);
    CHECK_EQ_INT(cleared.kind, DWMessageLayout);
    CHECK(cleared.source == nil);
}

DW_TEST(incoming_restore_parses_with_source_and_null) {
    DWIncoming *m = Parse(@"{\"type\":\"restore\",\"source\":\"com.apple.keylayout.German\"}");
    CHECK_EQ_INT(m.kind, DWMessageRestore);
    CHECK_EQ_OBJ(m.source, @"com.apple.keylayout.German");
    DWIncoming *cleared = Parse(@"{\"type\":\"restore\",\"source\":null}");
    CHECK(cleared != nil);
    CHECK_EQ_INT(cleared.kind, DWMessageRestore);
    CHECK(cleared.source == nil);
}

DW_TEST(incoming_catch_parses_keys_and_hold) {
    DWIncoming *m = Parse(@"{\"type\":\"catch\",\"id\":9,\"keys\":[{\"code\":36,\"mods\":0}],\"hold_ms\":1500}");
    CHECK(m != nil);
    CHECK_EQ_INT(m.kind, DWMessageCatch);
    CHECK_EQ_INT(m.messageID, 9);
    CHECK_EQ_INT(m.keys.count, 1);
    CHECK_EQ_INT(m.keys.firstObject.code, 36);
    CHECK_EQ_INT(m.keys.firstObject.mods, 0);
    CHECK_EQ_INT(m.holdMs, 1500);
}

DW_TEST(incoming_catch_without_keys_disarms) {
    DWIncoming *m = Parse(@"{\"type\":\"catch\",\"id\":9,\"keys\":[]}");
    CHECK(m != nil);
    CHECK_EQ_INT(m.kind, DWMessageCatch);
    CHECK_EQ_INT(m.messageID, 9);
    CHECK_EQ_INT(m.keys.count, 0);
}

DW_TEST(incoming_catch_hold_defaults_and_limits) {
    DWIncoming *noHold = Parse(@"{\"type\":\"catch\",\"id\":1,\"keys\":[{\"code\":36,\"mods\":0},{\"code\":76,\"mods\":0}]}");
    CHECK_EQ_INT(noHold.keys.count, 2);
    CHECK_EQ_INT(noHold.holdMs, DWDefaultHoldMs);
    DWIncoming *huge = Parse(@"{\"type\":\"catch\",\"id\":1,\"keys\":[{\"code\":36,\"mods\":0}],\"hold_ms\":600000}");
    CHECK_EQ_INT(huge.holdMs, DWMaxHoldMs);
    DWIncoming *negative = Parse(@"{\"type\":\"catch\",\"id\":1,\"keys\":[{\"code\":36,\"mods\":0}],\"hold_ms\":-5}");
    CHECK_EQ_INT(negative.holdMs, 0);
}

DW_TEST(incoming_release_and_quit_parse) {
    DWIncoming *release = Parse(@"{\"type\":\"release\",\"id\":9,\"text\":\"…\"}");
    CHECK_EQ_INT(release.kind, DWMessageRelease);
    CHECK_EQ_INT(release.messageID, 9);
    CHECK_EQ_OBJ(release.text, @"…");
    DWIncoming *quit = Parse(@"{\"type\":\"quit\"}");
    CHECK(quit != nil);
    CHECK_EQ_INT(quit.kind, DWMessageQuit);
}

DW_TEST(incoming_text_keeps_every_character) {
    NSString *text = @"Zeile 1\nZeile \"2\"\t\\ Grüße 😀 /";
    NSString *line = [NSString stringWithFormat:@"{\"type\":\"insert\",\"id\":3,\"text\":%@}", DWJSONQuote(text)];
    DWIncoming *m = Parse(line);
    CHECK_EQ_OBJ(m.text, text);
}

DW_TEST(incoming_unknown_types_and_fields_are_ignored) {
    DWIncoming *unknown = Parse(@"{\"type\":\"future_thing\",\"x\":1}");
    CHECK(unknown != nil);
    CHECK_EQ_INT(unknown.kind, DWMessageUnknown);
    CHECK_EQ_OBJ(unknown.type, @"future_thing");
    DWIncoming *extra = Parse(@"{\"type\":\"insert\",\"id\":7,\"text\":\"a\",\"new_field\":{\"deep\":[1,2]},\"z\":null}");
    CHECK_EQ_INT(extra.kind, DWMessageInsert);
    CHECK_EQ_OBJ(extra.text, @"a");
    DWIncoming *extraKey = Parse(@"{\"type\":\"catch\",\"id\":2,\"keys\":[{\"code\":36,\"mods\":0,\"side\":\"left\"}]}");
    CHECK_EQ_INT(extraKey.keys.count, 1);
}

DW_TEST(incoming_invalid_lines_are_rejected) {
    CHECK(ParseError(@"not json") != nil);
    CHECK(ParseError(@"[1,2,3]") != nil);
    CHECK(ParseError(@"\"insert\"") != nil);
    CHECK(ParseError(@"{\"id\":7,\"text\":\"a\"}") != nil);          // no type
    CHECK(ParseError(@"{\"type\":7}") != nil);                      // type not a string
    CHECK(ParseError(@"{\"type\":\"insert\",\"id\":7}") != nil);    // no text
    CHECK(ParseError(@"{\"type\":\"insert\",\"text\":\"a\"}") != nil); // no id
    CHECK(ParseError(@"{\"type\":\"insert\",\"id\":\"7\",\"text\":\"a\"}") != nil); // id not a number
    CHECK(ParseError(@"{\"type\":\"insert\",\"id\":true,\"text\":\"a\"}") != nil);  // id a bool
    CHECK(ParseError(@"{\"type\":\"insert\",\"id\":7.5,\"text\":\"a\"}") != nil);   // id not whole
    CHECK(ParseError(@"{\"type\":\"layout\"}") != nil);              // no source
    CHECK(ParseError(@"{\"type\":\"layout\",\"source\":5}") != nil); // source not a string
    CHECK(ParseError(@"{\"type\":\"catch\",\"id\":9}") != nil);      // no keys
    CHECK(ParseError(@"{\"type\":\"catch\",\"id\":9,\"keys\":[{\"mods\":0}]}") != nil); // key without code
    CHECK(ParseError(@"{\"type\":\"catch\",\"id\":9,\"keys\":[{\"code\":70000,\"mods\":0}]}") != nil); // no key code
    CHECK(ParseError(@"{\"type\":\"release\",\"id\":9}") != nil);    // no text
}

// --- Framing ------------------------------------------------------------------

DW_TEST(framing_joins_partial_lines) {
    DWLineBuffer *buffer = [[DWLineBuffer alloc] init];
    CHECK_EQ_INT([buffer linesByAppending:DWData(@"{\"type\":\"qu")].count, 0);
    CHECK_EQ_INT([buffer linesByAppending:DWData(@"it\"")].count, 0);
    NSArray<NSData *> *lines = [buffer linesByAppending:DWData(@"}\n")];
    CHECK_EQ_INT(lines.count, 1);
    CHECK_EQ_OBJ(DWString(lines.firstObject), @"{\"type\":\"quit\"}");
}

DW_TEST(framing_splits_several_lines_in_one_read) {
    DWLineBuffer *buffer = [[DWLineBuffer alloc] init];
    NSArray<NSData *> *lines = [buffer linesByAppending:DWData(@"{\"a\":1}\n{\"b\":2}\n\n{\"c\":3}\n{\"d\"")];
    CHECK_EQ_INT(lines.count, 3);
    CHECK_EQ_OBJ(DWString(lines[0]), @"{\"a\":1}");
    CHECK_EQ_OBJ(DWString(lines[1]), @"{\"b\":2}");
    CHECK_EQ_OBJ(DWString(lines[2]), @"{\"c\":3}");
    NSArray<NSData *> *rest = [buffer linesByAppending:DWData(@":4}\n")];
    CHECK_EQ_INT(rest.count, 1);
    CHECK_EQ_OBJ(DWString(rest.firstObject), @"{\"d\":4}");
}

DW_TEST(framing_keeps_multibyte_characters_split_across_reads) {
    DWLineBuffer *buffer = [[DWLineBuffer alloc] init];
    NSData *all = DWData(@"{\"type\":\"insert\",\"id\":1,\"text\":\"ü😀\"}\n");
    // Cut inside the emoji's four bytes.
    NSUInteger cut = all.length - 5;
    CHECK_EQ_INT([buffer linesByAppending:[all subdataWithRange:NSMakeRange(0, cut)]].count, 0);
    NSArray<NSData *> *lines = [buffer linesByAppending:[all subdataWithRange:NSMakeRange(cut, all.length - cut)]];
    CHECK_EQ_INT(lines.count, 1);
    DWIncoming *m = [DWIncoming messageFromLine:lines.firstObject error:NULL];
    CHECK_EQ_OBJ(m.text, @"ü😀");
}

DW_TEST(framing_invalid_line_is_skipped_and_the_next_one_still_parses) {
    DWLineBuffer *buffer = [[DWLineBuffer alloc] init];
    NSArray<NSData *> *lines = [buffer linesByAppending:DWData(@"garbage{\n{\"type\":\"quit\"}\n")];
    CHECK_EQ_INT(lines.count, 2);
    int valid = 0, invalid = 0;
    for (NSData *line in lines) {
        if ([DWIncoming messageFromLine:line error:NULL]) valid++; else invalid++;
    }
    CHECK_EQ_INT(valid, 1);
    CHECK_EQ_INT(invalid, 1);
}

DW_TEST(framing_drops_an_overlong_line_whole) {
    DWLineBuffer *buffer = [[DWLineBuffer alloc] initWithMaxLineLength:16];
    CHECK_EQ_INT([buffer linesByAppending:DWData(@"0123456789")].count, 0);
    CHECK_EQ_INT([buffer linesByAppending:DWData(@"0123456789")].count, 0); // now 20 bytes, over the limit
    NSArray<NSData *> *lines = [buffer linesByAppending:DWData(@"tail\n{\"ok\":1}\n")];
    CHECK_EQ_INT(lines.count, 1);
    CHECK_EQ_OBJ(DWString(lines.firstObject), @"{\"ok\":1}");
    CHECK_EQ_INT(buffer.droppedLines, 1);
    NSArray<NSData *> *exact = [buffer linesByAppending:DWData(@"0123456789abcdef\n")];
    CHECK_EQ_INT(exact.count, 1); // exactly at the limit is fine
}

DW_TEST(framing_reset_forgets_a_partial_line) {
    DWLineBuffer *buffer = [[DWLineBuffer alloc] init];
    [buffer linesByAppending:DWData(@"{\"half")];
    [buffer reset];
    NSArray<NSData *> *lines = [buffer linesByAppending:DWData(@"{\"type\":\"quit\"}\n")];
    CHECK_EQ_INT(lines.count, 1);
    CHECK_EQ_OBJ(DWString(lines.firstObject), @"{\"type\":\"quit\"}");
}
