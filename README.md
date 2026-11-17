# pno

A text format for piano scores, and a CLI for it. An AI agent can read and edit a `pno` file directly, with few tokens and no arithmetic. The CLI checks, formats, converts, renders and analyzes the files. MuseScore 4 draws the pages.

The first bars of Chopin's Étude Op. 10 No. 3 ("Tristesse"):

```
@ format=1
@ title="Étude Op. 10 No. 3"
@ composer="Frédéric Chopin"
@ key=E
@ time=2/4

0 tempo  | x "Lento, ma non troppo" |
0 rh.txt | x "legato" |
0 rh.dyn | x p |
0 rh     | x B3:su( |
0 lh     | x r |

1 rh.dyn | _ | mp:above |
1 rh.hp  | _ <:above - - | _ |
1 rh     | E4 - D#4 E4) | D#4+F#4:su( |
1 rh2    | G#3 B3 G#3 B3 | A3 B3 A3 B3 |
1 lh     | E2 B2:ac - B2 | B1 B2:ac - B2 |
1 lh2    | E2 | B1 |

2 rh.dyn | _ mf:above | mp:above |
2 rh.hp  | <:above >:above | _ |
2 rh     | ~F#4 G#4 G#4 F#4) | E4+G#4:ac:su( |
2 rh2    | A3 B3 A3 B3 | G#3 B3 G#3 B3 |
2 rh3    | ~D#4 D#4 | _ |
2 lh     | B1 B2:ac - B2 | E2 B2:ac - B2 |
2 lh2    | B1 | E2 |
```

- Each line is one bar and one lane: `<bar> <lane> | cell | cell | ... |`. Bar 0 is the pickup; `x` is time that is not in the bar.
- A cell is one beat of the time signature. The tokens in a cell share the beat equally: `| G#3 B3 G#3 B3 |` is four sixteenths in 2/4.
- Pitches are absolute. `-` continues a note, `~` ties a pitch, `+` joins a chord, `r` is a rest.
- `rh2` and `rh3` are more voices of the upper staff. `rh.dyn` and `rh.hp` hold dynamics and hairpins. `(` and `)` are slurs, `:ac` is an accent.
- `fmt` gives one canonical form, so a text diff is the score diff.

The full format is in [SPEC.md](SPEC.md). `pno spec` prints it one section at a time.

## Install

```sh
cargo install --path .
```

- Rust 1.85 or later.
- [MuseScore 4](https://musescore.org) for `render`, for PDF, PNG, SVG and audio export, and for `.mxl` and `.mscz` import. If it is not in the default place, set `PNO_MUSESCORE` to the path of its program.
- For audio with an SFZ piano (`--sound`): [sfizz](https://github.com/sfztools/sfizz/releases) (the Windows zip contains `sfizz_render.exe`; put it on the `PATH` or set `PNO_SFIZZ`), and [ffmpeg](https://ffmpeg.org) for `.mp3`, `.flac` and `.ogg` (or set `PNO_FFMPEG`). A free SFZ piano: [Salamander Grand Piano](https://github.com/sfzinstruments/SalamanderGrandPiano) (CC BY 3.0).
- For Claude Code, copy `skill/pno` to `~/.claude/skills/pno`, and `skill/audio-to-pno` to `~/.claude/skills/audio-to-pno`.

## Use

```sh
pno import score.musicxml          # writes score.pno and reports what it does not keep
pno sound score.pno 9-10 --pedal   # pitches of each hand at each onset
pno set score.pno '9 lh | C3 | - | - | - |'
pno fmt score.pno                  # canonical form, then check
pno render score.pno 9-14 -o bars.png
pno export score.pno out.musicxml  # or out.pdf, out.mid
pno export score.pno out.mp3 --sound piano.sfz --humanize --phrase
```

| Command | Function |
|---|---|
| `check FILE [BARS]` | Syntax, bar fill, note values, ties and open spans. Each error gives the bar, lane and cell, and most give a correction. |
| `fmt FILE [--fix] [--check]` | Writes the canonical form, then checks. `--fix` splits invalid `-` runs and renumbers the bars. |
| `import FILE [-o OUT] [--keep stems,beams]` | MusicXML (and `.mxl`, `.mscz`) to `pno`, with a report of each dropped element. |
| `export FILE OUT [--humanize [--seed N]] [--phrase] [--sound SFZ]` | MusicXML and MIDI directly; PDF, PNG and SVG through MuseScore. Audio through MuseScore, or with `--sound` through sfizz with an SFZ instrument (`.wav`, and `.mp3`, `.flac`, `.ogg` through ffmpeg); its loudest point is at -1 dBFS. `--humanize` plays MIDI and `--sound` audio with small changes of timing and velocity; the same seed gives the same result. `--phrase` shapes each phrase: softer and slower at its ends, louder and up to 2 % faster near its middle. |
| `render FILE [BARS] [-o OUT] [--dpi N]` | A trimmed PNG of a bar range or of the score. |
| `sound FILE [BARS] [--pedal]` | The pitches that sound at each onset, with held notes and the pedal. |
| `analyze FILE [BARS] [--all]` | The chord of each beat, dissonances between the hands, repeated bars. |
| `set FILE LINE...` | Replaces lines by bar and lane, or adds a lane to a bar. |
| `bars insert\|delete\|copy`, `transpose` | Structural edits that are error-prone as text edits. |
| `spec [SECTION]` | Prints the format reference, for example `pno spec 3.7`. |

The editing commands write the file only if the result passes `check`.

## Limitations

| Area | Limit |
|---|---|
| Scope | One piano part: two staves, at most four voices on each staff. |
| Dropped by design | Layout coordinates, page style, fonts, colors, chord symbols, source voice numbers, and the placement of articulations. The import reports each one. |
| Stems and beams | Not kept by default; MuseScore draws them. `--keep stems,beams` keeps them. |
| Not supported yet | Lyrics, multi-bar rests, glissando lines, half pedal, a tuplet across a barline, notes shorter than a 128th, a bar that is longer than its time signature by a part of a beat, and more (SPEC section 3.12). The import stops and names the feature. It never drops it silently. |
| Rounded tuplets | The import reads exact note lengths, but directions, clefs and grace notes keep the rounded position of the source. |
| MuseScore | MuseScore 4 does not read a tie into another voice. |
| Not an engraver | `render` and PDF export need MuseScore. |
| Playback | The MIDI and `--sound` export play the stored velocities and tempo marks. A hairpin changes the sound only through the note velocities in the file (SPEC section 3.5). With `--humanize`, the words "rit.", "rall.", "accel." and "a tempo" (in the `tempo` and `txt` lanes) change the tempo, and the piece ends with a ritardando if the score has none there. `--phrase` finds the phrases from slurs of two bars or longer in `rh`, and else from groups of 2 to 8 bars that end at a section end, a rest, a long note or a bass on the tonic; there is no way to write phrases in the file. `--sound` reads SFZ, not SF2. |

### Corpus coverage

Measured on 2026-11-15. A round trip is import, then export, with the same music as the source.

| Corpus | Files | Round trips | Named stops | Faults | Equal bars, all files | Equal bars, files that import |
|---|---|---|---|---|---|---|
| [ASAP](https://github.com/fosfrancesco/asap-dataset) (classical, Bach to Ravel) | 235 | 87 (37 %) | 113 | 33 | 37.5 % | 99.0 % |
| PDMX (other styles) | 105 | 43 (41 %) | 57 | 5 | 46.9 % | 98.6 % |

- A named stop is a file with a feature that `pno` does not support yet. The import stops and names it. All bars of such a file count as not equal.
- A fault is a file that imports but does not come back the same.
- The target is 95 % round trips, so it is not met yet.
- A `pno` file that is exported to MusicXML and imported again comes back byte for byte. The tests check this on all fixtures.
- The corpus files are not in this repository (ASAP is CC BY-NC-SA).

### Size

Measured on the files that round-trip. Each value is the size of the other format divided by the size of the `.pno` file.

| Format | ASAP | PDMX |
|---|---|---|
| MusicXML with the same content | 33× | 31× |
| The same, compressed (`.mxl` vs compressed `pno`) | 3.5× | 3.9× |
| LilyPond from `musicxml2ly` | 1.8× | 1.6× |

## Tests

```sh
cargo test
PNO_CORPUS=<folder of MusicXML files> cargo test --release --test corpus -- --ignored --nocapture
```

The MuseScore tests skip when MuseScore 4 is not installed.

## License

MIT
