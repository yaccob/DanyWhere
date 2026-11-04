# Contributing to DanyWhere

DanyWhere is a personal distribution of [Handy](https://github.com/cjpais/Handy), maintained by one person in their spare time. Bug reports and pull requests are welcome, but there is no promise of a quick answer or of a merge.

## Where a change belongs

- **Useful to every Handy user** (a bug that also occurs in Handy, a general improvement): contribute it to [Handy](https://github.com/cjpais/Handy) and follow Handy's own [contributing guide](https://github.com/cjpais/Handy/blob/main/CONTRIBUTING.md). DanyWhere picks such changes up when it merges Handy.
- **Specific to DanyWhere** (its additions listed in the [README](README.md), its branding, releases and model mirror): contribute it here.

## Reporting a bug

1. Search the [existing issues](https://github.com/yaccob/DanyWhere/issues), including closed ones.
2. Check whether the problem also occurs in Handy. If it does, report it to Handy instead.
3. Open an issue with the bug report template. Include the DanyWhere version, your macOS version and Mac model, whether DanyWhere has the Accessibility permission, and the log (Settings → About shows the log directory). Remove private dictated text from logs before posting them.

Ideas and questions are welcome as ordinary issues.

## Pull requests

- Open an issue first for anything larger than a small fix, so we can agree on the approach.
- One fix or feature per pull request.
- Behavior changes come with tests. Unit tests live next to the code (`#[cfg(test)] mod tests` in Rust, `*.test.ts` in the frontend).
- Before submitting, run:

  ```bash
  bun run lint
  bun run format:check
  bun run check:translations
  (cd src-tauri && cargo test --lib)
  ```

- Use [conventional commit](https://www.conventionalcommits.org/) messages (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`) and say why a change is needed, not only what it does.
- Contributions made with AI assistance are fine; say so in the pull request.

Build instructions are in [BUILD.md](BUILD.md); [AGENTS.md](AGENTS.md) gives an overview of the code base.

## Translations

User-facing text comes from the locale files in `src/i18n/locales/`. Strings that DanyWhere shares with Handy are translated in Handy (see [CONTRIBUTING_TRANSLATIONS.md](CONTRIBUTING_TRANSLATIONS.md), Handy's guide); corrections to DanyWhere's own strings are welcome here.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
