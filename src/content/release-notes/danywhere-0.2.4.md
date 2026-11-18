## Window titles without Accessibility

- Without the Accessibility permission, DanyWhere used to ask only iTerm2, Terminal, Chrome, Safari and Outlook for the title of the window you dictate into. Since 0.2.3 it asks every app that can tell it, such as TextEdit, Notes, Mail, Microsoft Edge, Word, Excel and Pages. Which apps can, and how to ask them, DanyWhere reads from each app's own files, without asking the app.
- **Post Process → Context from Earlier Dictations → Window Titles** lists the running apps that can tell their title, each with **Allow…**, and the installed ones below them. macOS asks you once per app.
- New in 0.2.4: the first dictation after you allow an app gets its title too. An app can be slow to answer the first time; DanyWhere now asks it once when the recording starts, while you speak.
- An app under **Excluded Apps** is not asked for its window title at all.
