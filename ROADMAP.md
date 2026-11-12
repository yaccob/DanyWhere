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

**Next (user, 2026-11-12):** a trial version, so the user can try context in daily use. Decided with the user:

- **Only from the same window.** Context comes only from earlier dictations made in the same window, matched by its title with status symbols removed (e.g. the spinner Claude Code puts in front of a terminal title). Without a window title there is no context at all, and there is no fallback to "the last few dictations": the user often works on several things at once, and a wrong context is worse than none. On a Mac without Accessibility (the company Mac) this means no context for now.
- **Also dictations that were never post-processed** may serve as context. In return they come only from the same window (the same conversation or document whose text is being sent anyway), only up to an age limit (default 60 minutes, configurable), never from apps on an exclusion list (password managers, banking), and the History marks every entry that left the computer as context. Local models small enough for the user's Mac are not good enough yet (the user's experience), so this can't rely on one.
- **Window titles in Claude Code** carry the session's topic, which is why the user wants them. Observed on 2026-11-12: iTerm2's title was the session name given at a handoff and stayed so for the whole following session, so a title can outlast a change of topic; the age limit bounds that.

**Correct transcripts and post-processing in the History** (user, 2026-11-12; in progress on `feat/history-corrections`): the original transcription and post-processed text stay as they were made; a correction of either is stored beside it. A post-processing can be corrected, and it can be deleted, so that the (corrected) transcription is the entry's text again. After a transcription is corrected nothing is post-processed again by itself: the History says that the post-processed text was made from the uncorrected transcription, and the wand button post-processes the corrected one. What an entry is (shown, copied, pasted again, used as context): the corrected post-processing, else the post-processing, else the corrected transcription, else the transcription.

### Post-processing: the model list loads by itself

**Found (user, 2026-11-08):** the model dropdown offered only the selected model (`claude-sonnet-5`), although the API key can use 14 Anthropic models. Handy keeps the fetched list in memory only and fetches it only on a provider switch or with the refresh button, so after every start the dropdown held just the selected model.

**Done** (ships with 0.1.4): the Post Process page loads the list by itself when it shows a provider that is set up (a key, or a base URL for the custom provider) and whose list this app session has not loaded or tried yet: after the start, after a provider switch, once a key is entered. A failed attempt is repeated only after the key or base URL changes, or with the refresh button.

### Running without the Accessibility permission

Available: onboarding offers "Continue without Accessibility" for managed Macs (shortcuts via the Tauri backend, input simulation not set up). A DanyWhere feature for now; it may be offered to Handy later.

**Insert without Accessibility, the clipboard left alone** (user, 2026-11-11; ships with 0.1.4): on a Mac without the permission a dictation no longer goes to the general clipboard; before, every dictation replaced what the user had copied, which on the user's work Mac happened all the time. It waits in DanyWhere until the user inserts it with the macOS service "Insert Last Dictation" (German "Letztes Diktat einfügen"): the app the user types in asks DanyWhere for the text and inserts it itself, over a pasteboard used only for this exchange, so no permission is needed.

- **Shortcut** ⌃⌥⌘V, which DanyWhere sets once in the system's services settings and never again, so a change in System Settings stays; it can be changed under General. Apps that are already open learn a new shortcut only after a restart: neither System Settings nor any public call tells them (tried 2026-11-11/12).
- **Where it works** (tried 2026-11-11): iTerm2 (also vim's insert mode), Chrome and Safari (text fields and editable areas as in Confluence and Jira), VS Code, TextEdit. Apple's Terminal asks for the text but passes nothing on. Teams and Outlook were not tested. As with typing, vim modes need insert mode: VSCodeVim, and the shell's line editor with `bindkey -v`.
- **The clipboard only on demand** (user, 2026-11-11): where the service doesn't work, "Paste Last Transcript" puts the last dictation on the clipboard for the user to paste with ⌘V. (Considered before: a shortcut that switches the clipboard copy on and off, globally or per app.)
- **While a dictation is still recording or transcribing**, the service inserts nothing rather than the dictation before it.
- Set aside: an **input method** of DanyWhere's own (more work, has to be enabled as an input source), and keeping ⌘V but **restoring the clipboard after the first paste** as Handy's receipt-based paste does (`src-tauri/src/paste_tx/macos.rs`; clipboard managers that read eagerly end it early, and contents other than text or images may not come back).

**A sound when the dictation waits** (user, 2026-11-11; ships with 0.1.4): whenever the user has to insert or paste the text themselves, after a dictation or after "Paste Last Transcript", a third sound plays once the text is ready: a single marimba stroke on E5, a fourth above the start sound (the Pop theme plays two pops; a custom theme may add `custom_clipboard.wav` and otherwise borrows the marimba sound). The existing Audio Feedback switch turns it on and off. All feedback sounds now play one after another, so it never overlaps the stop sound.

**Tested on the company Mac** (user, 2026-11-12): the service works there as on the private Mac, but the waiting sound sometimes doesn't play when the dictation is ready to insert. Not investigated yet. The log line "Dictation waits for the user; playing the waiting sound." shows whether DanyWhere tried to play it. Apart from the Audio Feedback switch, the code skips the sound only when a newer dictation has already started or the text could not be handed over.

**Insert without a second shortcut** (user, 2026-11-12): ⌃⌥⌘V is complicated to press (it can be changed under General). The real wish is to insert as with Accessibility: the second press of the dictation shortcut ends the dictation, and the text appears once it is ready. Ideas (Claude's); the user chose the first one (2026-11-12):

- **The insert shortcut also ends a dictation.** **Done** (ships with the next release): pressed while recording, the service stops the recording; pressed while transcribing, it waits. It answers once the text is ready, instead of inserting nothing; then neither the waiting sound nor the "Ready" notice comes. The app being typed in waits meanwhile, up to the service's `NSTimeout` of 30 s; DanyWhere gives up after 20 s, and the dictation then waits for the shortcut as before. One shortcut starts a dictation, the other ends and inserts it, with no waiting for the sound. It works in the same apps as before. DanyWhere answers the service on its main thread and keeps that thread's run loop going while it waits.
- **One shortcut for both.** **Built** (user's choice, 2026-11-12; branch `feat/dictation-key-inserts`): while a recording runs that only the next press ends (toggle, or hold-or-toggle after a tap), DanyWhere releases both dictation shortcuts, so their second press reaches the app being typed in. There each is the key equivalent of a service of its own ("End Dictation and Insert", and "End Dictation with Post-processing and Insert" for the post-process shortcut), which ends the recording and answers once the text is ready, as the first idea does. DanyWhere keeps those two services' shortcuts equal to the dictation shortcuts and takes the shortcuts back as soon as the recording ends, for whatever reason. A setting under General ("Dictation shortcut also inserts", on by default without Accessibility) switches it off. The ⌘-or-⌃ rule of "Insert Last Dictation" was Claude's own caution; the services take what a dictation shortcut can be (Space, ⌥ or ⇧ alone, function keys). **Tried by hand on 2026-11-12** in NoAX: ⌃⇧Space works as a service key equivalent; the plain shortcut ended and inserted two dictations, the post-process shortcut three. Limits:
  - **Push-to-talk** stays as before: a key release reaches no app, so the text waits for "Insert Last Dictation".
  - **Where no service can take the press** (Apple's Terminal, the Finder, no text field), the second press does nothing and the recording keeps running. There, "Paste Last Transcript" now ends the recording and puts the text on the clipboard once it's ready; Escape cancels. Not yet tried by hand.
  - **Apps that are already open** learn the services' shortcuts only after a restart, as with "Insert Last Dictation".
  - **When nothing records** (DanyWhere wasn't running and macOS started it to answer the service, or its global shortcut failed to register), the two services never insert an earlier dictation (user, 2026-11-12). A setting next to the first one chooses: start a recording for that shortcut, locked, so its next press ends and inserts it (the default; the user found it "noch besser"), or do nothing. The call is answered with nothing at once; the recording starts as soon as DanyWhere is ready, or not at all after 15 s.
- **An input method of DanyWhere's own** (set aside on 2026-11-11 as more work): an input method can insert text at any moment, with no key press at all. Input methods also work in Apple's Terminal; whether an insertion without a key press works there is untested. The user has to add it once as an input source and keep it selected. Whether a managed Mac allows a third-party input method is unknown; a minimal test input method on the company Mac would tell.

Next steps:

- **Switch automatically.** Today the user chooses the mode once in onboarding and it stays, even if the permission is granted later. Instead: without the permission, use the mode without Accessibility with a short notice; once the permission is granted, return to the full mode on its own.
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
