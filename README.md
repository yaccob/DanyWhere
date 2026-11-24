# DanyWhere

**Dictate anywhere.** DanyWhere is a personal distribution of [Handy](https://github.com/cjpais/Handy), the free, open-source, offline speech-to-text app by CJ Pais and contributors.

The hard problems — local speech recognition, voice activity detection, global shortcuts and pasting into any app on three platforms — are solved in Handy. DanyWhere admires that work and builds on it with additions its author needs in daily use, for example running on managed Macs where the Accessibility permission is blocked. Anything here that is useful to Handy is happily offered upstream.

- **Not affiliated with or endorsed by Handy or CJ Pais.** The Handy name, logo and icon belong to the Handy project; DanyWhere uses its own branding.
- **Report DanyWhere problems in this repository**, not in Handy's issue tracker.
- **Support the original:** if you find this useful, consider supporting Handy via [handy.computer](https://handy.computer).
- What is planned: [ROADMAP.md](ROADMAP.md).

> Status: early. Releases are for macOS (Apple silicon and Intel), Windows and Linux (each x64 and ARM64). The Windows and Linux builds are new since 0.1.2 and have hardly been tried yet; reports are welcome.

## What DanyWhere adds to Handy

### Defaults and profiles

- **Defaults, and profiles that change only what they need.** Every setting that concerns a dictation, from the speech model, languages and post-processing to the shortcuts, microphone, sounds and word list, has a default. A profile follows the defaults and keeps its own value only where you set one; each setting shows which applies and resets an own value with one click, and the defaults show which profiles differ. Settings of the app itself, such as the interface language, autostart and updates, are the same for every profile. Choose what dictations use at the top of the sidebar or in the tray, and export a profile to a file to import it elsewhere; its API key is encrypted with a password you choose.
- **A profile per application.** Give a profile the applications it is for, for example one for your terminal and another for your messenger. The application in front when the recording ends decides, and when the text later goes into another application, DanyWhere redoes only what that application's profile does differently. Control+Shift+A (Alt+Shift+A on Windows) switches this off and on. On Windows you type the program names; Linux can't tell the application in front yet.

### Post-processing

- **Post-processing that works on your text instead of answering it.** DanyWhere tells the model itself that it gets a speech-recognition transcript to work on, not a question to answer, that its language stays, and that misheard words are to be corrected where the context makes clear what was said. A prompt only says what to do with the text. **Effort**, next to the model, chooses how much the model thinks.
- **Several prompts in one go.** The prompts are one library for all profiles; each profile switches on the ones it uses and puts them in order, and they go to the model in one request, applied in turn.
- **Your word lists as known terms.** Post-processing gets the word lists in effect, so it can write a misheard name as listed and tell it from a word that fits the sentence, with every speech model.
- **Earlier dictations as context.** An end shortcut can send your earlier dictations from the same window along with its post-processing, so the model knows what the conversation is about.
- **Post-processing on this computer, and you see where it runs.** Next to the provider, the settings say whether post-processing stays on this computer or goes to the cloud. With a model served on this computer, such as Ollama or LM Studio through the Custom provider, DanyWhere has it loaded as soon as you start speaking, so its cold start doesn't add to the wait.
- **Decide at the end how a dictation ends.** A recording starts with a start shortcut and ends with an end shortcut of your choice, each with a key combination of its own: whether the text is post-processed (while post-processing is switched on), whether earlier dictations go along as context, and what is appended. On macOS twelve end shortcuts come out of the box (eight without Accessibility, since DanyWhere can't press keys without it), following one rule: Option post-processes, Shift picks the smaller variant, and Control with Return presses a key. So Control+Space ends with a space, Return with a new paragraph and Control+Return with a press of Enter, to send a message; Option+Control+Space, Option+Return and Option+Control+Return do the same with post-processing. Pressing a start shortcut again is one of these ends, so it ends without post-processing. On Windows and Linux the combinations of the two start shortcuts are the ends, the first without and the second with post-processing, whichever started the recording. Releasing a held start shortcut ends as set under **When Held**; out of the box the second one post-processes.

### History

- **Every edit kept as a version.** Each entry is a stack of versions with its result on top: post-processing, your own edit and transcribing again always add a version on top and delete nothing, and versions can be moved and deleted. Click into the text to edit it; the wand post-processes the version you see, with the profile's prompts or any single prompt, long after the dictation too. Each entry names the speech model that transcribed it.
- **Search the whole History,** every version included, with match case, whole words and regular expressions, and filter by app, window title or file name.
- **A long history that stays small.** Recordings are stored as Opus, about 12 times smaller than WAV, and a storage limit (500 MB by default, about 54 hours of dictation, or unlimited) replaces the count of five recent entries. When old recordings are removed, their transcripts stay in the History; a star keeps a recording permanently.

### Without the Accessibility permission (macOS)

- **Works where this permission is blocked.** On managed Macs, choose "Continue without Accessibility" during setup: a dictation waits in DanyWhere until you insert it with Control+Shift+V or a second press of the dictation shortcut, in apps that support macOS services, and your clipboard stays as it was. Control+Shift+C puts it on the clipboard instead, for apps where inserting doesn't work. Shortcuts with the fn key are not available in this mode.
- **Window titles from the apps themselves.** Without this permission, DanyWhere asks every app that can tell the title of its front window, once you allow it per app in the settings: browsers such as Chrome, Edge and Safari, iTerm2 and Terminal, Mail, Notes, TextEdit, Microsoft Office and many more. Which apps can, DanyWhere reads from their files, without asking them. Context from earlier dictations and the History's window filter need the title.

### Inserting and copying

- **Paste or copy the last transcript again.** Control+Shift+V (Alt+Shift+V on Windows and Linux) puts the most recent transcript where the cursor is now, for example after a dictation went nowhere because no text field had focus. Control+Shift+C (Alt+Shift+C) puts it on the clipboard.
- **Append text and a key press.** Each end shortcut can add text after the dictation, such as a space or a new line, and then a key press such as Enter to send a message right away.

### Speech models and word lists

- **A small model selection for laptops.** Six models that are fast enough for dictation on a MacBook: Parakeet Ultra (25 European languages), Whisper large-v3-turbo (100 languages), Canary 1B v2 (translates to English), Parakeet unified EN 0.6B (English with live preview), Whisper small and Canary 180M flash. Setup recommends one for the languages you speak and starts downloading it while you grant permissions. Models and the language detector some of them use download from DanyWhere's own mirror, [yaccob/DanyWhere-models](https://github.com/yaccob/DanyWhere-models), with Hugging Face as a fallback, and every file is checked against its SHA-256 hash.
- **Word lists per topic.** The word list of the defaults always applies, each profile can add words of its own, and named additional lists, for example one per project, can be switched on one at a time in the settings, from the tray or with Control+Shift+L (Alt+Shift+L on Windows and Linux). Speech models that take a vocabulary, the Whisper models, use them while they recognize speech; with every model, post-processing gets them as known terms. The settings and the model list say which models use them.

### Also new

- **A sound when everything is done,** post-processing included, so you know when to look back.
- **The overlay on the screen you work on**, the one with the window in front, with or without Accessibility (macOS and Windows).
- **Your own colours** in a `theme.css` next to the settings, applied without a restart (Debug → Open Theme File).
- **Key combinations as the system writes them** (⌃⌥D on macOS, Ctrl+Shift+V elsewhere), and in a combination with a main key the left and the right modifier keys count alike.
- **What's new after an update,** in English or German.

Everything else works as in Handy.

## Install

DanyWhere has its own app name, settings, process name (`danywhere`) and update channel, so it can be installed next to Handy; give the two apps different shortcuts.

The download links below are for DanyWhere 0.3.2; all releases are on this repository's [releases](https://github.com/yaccob/DanyWhere/releases) page. DanyWhere checks the releases for updates on every platform; updates are signed with DanyWhere's own key.

### macOS

You need a Mac with macOS 11 (Big Sur) or later, with Apple silicon or an Intel processor. On Intel Macs, transcription runs on the processor, so older or dual-core Macs work best with the smaller models.

1. Download [DanyWhere_0.3.2_aarch64.dmg](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_aarch64.dmg) (Apple silicon) or [DanyWhere_0.3.2_x64.dmg](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_x64.dmg) (Intel). Both are signed with an Apple Developer ID and notarized by Apple.
2. Open the DMG and drag DanyWhere into Applications.

   Or install it with [Homebrew](https://brew.sh) instead of steps 1 and 2: `brew install --cask yaccob/tap/danywhere`

3. Start DanyWhere and follow the setup: allow microphone access, then grant Accessibility, or choose "Continue without Accessibility" if your organization blocks it.
4. Press Control+Space and speak. Hold the shortcut to record and release it to stop, or tap it to start and tap again to stop; either way a space follows the text. Once post-processing is switched on (Settings → Advanced → Transcription) and set up, the transcript is post-processed when you hold Control+Shift+Space instead and release it, or when you end a recording with Option+Control+Space instead of Control+Space.

### Windows

You need Windows 10 or 11 on an x64 or ARM64 PC. On x64, transcription can use the graphics card (Vulkan); on ARM64, it runs on the processor.

1. Download the installer: [DanyWhere_0.3.2_x64-setup.exe](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_x64-setup.exe) for x64, [DanyWhere_0.3.2_arm64-setup.exe](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_arm64-setup.exe) for ARM64. For deployment with tools that need an MSI, use [DanyWhere_0.3.2_x64_en-US.msi](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_x64_en-US.msi) or [DanyWhere_0.3.2_arm64_en-US.msi](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_arm64_en-US.msi) instead.
2. Run the installer. DanyWhere is not signed with a Windows code-signing certificate yet, so Windows SmartScreen warns "Windows protected your PC": click **More info**, then **Run anyway**. On Windows 11 with Smart App Control switched on (Windows Security → App & browser control), Windows blocks unsigned apps without offering this choice.

   The installer asks for a normal or a portable installation. A portable installation keeps settings, models and history in a `Data` folder next to `danywhere.exe`.

3. Start DanyWhere from the Start menu and follow the setup: allow microphone access and pick a model. Windows needs no Accessibility permission.
4. Press Ctrl+Space and speak. Hold the shortcut to record and release it to stop, or tap it to start and tap again to stop. Once post-processing is switched on (Settings → Advanced → Transcription) and set up, Ctrl+Shift+Space does the same and post-processes the transcript.

### Linux

DanyWhere comes for x64 and ARM64 as a `.deb`, an `.rpm` and an AppImage:

| Package                             | x64                                                                                                                                     | ARM64                                                                                                                                       |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| deb (Debian, Ubuntu 22.04 or later) | [DanyWhere_0.3.2_amd64.deb](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_amd64.deb)           | [DanyWhere_0.3.2_arm64.deb](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_arm64.deb)               |
| rpm (Fedora, openSUSE)              | [DanyWhere-0.3.2-1.x86_64.rpm](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere-0.3.2-1.x86_64.rpm)     | [DanyWhere-0.3.2-1.aarch64.rpm](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere-0.3.2-1.aarch64.rpm)       |
| AppImage (any distribution)         | [DanyWhere_0.3.2_amd64.AppImage](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_amd64.AppImage) | [DanyWhere_0.3.2_aarch64.AppImage](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.3.2/DanyWhere_0.3.2_aarch64.AppImage) |

1. Download the package for your distribution and processor, and install it (commands for x64):

   ```bash
   # deb: apt installs the dependencies too (dpkg -i does not)
   sudo apt install ./DanyWhere_0.3.2_amd64.deb
   # rpm (openSUSE: sudo zypper install …)
   sudo dnf install ./DanyWhere-0.3.2-1.x86_64.rpm
   # AppImage
   chmod +x DanyWhere_0.3.2_amd64.AppImage && ./DanyWhere_0.3.2_amd64.AppImage
   ```

2. Install the tool DanyWhere pastes with: `xdotool` on X11, `wtype` or `dotool` on Wayland. Without one, pasting may not work, especially on Wayland. Handy's [Linux notes](https://github.com/cjpais/Handy#linux-notes) have the details, for example for Ubuntu 26.04.
3. Start DanyWhere and follow the setup: allow microphone access and pick a model.
4. Press Ctrl+Alt+Space and speak; Ctrl+Alt+Shift+Space does the same with post-processing. On Wayland, global shortcuts usually have to be set up in the desktop environment: bind them to `danywhere --toggle-transcription` and `danywhere --toggle-post-process` (see [Command line](#command-line)).

On a Wayland desktop the AppImage runs as a Wayland app, like the deb and rpm, so its window follows your display scaling. If it misbehaves there, start it with `HANDY_GDK_BACKEND=x11 ./DanyWhere_0.3.2_amd64.AppImage` to run it through Xwayland instead.

## Command line

DanyWhere accepts the same flags as Handy:

```bash
danywhere --toggle-transcription  # start or stop recording
danywhere --toggle-post-process   # the same, with post-processing
danywhere --cancel                # cancel the current operation
danywhere --help                  # all flags
```

The remote-control flags are sent to the running DanyWhere instance. Where the executable is:

| Platform          | Executable                                                                                                                                  |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| macOS             | `/Applications/DanyWhere.app/Contents/MacOS/danywhere`                                                                                      |
| Windows           | `%LOCALAPPDATA%\DanyWhere\danywhere.exe`, in PowerShell `& "$env:LOCALAPPDATA\DanyWhere\danywhere.exe"` (portable: in the folder you chose) |
| Linux, deb or rpm | `danywhere` (in `/usr/bin`)                                                                                                                 |
| Linux, AppImage   | the AppImage file itself                                                                                                                    |

## Networks that block model downloads

If DanyWhere cannot download a model, download the file yourself from the matching release of [yaccob/DanyWhere-models](https://github.com/yaccob/DanyWhere-models/releases), put it unrenamed into DanyWhere's models folder, and restart DanyWhere. The model then shows up as downloaded under Settings → Models.

| Platform | Models folder                                                                               |
| -------- | ------------------------------------------------------------------------------------------- |
| macOS    | `~/Library/Application Support/com.yaccob.danywhere/models/`                                |
| Windows  | `%APPDATA%\com.yaccob.danywhere\models\` (portable: `Data\models\` next to `danywhere.exe`) |
| Linux    | `~/.local/share/com.yaccob.danywhere/models/`                                               |

## More documentation

Handy's [README](https://github.com/cjpais/Handy#readme) explains how the app works, debug mode, known limitations (Bluetooth microphones, the fn key on third-party keyboards, Wayland) and troubleshooting. It applies to DanyWhere as well, except for installation, updates and where to report problems.

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
