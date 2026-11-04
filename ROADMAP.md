# DanyWhere roadmap

Planned work on DanyWhere, roughly in priority order within each section. Items that would benefit every Handy user are marked **upstream candidate** and are offered to [Handy](https://github.com/cjpais/Handy) as well.

## Features and improvements

### Keep the transcript when there is nowhere to paste it

**Problem:** when the focus is not on something that accepts text (the desktop, a non-editable window), recording and transcription work, but the text ends up nowhere. Inherited from Handy: the paste is sent without checking for an editable target, and the clipboard is restored afterwards.

**Done:** the "Paste Last Transcript" shortcut (Option+Shift+V on macOS) puts the most recent transcript where the cursor is now; without Accessibility it goes to the clipboard, and a notice tells the user where the text is (duration configurable under Advanced, 0 = off).

**Possible later addition:** detect automatically that a paste went nowhere and leave the text on the clipboard with the same notice. Handy's experimental receipt-based paste (`reliable_paste`, `src-tauri/src/paste_tx/macos.rs`) gets a callback when an app actually reads the pasteboard and already detects "no read within timeout". Clipboard managers that read eagerly need checking. **Upstream candidate.**

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

### Translation to English

**Problem:** Whisper large-v3-turbo, DanyWhere's default model, cannot translate (it was fine-tuned on transcription data only). The legacy `ggml-large-v3-turbo.bin` still reports translation support, so "Translate to English" is offered, the log says "(translated)", and the text stays in the spoken language (reproduced 2026-10-24). DanyWhere lets the catalog's capabilities win (the GGUF Turbo is marked `translate: false`), so the option no longer appears for Turbo (verified 2026-10-26).

**Options:** translate with Whisper medium or small (trained for translation; both in DanyWhere's selection), or with LLM post-processing (a "translate to English" prompt; usually better quality, needs a provider). Report the legacy-Turbo capability to Handy as a follow-up to [#1746](https://github.com/cjpais/Handy/issues/1746). **Upstream candidate** (the report).

### History: keep more, store less

**Problem:** inherited from Handy, the history keeps a fixed number of entries (default 5, `history_limit`) together with their recordings. Recordings are uncompressed WAV (16 kHz mono 16-bit, about 1.9 MB per minute), which is why the limit is small; the transcripts themselves take almost no space.

**Idea:**

- Limit recordings by **disk space** instead of a count: a maximum size (with a sensible minimum, and "unlimited" as an option); when it is exceeded, the oldest unsaved recordings are deleted first.
- Keep **transcripts** independently of their recordings (much longer, or unlimited), so the text history is not lost when audio is pruned.
- Store recordings **compressed**: Opus at about 24–32 kbit/s is roughly 8–10 times smaller and still fine for re-transcribing speech; FLAC is lossless but only about 2 times smaller. The 16 kHz rate is what the models use, so recording at a higher rate would not improve transcription.

**Upstream candidate.**

### Running without the Accessibility permission

Available: onboarding offers "Continue without Accessibility" for managed Macs (shortcuts via the Tauri backend, transcript left on the clipboard, input simulation not set up). A DanyWhere feature for now; it may be offered to Handy later.

Next steps:

- **Switch automatically.** Today the user chooses the mode once in onboarding and it stays, even if the permission is granted later. Instead: without the permission, use the clipboard mode with a short notice; once the permission is granted, return to the full mode on its own.
- **Test both states through one permission check.** Route every Accessibility check (backend and frontend) through a single function that tests can replace, and cover the whole flow — onboarding, startup, shortcut backend, input simulation, paste vs. clipboard — once with "granted" and once with "not granted". Component tests for the onboarding need a React test setup, which the project does not have yet. A real run without the permission stays necessary when code around permissions, pasting or shortcuts changes, because the operating system and third-party libraries can behave in ways a mock does not show.

## Branding

- Own tray icons for the recording and transcribing states (currently Handy's ear and brain).
- About page: "DanyWhere is based on Handy" with a link.

## Distribution

Done: macOS releases for Apple silicon, signed with a Developer ID and notarized, with signed updater artifacts and `latest.json` in this repository's releases.

- Trademark search for the name before going public.
- Make the repository and the first release public (the updater only finds published releases of a public repository).
- Windows and Linux builds; Windows signing with an own certificate (Handy's signing setup was removed).
