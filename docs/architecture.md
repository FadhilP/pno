# Architecture

`pno` is one Rust binary. Each command reads a `.pno` text file, turns it into a model, and then checks, edits, analyzes or exports it.

```
.pno text ──parse──▶ Score (model) ──Analysis──▶ check, sound, analyze
    ▲                    │                  └──▶ perform ──▶ MIDI, audio, video
    └──────write─────────┤                  └──▶ MusicXML ──▶ MuseScore: PDF, PNG, SVG, audio, .mscz
                         └──edit (bars, set, transpose, phrase)
MusicXML ──import──▶ Score
```

## Modules

| Module | Role |
|---|---|
| `main.rs` | The command line (`clap`), the dispatch of each command, and the shared helpers |
| `text/` | The text format: `parse.rs` (text to `Score`), `token.rs` (one cell token), `attr.rs` (`@` attributes), `write.rs` (`Score` to canonical text) |
| `model.rs` | The data types of a score |
| `frac.rs` | `Frac`, an exact fraction for all musical time |
| `diag.rs` | `Diag`: an error or warning with its line and place |
| `analysis.rs` | `Analysis`: the facts that most commands need, computed once |
| `accidental.rs` | The accidentals that a note shows by default (key, earlier notes in the bar) |
| `check.rs` | The `check` rules |
| `fmt.rs` | The canonical form |
| `edit.rs` | The structural edits |
| `analyze.rs` | The text reports of `sound` and `analyze` |
| `perform.rs`, `midi.rs`, `audio.rs`, `video.rs` | Playback: see [playback and export](playback-and-export.md) |
| `musicxml.rs`, `musicxml/` | MusicXML export and import |
| `render.rs` | Runs MuseScore |
| `a2p.rs` | `pno a2p`: see [audio-to-pno](audio-to-pno.md) |

## The text layer

- `text::parse::parse(text)` returns `Parsed { score, diags }`. A file must start with `@ format=1`. Parse errors are diagnostics, not panics, so `check` can report all of them.
- `text::write::write(score)` writes the canonical text. `fmt::format(score, fix)` drops default values, writes the text, and parses it again to make sure that the output is valid. With `Fix::Yes` it also splits invalid `-` runs with ties and renumbers the bars.
- `SPEC.md` is the format reference. `main.rs` embeds it, and `pno spec [SECTION]` prints the section list or one `## ` section.

## The model

| Type | Content |
|---|---|
| `Score` | `header` attributes and `bars` |
| `Bar` | `label` (`1`, `8b`, `0` for a pickup), source line, attributes, `lines`, and `layout` |
| `BarLayout` | The meter, the length of each cell, the `x` time at the start (`lead`) and end (`trail`), and the full length |
| `Line`, `El` | One lane of a bar, and its elements with start and length |
| `Lane` | `Voice(staff, n)`, `Ped`, `PedRule`, `Tempo`, `Phrase`, `Mark(staff, kind, n)` |
| `Event` | A note, chord or rest token: grace notes before, the head, token marks, slurs, notes after |
| `Note`, `Pitch` | One pitch of a chord with its tie, display and marks (finger, velocity offset, mute, ...) |

## Analysis

`Analysis::new(&score)` computes, in this order: bar info (meter, key, velocity settings), the leaves (each note or rest with its bar, lane, start and length), value checks, ties, slurs, spans (dynamics, hairpins, ottavas), pedal events, pauses, and the default accidentals. It also has queries:

- `abs(bar, t)`: the time from the start of the score.
- `ottava_at`, `level_at`, `mark_at`: what is in effect at a place.
- `velocity(note)`: the velocity of a written note, from the dynamic level and the lane, token and note offsets. Playback, `check` and `sound --velocity` all use it.

`check`, `sound`, `analyze`, the MusicXML export and the performance all read the same `Analysis`.

## How commands use the helpers

| Helper | Guarantee |
|---|---|
| `load` | Parses only |
| `checked` | Parses and runs `check`; stops with exit code 1 if there is an error |
| `editable` | `checked`, and the file must be in canonical form (`pno fmt` first) |
| `save` | Formats, parses again and checks the result; writes the file only if there is no error |

| Command | Path |
|---|---|
| `check` | `load` → `check::check` → report |
| `fmt` | parse → `fmt::format` → write if changed → `check` |
| `sound`, `analyze` | `checked` → `Analysis` → `analyze::sound` or `analyze::analyze` |
| `bars`, `set`, `transpose`, `phrase` | `editable` → `edit::…` → `save` |
| `import` | MusicXML (`.mxl` and `.mscz` through MuseScore) → `musicxml::import` → `.pno` text and a report of what it drops |
| `export`, `render` | `checked` → `Analysis` → MusicXML or a performance → the output files |
| `spec` | prints `SPEC.md` |
| `a2p` | `a2p::run` |

So an editing command never leaves a file that fails `check`.

## Exit codes

0 is success, 1 is a failed check or a failed program, and 2 is a usage error. `pno a2p` passes on the exit code of `a2p.py`.
