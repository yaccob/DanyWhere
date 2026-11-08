# Release Notes

Add user-facing release notes as Markdown files named by app version:

```text
src/content/release-notes/0.8.4.md
```

**DanyWhere:** name DanyWhere's notes `danywhere-<version>.md` (e.g.
`danywhere-0.2.0.md`). Only those are shown; the plain `<version>.md` files are
Handy's notes, kept as upstream ships them (see
`src/components/whats-new/releaseNoteFiles.ts`).

`danywhere-<version>.md` is written in English and shown for every language
without a translation. A translation sits next to it as
`danywhere-<version>.<language>.md`, named by the app's language code from
`src/i18n/languages.ts` (e.g. `danywhere-0.2.0.de.md`, `danywhere-0.2.0.zh-TW.md`).
The dialog picks the translation for the app's language, then for its base
language (`zh-TW` falls back to `zh`), then the English file.
`releaseNoteFiles.test.ts` checks that every translation has its English file
and names a language the app offers.

The update modal shows the highest bundled release note newer than the
persisted `whats_new_last_seen_version` and not newer than the running app
version.

Keep these files focused on headline user-facing changes. Release notes support
paragraphs, headings, lists, links, code, quotes, separators, hard line breaks,
and local images under `/release-notes/...`. Raw HTML is ignored before
rendering.

Place image assets in `public/release-notes/{version}/` and reference them from
Markdown with absolute paths:

```md
![Streaming transcription preview](/release-notes/0.9.0/streaming-transcription.webp)
```
