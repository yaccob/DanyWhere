# DanyWhere

**Dictate anywhere.** DanyWhere is a personal distribution of [Handy](https://github.com/cjpais/Handy), the free, open-source, offline speech-to-text app by CJ Pais and contributors.

The hard problems — local speech recognition, voice activity detection, global shortcuts and pasting into any app on three platforms — are solved in Handy. DanyWhere admires that work and builds on it with a few additions its author needs in daily use, for example running on managed Macs where the Accessibility permission is blocked. Anything here that is useful to Handy is happily offered upstream.

- **Not affiliated with or endorsed by Handy or CJ Pais.** The Handy name, logo and icon belong to the Handy project; DanyWhere uses its own branding.
- **Report DanyWhere problems in this repository**, not in Handy's issue tracker.
- **Support the original:** if you find this useful, consider supporting Handy via [handy.computer](https://handy.computer).
- What is planned: [ROADMAP.md](ROADMAP.md).

> Status: early. Releases are for Macs, with Apple silicon or an Intel processor; on Windows and Linux, use [Handy](https://github.com/cjpais/Handy).

## What DanyWhere adds to Handy

- **Works without the Accessibility permission.** On managed Macs where this permission is blocked, choose "Continue without Accessibility" during setup: each transcript is put on the clipboard for you to paste with ⌘V, and a short notice says so. Shortcuts with the fn key are not available in this mode.
- **Paste the last transcript again.** Option+Shift+V puts the most recent transcript where the cursor is now, for example after a dictation went nowhere because no text field had focus.
- **A small model selection for laptops.** Six models that are fast enough for dictation on a MacBook: Parakeet Ultra (25 European languages), Whisper large-v3-turbo (100 languages), Canary 1B v2 (translates to English), Parakeet unified EN 0.6B (English with live preview), Whisper small and Canary 180M flash. Setup recommends one for the languages you speak and starts downloading it while you grant permissions. Models and the language detector some of them use download from DanyWhere's own mirror, [yaccob/DanyWhere-models](https://github.com/yaccob/DanyWhere-models), with Hugging Face as a fallback, and every file is checked against its SHA-256 hash.
- **A long history that stays small.** Recordings are stored as Opus, about 12 times smaller than WAV, and a storage limit (500 MB by default, about 54 hours of dictation, or unlimited) replaces the count of five recent entries. When old recordings are removed, their transcripts stay in the history.
- **Word lists per topic.** The custom words are a global word list that always applies. Named additional lists, for example one per project, can be switched on one at a time in the settings, from the tray menu or with Option+Shift+L; their words go to the model first.
- **Decide on post-processing at the end.** In toggle mode, the post-processing shortcut also stops a recording that was started with the plain shortcut, and the transcript is post-processed.
- **Installs next to Handy.** DanyWhere has its own app name, settings, process name (`danywhere`) and update channel, so both apps can be installed at the same time. Give them different shortcuts.

Everything else works as in Handy.

## Install

You need a Mac with macOS 11 (Big Sur) or later, with Apple silicon or an Intel processor. On Intel Macs, transcription runs on the processor, so older or dual-core Macs work best with the smaller models.

1. Download `DanyWhere_<version>_aarch64.dmg` (Apple silicon) or `DanyWhere_<version>_x64.dmg` (Intel) from this repository's [releases](https://github.com/yaccob/DanyWhere/releases). Both are signed with an Apple Developer ID and notarized by Apple.
2. Open the DMG and drag DanyWhere into Applications.

   Or install it with [Homebrew](https://brew.sh) instead of steps 1 and 2: `brew install --cask yaccob/tap/danywhere`

3. Start DanyWhere and follow the setup: allow microphone access, then grant Accessibility, or choose "Continue without Accessibility" if your organization blocks it.
4. Press Option+Space and speak. Hold the shortcut to record and release it to stop, or tap it to start and tap again to stop. Once post-processing is switched on (Settings → Advanced → Transcription) and set up, Option+Shift+Space does the same and post-processes the transcript.

DanyWhere checks this repository's releases for updates; updates are signed with DanyWhere's own key.

## Command line

DanyWhere accepts the same flags as Handy. On macOS, call the executable inside the app bundle:

```bash
/Applications/DanyWhere.app/Contents/MacOS/danywhere --toggle-transcription  # start or stop recording
/Applications/DanyWhere.app/Contents/MacOS/danywhere --toggle-post-process   # the same, with post-processing
/Applications/DanyWhere.app/Contents/MacOS/danywhere --cancel                # cancel the current operation
/Applications/DanyWhere.app/Contents/MacOS/danywhere --help                  # all flags
```

The remote-control flags are sent to the running DanyWhere instance.

## Networks that block model downloads

If DanyWhere cannot download a model, download the file yourself from the matching release of [yaccob/DanyWhere-models](https://github.com/yaccob/DanyWhere-models/releases), put it unrenamed into `~/Library/Application Support/com.yaccob.danywhere/models/`, and restart DanyWhere. The model then shows up as downloaded under Settings → Models.

## More documentation

Handy's [README](https://github.com/cjpais/Handy#readme) explains how the app works, debug mode, known limitations (Bluetooth microphones, the fn key on third-party keyboards) and troubleshooting. It applies to DanyWhere as well, except for installation, updates and where to report problems.

To build DanyWhere from source, follow [BUILD.md](BUILD.md).

## Contributing

DanyWhere is a one-person side project. Bug reports are welcome in this repository's [issues](https://github.com/yaccob/DanyWhere/issues); see [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Changes that help every Handy user belong in [Handy](https://github.com/cjpais/Handy).

## License

MIT License — see [LICENSE](LICENSE). Handy is copyright CJ Pais; DanyWhere's modifications are copyright Jakob Stemberger.

Handy is open-source software, but the Handy name, logo, icon and brand assets are not. DanyWhere uses its own name and branding and does not imply endorsement by or affiliation with Handy.

## Acknowledgments

- **[Handy](https://github.com/cjpais/Handy)** by CJ Pais and contributors — the app DanyWhere is built on
- **Whisper** by OpenAI, **Parakeet** and **Canary** by NVIDIA for the speech recognition models
- **ggml** for an amazing cross-platform tensor library
- **Silero** for great lightweight VAD
- **Tauri** team for the excellent Rust-based app framework
