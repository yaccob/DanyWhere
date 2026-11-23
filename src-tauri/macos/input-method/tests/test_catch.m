// Stage 2: which key press is caught, and how a held key ends.

#import "DWTest.h"

#import "DWCatch.h"

// NSEvent's device-independent modifier flags, spelled out so this file
// needs no AppKit.
enum {
    CapsLock = 1 << 16,
    Shift = 1 << 17,
    Control = 1 << 18,
    Option = 1 << 19,
    Command = 1 << 20,
    NumericPad = 1 << 21,
    Function = 1 << 23,
    LeftShiftDevice = 0x2, // device-dependent bit (NX_DEVICELSHIFTKEYMASK)
};

static NSArray<DWKeyChord *> *EnterKeys(void) {
    return @[[DWKeyChord chordWithCode:36 mods:0], [DWKeyChord chordWithCode:76 mods:0]];
}

DW_TEST(catch_chord_modifiers_keep_only_shift_control_option_command) {
    CHECK_EQ_INT(DWChordModifiers(0), 0);
    CHECK_EQ_INT(DWChordModifiers(Shift | LeftShiftDevice), Shift);
    CHECK_EQ_INT(DWChordModifiers(CapsLock | NumericPad | Function), 0);
    CHECK_EQ_INT(DWChordModifiers(Control | Option | Command | Shift), Control | Option | Command | Shift);
}

DW_TEST(catch_is_disarmed_at_first) {
    DWCatchState *state = [[DWCatchState alloc] init];
    CHECK(!state.armed);
    CHECK(![state takeKeyCode:36 modifierFlags:0 isRepeat:NO hasClient:YES]);
}

DW_TEST(catch_arms_and_takes_one_matching_press) {
    DWCatchState *state = [[DWCatchState alloc] init];
    [state armWithID:9 keys:EnterKeys() holdMs:1500];
    CHECK(state.armed);
    CHECK_EQ_INT(state.armedID, 9);
    CHECK_EQ_INT(state.holdMs, 1500);
    CHECK(![state takeKeyCode:0 modifierFlags:0 isRepeat:NO hasClient:YES]);  // "a": not a key to catch
    CHECK(state.armed);
    CHECK(![state takeKeyCode:36 modifierFlags:Shift isRepeat:NO hasClient:YES]); // ⇧↩: other modifiers
    CHECK(state.armed);
    CHECK([state takeKeyCode:36 modifierFlags:0 isRepeat:NO hasClient:YES]);
    CHECK(!state.armed); // one press per catch
    CHECK(![state takeKeyCode:36 modifierFlags:0 isRepeat:NO hasClient:YES]);
}

DW_TEST(catch_matches_the_keypad_enter_and_ignores_state_flags) {
    DWCatchState *state = [[DWCatchState alloc] init];
    [state armWithID:1 keys:EnterKeys() holdMs:100];
    // The keypad Enter carries the numeric pad (and on some keyboards the
    // function) flag; with Caps Lock on it is still the same chord.
    CHECK([state takeKeyCode:76 modifierFlags:NumericPad | Function | CapsLock isRepeat:NO hasClient:YES]);
}

DW_TEST(catch_matches_modifier_chords) {
    DWCatchState *state = [[DWCatchState alloc] init];
    [state armWithID:2 keys:@[[DWKeyChord chordWithCode:36 mods:Command]] holdMs:100];
    CHECK(![state takeKeyCode:36 modifierFlags:0 isRepeat:NO hasClient:YES]);
    CHECK([state takeKeyCode:36 modifierFlags:Command | 0x8 isRepeat:NO hasClient:YES]);
}

DW_TEST(catch_ignores_auto_repeat) {
    DWCatchState *state = [[DWCatchState alloc] init];
    [state armWithID:3 keys:EnterKeys() holdMs:100];
    CHECK(![state takeKeyCode:36 modifierFlags:0 isRepeat:YES hasClient:YES]);
    CHECK(state.armed);
    CHECK([state takeKeyCode:36 modifierFlags:0 isRepeat:NO hasClient:YES]);
}

DW_TEST(catch_needs_a_client) {
    DWCatchState *state = [[DWCatchState alloc] init];
    [state armWithID:4 keys:EnterKeys() holdMs:100];
    CHECK(![state takeKeyCode:36 modifierFlags:0 isRepeat:NO hasClient:NO]);
    CHECK(state.armed);
}

DW_TEST(catch_empty_keys_disarm) {
    DWCatchState *state = [[DWCatchState alloc] init];
    [state armWithID:5 keys:EnterKeys() holdMs:100];
    [state armWithID:5 keys:@[] holdMs:0];
    CHECK(!state.armed);
    CHECK(![state takeKeyCode:36 modifierFlags:0 isRepeat:NO hasClient:YES]);
    [state armWithID:6 keys:EnterKeys() holdMs:100];
    [state disarm];
    CHECK(!state.armed);
}

DW_TEST(catch_a_new_catch_replaces_the_old) {
    DWCatchState *state = [[DWCatchState alloc] init];
    [state armWithID:7 keys:EnterKeys() holdMs:100];
    [state armWithID:8 keys:@[[DWKeyChord chordWithCode:49 mods:0]] holdMs:200];
    CHECK_EQ_INT(state.armedID, 8);
    CHECK_EQ_INT(state.holdMs, 200);
    CHECK(![state takeKeyCode:36 modifierFlags:0 isRepeat:NO hasClient:YES]);
    CHECK([state takeKeyCode:49 modifierFlags:0 isRepeat:NO hasClient:YES]);
}

// --- The held key ---------------------------------------------------------------

DW_TEST(hold_release_in_time_lets_the_key_through_with_its_text) {
    DWHoldGate *gate = [[DWHoldGate alloc] init];
    [gate beginForID:9];
    CHECK(gate.holding);
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 50 * NSEC_PER_MSEC), dispatch_get_global_queue(0, 0), ^{
        [gate releaseID:9 text:@"Hallo Welt"];
    });
    NSString *text = nil;
    NSDate *start = [NSDate date];
    DWHoldOutcome outcome = [gate waitMilliseconds:2000 text:&text];
    double ms = [[NSDate date] timeIntervalSinceDate:start] * 1000;
    CHECK_EQ_INT(outcome, DWHoldReleased);
    CHECK_EQ_OBJ(text, @"Hallo Welt");
    CHECK(ms < 1500);
    CHECK(!gate.holding);
}

DW_TEST(hold_without_release_times_out_and_swallows) {
    DWHoldGate *gate = [[DWHoldGate alloc] init];
    [gate beginForID:9];
    NSString *text = @"untouched";
    NSDate *start = [NSDate date];
    DWHoldOutcome outcome = [gate waitMilliseconds:120 text:&text];
    double ms = [[NSDate date] timeIntervalSinceDate:start] * 1000;
    CHECK_EQ_INT(outcome, DWHoldTimedOut);
    CHECK(text == nil);
    CHECK(ms >= 100);
    CHECK(ms < 1000);
    CHECK(!gate.holding);
}

DW_TEST(hold_release_with_another_id_is_dropped) {
    DWHoldGate *gate = [[DWHoldGate alloc] init];
    [gate beginForID:9];
    CHECK(![gate releaseID:8 text:@"wrong"]);
    DWHoldOutcome outcome = [gate waitMilliseconds:60 text:NULL];
    CHECK_EQ_INT(outcome, DWHoldTimedOut);
}

DW_TEST(hold_late_release_is_dropped) {
    DWHoldGate *gate = [[DWHoldGate alloc] init];
    [gate beginForID:9];
    CHECK_EQ_INT([gate waitMilliseconds:30 text:NULL], DWHoldTimedOut);
    CHECK(![gate releaseID:9 text:@"late"]);
    // A later hold does not get the late text.
    [gate beginForID:10];
    CHECK_EQ_INT([gate waitMilliseconds:30 text:NULL], DWHoldTimedOut);
}

DW_TEST(hold_release_before_the_wait_starts_is_kept) {
    // `caught` goes out before the main thread starts waiting, so the
    // release may win that race.
    DWHoldGate *gate = [[DWHoldGate alloc] init];
    [gate beginForID:11];
    CHECK([gate releaseID:11 text:@"early"]);
    NSString *text = nil;
    CHECK_EQ_INT([gate waitMilliseconds:1000 text:&text], DWHoldReleased);
    CHECK_EQ_OBJ(text, @"early");
}

DW_TEST(hold_release_without_a_hold_is_dropped) {
    DWHoldGate *gate = [[DWHoldGate alloc] init];
    CHECK(!gate.holding);
    CHECK(![gate releaseID:1 text:@"nobody waits"]);
}
