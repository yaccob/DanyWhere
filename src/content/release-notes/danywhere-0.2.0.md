## Profiles

A profile holds what decides how a dictation is processed: the speech model, languages, post-processing with its own prompts, filler words, word lists, context and what is appended. Shortcuts, microphone, API keys and the interface are the same in every profile. Your settings became the first profile.

- Choose the profile at the top of the sidebar or in the tray. The new **Profiles** page lists them, duplicates, exports and imports them; an exported API key is encrypted with a password you choose.
- **Profiles per application:** give a profile the applications it is for, and a dictation into one of them uses that profile, speech model included. **Choose Profile by Application** (⌃⇧A on macOS, Alt+Shift+A elsewhere) switches this off and on without changing the list. On Windows you type the program names; Linux can't tell the application in front yet.
- With two or more profiles, the overlay shows which one a dictation uses.

## Context by a switch

The Context Hotkey is gone, together with its macOS service and the command-line flag `--toggle-post-process-with-context`. Instead, **Send Context** is a switch per profile: while it is on, every dictation with post-processing sends your earlier dictations from the same window along. It starts **off**. Turn it on in the tray, in the sidebar, under **Post Process → Context from Earlier Dictations**, or with ⌃⇧X (Alt+Shift+X elsewhere).

## History: versions and search

- Every entry is a stack of versions: the transcription at the bottom, and each edit, yours or the post-processing's, on top of the version it was made from. Click into the text to edit it; **Save** changes the version you see, **Save as New Version** adds one. The track under the text lists the versions and goes back to an earlier one. Your earlier corrections became versions.
- **Search History** searches all entries and all their versions, with match case, whole words and regular expressions. A second field filters by app, window title or file name.
- The texts sit in boxes that scroll; drag the lower edge to make them taller or shorter.

## Post-processing

- DanyWhere now tells the model itself what it gets: a speech-recognition transcript to work on, not to answer. It also asks the model to correct misheard words where the context makes clear what was said. A prompt now only says what to do with the text. A prompt that still contains `${output}` runs as before, and DanyWhere names such prompts at startup.
- **Effort**, under the model, chooses how much the model thinks, per profile and provider.
- **Advanced → Transcription** sets how long post-processing and loading the model list may take.

## Output

- **Append** (Advanced → Output) replaces **Append Trailing Space** and **Auto Submit**: any text, where `\s` is a space and `\n` a new line, then a key press such as Enter. Your old settings were carried over. Key presses need Accessibility; without it only the text is appended.
- **Copy Last Transcript** (⌃⇧C on macOS, Alt+Shift+C elsewhere) puts the last dictation on the clipboard. During a dictation it ends it and copies the text once it's ready.
- On a Mac without Accessibility, **⌃⇧V** (Paste Last Transcript) now inserts the last dictation through the macOS service instead of putting it on the clipboard; the service no longer has a shortcut of its own (⌃⌥⌘V). Apps that were already open learn this after a restart. If Paste Last Transcript still has ⌥⇧V from before 0.1.6, change it to a combination with ⌃ or ⌘.

## Also new

- A sound once a dictation is completely done, post-processing included: **Sound When Done**, under Sound.
- A `theme.css` next to the settings changes the app's colours, without a restart: **Debug → Open Theme File** (⌘⇧D on macOS, Ctrl+Shift+D elsewhere, opens the Debug page).
- The **Experimental Features** switch is gone; its six settings are always on the Debug page.
- Based on Handy 0.10.1. On Linux, new installations paste with **Clipboard (Shift+Insert)**, which works in terminals too. If you use **Direct**, consider switching under **Advanced → Paste Method**.
