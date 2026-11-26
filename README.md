# DanyWhere

**Dictate anywhere.** DanyWhere turns what you say into text, right where your cursor is, in whatever app you're working in. It is free, open-source software for macOS, Windows and Linux.

Speech recognition runs offline, on your own computer, with a model you choose, and you shape how much happens to each dictation before it arrives.

- **Manual:** [yaccob.github.io/DanyWhere](https://yaccob.github.io/DanyWhere/), in English, German, Spanish and French; DanyWhere shows the same manual under **Manual** in its sidebar.
- **Report DanyWhere problems in this repository's [issues](https://github.com/yaccob/DanyWhere/issues).**
- What is planned: [ROADMAP.md](ROADMAP.md).
- Where DanyWhere comes from: [Origin and thanks](#origin-and-thanks).

> Status: early. Releases are for macOS (Apple silicon and Intel), Windows and Linux (each x64 and ARM64). The Windows and Linux builds are new since 0.1.2 and have hardly been tried yet; reports are welcome.

## What sets it apart

- **End each dictation the way you need it.** The keys that stop a recording also decide what follows the text: a space, a new paragraph, or Enter to send a message. On a Mac, six such end shortcuts come ready.
- **Say what should happen to the text, right after it.** Press ⇥ during a recording and say "translate this into Polish" or "answer the question": a language model applies it to this one dictation, on top of your prompts.
- **Post-processing that works on your text instead of answering it,** by a language model of your choice, in the cloud or on your own computer, with your prompts, your word lists and, if you like, your earlier dictations as context.
- **Defaults, and profiles that change only what they need,** one per app if you like: dictate one way into your terminal and another into your chat.
- **A History that keeps every version** of a dictation, searchable, with its recordings stored small.
- **Works on managed Macs** where the Accessibility permission is blocked.
- **Offline speech recognition** with a small set of models that are fast enough on a laptop, and word lists per topic.

Everything DanyWhere can do is listed in the manual under [What DanyWhere can do](https://yaccob.github.io/DanyWhere/features/); its chapters explain how it works.

## Install

DanyWhere has its own app name, settings, process name (`danywhere`) and update channel, so it can be installed next to Handy; give the two apps different shortcuts.

The download links below are for DanyWhere 0.5.0; all releases are on this repository's [releases](https://github.com/yaccob/DanyWhere/releases) page. DanyWhere checks the releases for updates on every platform; updates are signed with DanyWhere's own key.

### macOS

You need a Mac with macOS 11 (Big Sur) or later, with Apple silicon or an Intel processor. On Intel Macs, transcription runs on the processor, so older or dual-core Macs work best with the smaller models.

1. Download [DanyWhere_0.5.0_aarch64.dmg](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_aarch64.dmg) (Apple silicon) or [DanyWhere_0.5.0_x64.dmg](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_x64.dmg) (Intel). Both are signed with an Apple Developer ID and notarized by Apple.
2. Open the DMG and drag DanyWhere into Applications.

   Or install it with [Homebrew](https://brew.sh) instead of steps 1 and 2: `brew install --cask yaccob/tap/danywhere`

3. Start DanyWhere and follow the setup: allow microphone access, then grant Accessibility, or choose "Continue without Accessibility" if your organization blocks it.
4. Start a dictation with Control+Space: hold it while you speak and release it to stop, or tap it to start and tap again to stop; either way a space follows the text. Once a post-processing provider is set up (Settings → Post Process), press Tab during a recording to have the transcript post-processed, and say what should happen to it, such as "translate this into Polish", before you end.

### Windows

You need Windows 10 or 11 on an x64 or ARM64 PC. On x64, transcription can use the graphics card (Vulkan); on ARM64, it runs on the processor.

1. Download the installer: [DanyWhere_0.5.0_x64-setup.exe](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_x64-setup.exe) for x64, [DanyWhere_0.5.0_arm64-setup.exe](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_arm64-setup.exe) for ARM64. For deployment with tools that need an MSI, use [DanyWhere_0.5.0_x64_en-US.msi](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_x64_en-US.msi) or [DanyWhere_0.5.0_arm64_en-US.msi](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_arm64_en-US.msi) instead.
2. Run the installer. DanyWhere is not signed with a Windows code-signing certificate yet, so Windows SmartScreen warns "Windows protected your PC": click **More info**, then **Run anyway**. On Windows 11 with Smart App Control switched on (Windows Security → App & browser control), Windows blocks unsigned apps without offering this choice.

   The installer asks for a normal or a portable installation. A portable installation keeps settings, models and history in a `Data` folder next to `danywhere.exe`.

3. Start DanyWhere from the Start menu and follow the setup: allow microphone access and pick a model. Windows needs no Accessibility permission.
4. Start a dictation with Ctrl+Space: hold it while you speak and release it to stop, or tap it to start and tap again to stop. Once a post-processing provider is set up (Settings → Post Process), press Tab during a recording to have the transcript post-processed, and say what should happen to it before you end.

### Linux

DanyWhere comes for x64 and ARM64 as a `.deb`, an `.rpm` and an AppImage:

| Package                             | x64                                                                                                                                     | ARM64                                                                                                                                       |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| deb (Debian, Ubuntu 22.04 or later) | [DanyWhere_0.5.0_amd64.deb](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_amd64.deb)           | [DanyWhere_0.5.0_arm64.deb](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_arm64.deb)               |
| rpm (Fedora, openSUSE)              | [DanyWhere-0.5.0-1.x86_64.rpm](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere-0.5.0-1.x86_64.rpm)     | [DanyWhere-0.5.0-1.aarch64.rpm](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere-0.5.0-1.aarch64.rpm)       |
| AppImage (any distribution)         | [DanyWhere_0.5.0_amd64.AppImage](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_amd64.AppImage) | [DanyWhere_0.5.0_aarch64.AppImage](https://github.com/yaccob/DanyWhere/releases/download/danywhere-v0.5.0/DanyWhere_0.5.0_aarch64.AppImage) |

1. Download the package for your distribution and processor, and install it (commands for x64):

   ```bash
   # deb: apt installs the dependencies too (dpkg -i does not)
   sudo apt install ./DanyWhere_0.5.0_amd64.deb
   # rpm (openSUSE: sudo zypper install …)
   sudo dnf install ./DanyWhere-0.5.0-1.x86_64.rpm
   # AppImage
   chmod +x DanyWhere_0.5.0_amd64.AppImage && ./DanyWhere_0.5.0_amd64.AppImage
   ```

2. Install the tool DanyWhere pastes with: `xdotool` on X11, `wtype` or `dotool` on Wayland. Without one, pasting may not work, especially on Wayland. Handy's [Linux notes](https://github.com/cjpais/Handy#linux-notes) have the details, for example for Ubuntu 26.04.
3. Start DanyWhere and follow the setup: allow microphone access and pick a model.
4. Start a dictation with Ctrl+Alt+Space, held while you speak or tapped to start and to stop. Linux has no instruction shortcut: to have a dictation post-processed, start and end it with `danywhere --toggle-post-process` (see [Command line](#command-line)), or set Post Processing to Always (Settings → Advanced → Transcription). On Wayland, global shortcuts usually have to be set up in the desktop environment: bind them to `danywhere --toggle-transcription` and `danywhere --toggle-post-process` (see [Command line](#command-line)).

On a Wayland desktop the AppImage runs as a Wayland app, like the deb and rpm, so its window follows your display scaling. If it misbehaves there, start it with `HANDY_GDK_BACKEND=x11 ./DanyWhere_0.5.0_amd64.AppImage` to run it through Xwayland instead.

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

The [manual](https://yaccob.github.io/DanyWhere/) explains how DanyWhere works. Handy's [README](https://github.com/cjpais/Handy#readme) explains how the app works, debug mode, known limitations (Bluetooth microphones, the fn key on third-party keyboards, Wayland) and troubleshooting. It applies to DanyWhere as well, except for installation, updates and where to report problems.

To build DanyWhere from source, follow [BUILD.md](BUILD.md).

## Contributing

DanyWhere is a one-person side project. Bug reports are welcome in this repository's [issues](https://github.com/yaccob/DanyWhere/issues); see [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Changes that help every Handy user belong in [Handy](https://github.com/cjpais/Handy).

## License

MIT License — see [LICENSE](LICENSE). Handy is copyright CJ Pais; DanyWhere's modifications are copyright Jakob Stemberger.

Handy is open-source software, but the Handy name, logo, icon and brand assets are not. DanyWhere uses its own name and branding and does not imply endorsement by or affiliation with Handy.

## Acknowledgments

Besides Handy (see [Origin and thanks](#origin-and-thanks)), DanyWhere relies on:

- **Whisper** by OpenAI, **Parakeet** and **Canary** by NVIDIA for the speech recognition models
- **ggml** for an amazing cross-platform tensor library
- **Silero** for great lightweight VAD
- **Tauri** team for the excellent Rust-based app framework

## Origin and thanks

DanyWhere began as a fork of [Handy](https://github.com/cjpais/Handy), CJ Pais's open-source speech-to-text app, and has since gone its own way, with profiles, end shortcuts, post-processing and a History that keeps every version. Handy's groundwork still carries it (local speech recognition, voice detection, global shortcuts, inserting text on three platforms), and we are grateful for it; DanyWhere is an independent project, and Handy's makers are neither involved in it nor responsible for it.

If DanyWhere is useful to you, please support Handy too, at [handy.computer](https://handy.computer).
