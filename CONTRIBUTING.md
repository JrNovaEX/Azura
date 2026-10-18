# Contributing to Azura

Thank you for wanting to help. Azura is a small project, and every bug
report, idea, translation and fix makes a real difference.

This guide explains how to help and what to keep in mind. You do not need to
read it all before opening an issue. Just start, and we will figure out the
rest together.

## Ways to help

### Report a bug

Open an [issue](https://github.com/JrNovaEX/Azura/issues) and include:

- What you did, what you expected, and what happened instead.
- Your system, for example "Ubuntu 24.04, GNOME 46" or "Fedora 41, KDE Plasma 6".
- The Azura version (shown at the bottom of **Settings**).
- A screenshot, if the problem is something you can see.
- If an item failed to open, the reason Azura showed next to it.

If your sessions file might be involved, you can attach it, but read it first.
It contains the names of your apps and any commands you added. Remove anything
you would rather keep private.

### Suggest an idea

Open an issue and describe the problem you want to solve, not only the
solution you have in mind. It helps us find the simplest way to do it.

Please read [What Azura is, and is not](#what-azura-is-and-is-not) first, so
your idea fits the direction of the project.

### Translate Azura

Azura is ready for more languages. Each language is one text file.

1. Copy `crates/azura/locales/en.toml` to a new file named after your
   language code, for example `de.toml`.
2. Translate the values. Keep the parts in curly braces, like `{n}`, exactly
   as they are.
3. Add your language to the `LANGUAGES` list in `crates/azura/src/i18n.rs`:
   its code, its name written in that language, whether it is written right
   to left, and its own digits if it has them.
4. Run `cargo test`. It checks that no text is missing and that every
   `{placeholder}` survived.

For right-to-left languages, break long lines yourself with `\n` (about 40
characters per line). The text engine cannot wrap right-to-left paragraphs on
its own yet.

**Persian text** must follow [TYPOGRAPHY.md](TYPOGRAPHY.md): a casual, modern
tone, no tanween, no trailing periods or exclamation marks, real half-spaces
and Persian digits. The tests check these rules automatically.

### Write code

Small, focused changes are the easiest to review. For anything bigger than a
bug fix, please open an issue first so we can agree on the approach before you
spend time on it.

## What Azura is, and is not

Azura does one thing: it lets people save a set of real things on their
computer and open them in order, with timing, in one click.

It is **not** a startup manager, a process manager, a system optimizer, an
automation platform or a productivity tracker. We say no to features that
would push it in those directions, even good ones, to keep it small and
focused.

A few rules every change has to respect:

- **Real data only.** Azura never shows made-up apps, demo entries or
  bundled third-party icons. App names and icons always come from the user's
  own system. Test fixtures stay inside tests.
- **Private.** No accounts, no telemetry, no analytics. The only network
  request Azura makes is the update check (GitHub's latest-release API and
  the release files). Do not add any other.
- **Never overwrite a file Azura could not read.** A file whose contents are
  unusable is moved aside; a file that merely could not be read is left
  exactly where it is and Azura refuses to start.
- **Honest status.** The interface never claims something worked when it did
  not, and never claims an app is "ready" when Azura cannot know that.
- **Light by design.** Nothing runs in the background, and nothing scans the
  disk while Azura is idle.
- **Calm interface.** Follow the existing design: solid colors, amber only
  for things that are "on", no gradients or glows. See the design notes in
  [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#design).

## Setting up

Everything you need to build and run Azura is in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). In short, with Rust installed:

```sh
cargo run
```

Use the `AZURA_CONFIG` environment variable to try things with a separate
sessions file, so your own sessions stay untouched:

```sh
AZURA_CONFIG=./test-sessions.toml cargo run
```

## Before you open a pull request

Please make sure these pass:

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Then check this list:

- [ ] The change does one thing, and the description says why it is needed.
- [ ] New behavior has tests where it can be tested without a real desktop.
- [ ] Any new user-facing text exists in every language file.
- [ ] If the interface changed, the description includes before and after
      screenshots, in English and in Persian (right to left).
- [ ] Platform-specific code stays in `crates/azura-platform`, and the core
      crate still has no operating system or interface code.
- [ ] If you changed platform code, say whether you could test it on a
      real machine. That is fine either way; we just need to know.

We review pull requests as soon as we can. If something needs to change, we
will explain why and help you get it over the line.

## Commit messages

Write a short summary line in the imperative mood ("Add Linux terminal
setting", not "Added"), then a blank line and a few lines on why, if it is not
obvious.

## Being kind

Be respectful and patient with everyone, especially people who are new to
open source or writing in a second language. Criticize ideas, not people.
Harassment of any kind is not welcome here.

## License

By contributing, you agree that your contribution is released under the
project's [MIT License](LICENSE).
