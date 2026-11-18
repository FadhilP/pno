# Development

## Repository layout

| Path | Role |
|---|---|
| `Cargo.toml`, `Cargo.lock` | The `pno` crate: edition 2024, Rust 1.85 or later, MIT |
| `SPEC.md` | The format reference. `pno spec` prints it (it is built into the binary). |
| `README.md` | The user documentation: format overview, playback, install, commands, limits, tests |
| `docs/` | These documents: how the code works |
| `src/` | The Rust CLI ([architecture](architecture.md)) |
| `tests/` | Integration tests and fixtures |
| `skill/pno/` | The Claude Code skill to read and edit scores with `pno` (`SKILL.md`) |
| `skill/audio-to-pno/` | The skill to transcribe a recording ([audio-to-pno](audio-to-pno.md)) |
| `tools/` | Local programs, not in Git (for example `tools/sfizz/`); nothing in the code refers to them |

`.gitattributes` sets LF line endings for all text files. Keep LF when a script writes a file.

## Build and install

```sh
cargo build
cargo install --path .     # installs pno; the skills call the installed binary
```

`cargo install` builds the working tree, with uncommitted changes. `pno a2p` embeds the files of `skill/audio-to-pno`, so install again after a change there.

Dependencies: `clap` (command line) and `roxmltree` (MusicXML import). Lints: `unsafe_code = "forbid"`, `clippy::unwrap_used = "warn"`.

## External programs

| Program | Used for | Found by |
|---|---|---|
| MuseScore 4 | PDF, PNG, SVG export, audio without `--sound`, `.mxl` and `.mscz` import, `render` | `PNO_MUSESCORE`, else the default install place |
| sfizz (`sfizz_render`) | Audio and video with `--sound` | `PNO_SFIZZ`, else the `PATH` |
| ffmpeg | `.mp3`, `.flac`, `.ogg`, `.mp4` | `PNO_FFMPEG`, else the `PATH` |
| Python 3.10 | The first run of `pno a2p` | `PNO_PYTHON`, else `python` on the `PATH` |

## Skills

Copy `skill/pno` to `~/.claude/skills/pno` and `skill/audio-to-pno` to `~/.claude/skills/audio-to-pno`. Copy them again after a change.

## Tests

```sh
cargo test
```

| File | Tests |
|---|---|
| `tests/check.rs` | All fixtures pass `check`; errors give the correct place |
| `tests/fmt.rs` | All fixtures are in canonical form; `fmt` and `fmt --fix` |
| `tests/commands.rs` | `sound`, `analyze`, `set`, `bars`, `transpose` on temporary copies |
| `tests/export.rs` | MusicXML export of the fixtures and the SPEC examples, compared by structure |
| `tests/import.rs` | MusicXML import, export and round trip |
| `tests/play.rs` | MIDI playback (ornaments, tempo, phrase, humanize), audio through sfizz and ffmpeg |
| `tests/musescore.rs` | MuseScore reads the exports without import problems; velocity lines survive a saved MuseScore file |
| `tests/corpus.rs` | Round trip of a MusicXML corpus (ignored by default) |
| `tests/common/` | Helpers: run the built `pno`, temporary folders, fixtures, MusicXML comparison |

- Fixtures: `tests/fixtures/tristesse.pno` (with its MusicXML) and the 27 scores in `tests/fixtures/spec/`, one for each format area.
- A test that needs MuseScore or sfizz prints a message and passes when the program is absent. The MP3 test runs only when `ffmpeg` is present.
- The corpus test needs MusicXML files that are not in the repo:

  ```sh
  PNO_CORPUS=<folder> cargo test --test corpus -- --ignored --nocapture
  ```

  `PNO_CORPUS` is a path list. `PNO_CORPUS_VERBOSE` prints each file. The README section "Corpus coverage" has the last results.

The tests of `a2p.py` measure accuracy on audio data that cannot be in the repo. See [audio-to-pno](audio-to-pno.md#accuracy-and-tests).
