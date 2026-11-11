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

**Deferred:** not before the first public release; the preferred behaviour is still open.

### Named word lists

**Problem:** there is one list of custom words for everything. Whisper models get the list as a short context ("Glossary: …") of at most 223 tokens (roughly 70–100 German words); what does not fit is dropped from the end. Parakeet and Canary have no such input; for them DanyWhere replaces similar-sounding words after transcription, and only for entries without umlauts or ß. One long list mixing topics therefore biases recognition less precisely than a short list for the topic at hand.

**Done:** the custom words are now the **Global Word List**, which always applies. Named additional lists (e.g. "Work", "Programming") are managed in the Advanced settings the same way as the post-processing prompts; one of them or none is switched on in addition. The model gets the additional list's words first, then the global ones. The shortcut ⌥⇧L steps through the lists (only global → first list → … → only global) and shows the result briefly in the overlay; the tray menu shows the active list and switches it.

### Phantom phrases at the end of a recording

Whisper sometimes appends text that was never said when the recording ends in silence (observed: "Vielen Dank." after a synthetic 60 s German recording). Known Whisper behaviour on trailing silence. To check: whether trimming trailing silence with the VAD before decoding (or dropping a final chunk without speech) avoids it.

### Translation to English

**Problem:** Whisper large-v3-turbo, DanyWhere's default model, cannot translate (it was fine-tuned on transcription data only). The legacy `ggml-large-v3-turbo.bin` still reports translation support, so "Translate to English" is offered, the log says "(translated)", and the text stays in the spoken language (reproduced 2026-10-24). DanyWhere lets the catalog's capabilities win (the GGUF Turbo is marked `translate: false`), so the option no longer appears for Turbo (verified 2026-10-26).

**Options:** translate with Whisper medium or small (trained for translation; both in DanyWhere's selection), or with LLM post-processing (a "translate to English" prompt; usually better quality, needs a provider). Report the legacy-Turbo capability to Handy as a follow-up to [#1746](https://github.com/cjpais/Handy/issues/1746). **Upstream candidate** (the report).

### History: keep more, store less

**Problem:** inherited from Handy, the history keeps a fixed number of entries (default 5, `history_limit`) together with their recordings. Recordings are uncompressed WAV (16 kHz mono 16-bit, about 1.9 MB per minute), which is why the limit is small; the transcripts themselves take almost no space.

**Done:**

- Recordings are stored as **Ogg Opus** at 24 kbit/s, about 12 times smaller than WAV (9 MB per hour of dictation); in a test with real dictations, re-transcribing them gave the same text as the WAV originals. Existing WAV recordings are converted once at startup.
- A **storage limit** (100 MB, 500 MB by default, 1 GB, 5 GB or unlimited) replaces the count and age options; over it, the oldest recordings are deleted. Saved entries and transcribed files count but are never deleted automatically, and the newest recording is always kept.
- **Transcripts stay** when their recording is deleted; the entry says the recording was removed.
- The history player and "Re-transcribe" decode recordings in the backend, so playback does not depend on the webview's codecs.

**Upstream candidate.**

### History: post-process an entry on demand

**Idea (user, 2026-11-07):** post-process a history entry at the press of a button, or re-transcribe it and post-process the result.

**Found:** "Re-transcribe" already post-processed, but only entries dictated with post-processing (`post_process_requested`). The history showed and copied only the raw transcript: the post-processed text was stored (`post_processed_text`) but never displayed, so its result was invisible. Handy's history does the same.

**Done** (released in 0.1.3):

- An entry with a post-processed text shows it, and "Show original" switches to the raw transcript; the copy button copies what is shown.
- A **Post-process** button (wand) post-processes the existing transcript with the prompt selected now. It needs no recording, so it also works once the storage limit has deleted it. While post-processing is off or not set up, it is greyed out and its tooltip says why.
- **Shift+click on Re-transcribe** transcribes again and post-processes the result (Shift, as in the dictation shortcuts). A plain click works as before.
- An entry post-processed this way counts as post-processed from then on, so a later "Re-transcribe" post-processes it too. While it runs, the entry and the footer say "Post-processing..."; when it fails, the entry stays as it was and a notice says so.

### History: where a dictation was made, and earlier dictations as context

**Idea (user, 2026-11-08, undecided):** store with each History entry the caption of the window or application the recording was started in, the word lists and the post-processing in effect, and show them in the History. On that basis, post-processing could optionally get the last N dictations from the same window as context.

**Found:** the History stores the text of the prompt that ran, but not its name, and does not show it; the word lists in effect are not recorded; nothing determines the active window yet. Post-processing sees only the current transcript.

**The problem behind it (user, 2026-11-08):** with "Build" in the word list, a dictated "Bild" comes out as "Build". For models without vocabulary input, such as Parakeet, the speech model hears "Bild" correctly and the fuzzy word replacement then changes every occurrence, whatever the sentence means. The similarity threshold cannot separate the two.

**Tested (2026-11-08, invented dictations, one model):** labelled context repairs this only if the prompt explicitly allows correcting sound-alike words over its rule to change nothing, and works best when told which word the list replaced. Context does nothing for dictations that are not post-processed, so the replacement itself needs a fix too.

**To settle first:**

- Whether the changes context makes to real dictations are improvements.
- Privacy: dictations made without post-processing have never left the computer, and window titles can be sensitive.
- Matching: an exact title is unreliable, because titles change with state or are too general.
- Availability: a window title needs Accessibility on macOS and has no general API on Wayland; the application name is easier.

### Post-processing: the model list loads by itself

**Found (user, 2026-11-08):** the model dropdown offered only the selected model (`claude-sonnet-5`), although the API key can use 14 Anthropic models. Handy keeps the fetched list in memory only and fetches it only on a provider switch or with the refresh button, so after every start the dropdown held just the selected model.

**Done** (ships with 0.1.4): the Post Process page loads the list by itself when it shows a provider that is set up (a key, or a base URL for the custom provider) and whose list this app session has not loaded or tried yet: after the start, after a provider switch, once a key is entered. A failed attempt is repeated only after the key or base URL changes, or with the refresh button.

### Running without the Accessibility permission

Available: onboarding offers "Continue without Accessibility" for managed Macs (shortcuts via the Tauri backend, transcript left on the clipboard, input simulation not set up). A DanyWhere feature for now; it may be offered to Handy later.

**A sound when the dictation is on the clipboard** (user, 2026-11-11; ships with 0.1.4): whenever the user has to paste the text themselves, after a dictation or after "Paste Last Transcript", a third sound plays once the text is on the clipboard: a single marimba stroke on E5, a fourth above the start sound (the Pop theme plays two pops; a custom theme may add `custom_clipboard.wav` and otherwise borrows the marimba sound). The existing Audio Feedback switch turns it on and off. All feedback sounds now play one after another, so it never overlaps the stop sound.

Next steps:

- **Insert without Accessibility and without the general clipboard (user, 2026-11-11).** On a Mac where Accessibility cannot be granted, bring the result into the active window with a shortcut of its own instead of ⌘V, without using the general clipboard. Today every dictation replaces what the user had copied, which on the user's work Mac happens all the time.
  - **Decided (2026-11-11): a macOS service** with its own keyboard shortcut. The app the user types in asks DanyWhere for the text and inserts it itself, over a pasteboard used only for this exchange, so no Accessibility is needed. In this mode the dictation no longer goes to the general clipboard (user, 2026-11-11).
  - **Tested with a stand-alone test service** (2026-11-11, shortcut ⌃⌥⌘V): the text arrives in iTerm2 (also in vim's insert mode), in Chrome and Safari (text fields and editable areas as in Confluence and Jira) and in VS Code. Apple's Terminal asks for the text but passes nothing on. Teams and Outlook were not tested. As with typing, vim modes need insert mode: VSCodeVim, and the shell's line editor with `bindkey -v`.
  - **The clipboard only on demand (user, 2026-11-11):** for apps where the service does not work, a shortcut copies the last dictation from DanyWhere's own store to the clipboard, and the user pastes it with ⌘V. Without Accessibility, "Paste Last Transcript" already does exactly that, so no new shortcut is needed; its description should say so once the dictation itself no longer goes to the clipboard. (Considered before: a shortcut that switches the clipboard copy on and off, globally or per app.)
  - Set aside: an **input method** of DanyWhere's own (more work, has to be enabled as an input source), and keeping ⌘V but **restoring the clipboard after the first paste** as Handy's receipt-based paste does (`src-tauri/src/paste_tx/macos.rs`; clipboard managers that read eagerly end it early, and contents other than text or images may not come back).
- **Switch automatically.** Today the user chooses the mode once in onboarding and it stays, even if the permission is granted later. Instead: without the permission, use the clipboard mode with a short notice; once the permission is granted, return to the full mode on its own.
- **Test both states through one permission check.** Route every Accessibility check (backend and frontend) through a single function that tests can replace, and cover the whole flow — onboarding, startup, shortcut backend, input simulation, paste vs. clipboard — once with "granted" and once with "not granted". Component tests for the onboarding need a React test setup, which the project does not have yet. A real run without the permission stays necessary when code around permissions, pasting or shortcuts changes, because the operating system and third-party libraries can behave in ways a mock does not show.

### "What's new" after an update

**Found (2026-11-08):** DanyWhere has Handy's "What's new" dialog, shown once after an update, but shipped no notes for it, so users learnt about new features only from the release page. Handy's notes are English only.

**Done** (the first note ships with 0.1.4):

- Notes in the app's language: `src/content/release-notes/danywhere-<version>.md` in English, the fallback for every language, plus translations `danywhere-<version>.<language>.md`. The dialog takes the translation for the app's language, then for its base language, then the English file.
- The first note, in English and German, explains the History's post-processing from 0.1.3, which no dialog announced.

**With each release:** write the version's note and its German translation. The dialog shows only the newest note, so it covers what is new since the last one.

## Branding

- About page: "DanyWhere is based on Handy" with a link.

## Distribution

Done: macOS releases for Apple silicon, signed with a Developer ID and notarized, with signed updater artifacts and `latest.json` in this repository's releases. The repository and DanyWhere 0.1.0 are public (2026-11-05), after a trademark pre-check of the name; Homebrew installs it from the tap `yaccob/tap`.

- **macOS on Intel:** releases for `x86_64-apple-darwin` next to Apple silicon (DMG, updater bundle, `darwin-x86_64` in `latest.json`, the Homebrew cask per architecture). First target: a 2 GHz dual-core Intel Core i5 on macOS 12.7.6. The binary already allows macOS 11; to check on the device: which models are fast enough for dictation there, and whether the system WebKit (Safari 16.4 or newer) renders the interface.
- **Windows and Linux:** releases since 0.1.2, built by GitHub Actions (`.github/workflows/release-danywhere.yml`) for Windows x64 and ARM64 (NSIS installer and MSI) and Linux x64 and ARM64 (deb, rpm, AppImage), in the same release and `latest.json` as the macOS builds. A first test of 0.1.2 on a real Windows PC went well (2026-11-08). Open: Windows code signing, without which SmartScreen warns and Smart App Control blocks the installer (options: SignPath Foundation, free for open source; a certificate of one's own; the Microsoft Store).
