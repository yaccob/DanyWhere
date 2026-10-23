# DanyWhere roadmap

Planned work on DanyWhere, roughly in priority order within each section. Items that would benefit every Handy user are marked **upstream candidate** and are offered to [Handy](https://github.com/cjpais/Handy) as well.

## Features and improvements

### Keep the transcript when there is nowhere to paste it

**Problem:** when the focus is not on something that accepts text (the desktop, a non-editable window), recording and transcription work, but the text ends up nowhere.

**Cause (inherited from Handy):** pasting saves the clipboard, writes the transcript, sends the paste shortcut without checking for an editable target, then restores the old clipboard (`paste_via_clipboard` in `src-tauri/src/clipboard.rs`). With no target, the paste goes nowhere and the restore also removes the transcript from the clipboard. It is only recoverable through the history or the tray item "Copy last transcription".

**Goal:** if the transcript could not be pasted, leave it on the clipboard and, once transcription (and post-processing, if used) has finished, show a notification: the text is on the clipboard because it could not be pasted — no editable field had focus.

**Approach:** Handy's experimental receipt-based paste (`reliable_paste`, `src-tauri/src/paste_tx/macos.rs`) puts a promise on the pasteboard and gets a callback when an app actually reads it; it already detects "no read within timeout". That signal is a more reliable trigger than guessing whether the focused element is editable. Clipboard managers that read the pasteboard eagerly need checking (Handy already marks the transcript as concealed for them).

**Upstream candidate.**

### Space between consecutive dictations

**Problem:** dictating into the same text field in several separate recordings (record, stop, record again) glues the pieces together: "…end of sentence.Next sentence…".

**Cause (inherited from Handy):** every recording is pasted on its own, with nothing before it and — unless the off-by-default setting "append trailing space" (`append_trailing_space`) is on — nothing after it. Within a single long recording there is no problem: the chunks of long-form transcription are joined with spaces (`join_transcripts` in `src-tauri/src/audio_toolkit/chunking.rs`, covered by tests; verified with a 60 s recording).

**Options:**

1. Turn on the existing trailing-space setting by default. Simple, but leaves a trailing space at the end of a field and before punctuation typed afterwards.
2. Look at the character before the cursor (macOS Accessibility: focused element's selected text range and the text before it) and prepend a space only when it is a letter, digit or sentence punctuation. Exact, but needs the Accessibility permission and is not supported by every app; fall back to option 3 or 1.
3. Remember the last paste (target app, time, last character) and prepend a space when the next dictation goes into the same app shortly afterwards and the previous one did not end in whitespace. Works without Accessibility; can be wrong if the cursor was moved in between.

**Proposal:** option 2 where available, option 3 as fallback (e.g. in "continue without Accessibility" mode); never add a space before text that starts with punctuation, and respect languages without word spacing (reuse `is_unspaced` from the chunk joiner). **Upstream candidate.**

### Phantom phrases at the end of a recording

Whisper sometimes appends text that was never said when the recording ends in silence (observed: "Vielen Dank." after a synthetic 60 s German recording). Known Whisper behaviour on trailing silence. To check: whether trimming trailing silence with the VAD before decoding (or dropping a final chunk without speech) avoids it.

### Continue without the Accessibility permission

Done in DanyWhere's base: onboarding offers "Continue without Accessibility" for managed Macs (shortcuts via the Tauri backend, transcript left on the clipboard). **Upstream candidate** (needs a Handy Discussion first because of the feature freeze).

## Branding

- A real DanyWhere wordmark to replace the placeholder text.
- Own tray icons for the recording and transcribing states (currently Handy's ear and brain).
- About page: "DanyWhere is based on Handy" with a link.

## Distribution

- Release workflow: build, sign the updater artifacts, publish `latest.json` in this repository's releases (the updater already points here).
- Trademark search for the name before going public.
- Apple Developer ID signing and notarization, so others can install without Gatekeeper warnings.
- Windows signing with an own certificate (Handy's signing setup was removed).
