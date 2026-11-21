# pno

A text format for piano scores and their performance, and a CLI for it. A `pno` file holds the notation and the playback data: velocities, tempo, pedal, the timing of single notes and the phrases. An AI agent can read and edit it directly, with few tokens and no arithmetic.

The CLI checks, formats, converts, renders and analyzes the files, and plays them: it writes MIDI, and audio with an SFZ piano. MuseScore 4 draws the pages.

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

How the code works (modules, playback, the audio-to-pno skill, tests) is in [docs/](docs/README.md).

## Playback

The file stores playback data only where it differs from what the notation implies (SPEC section 3.5):

```
@ vel.p=52
4 @ vel.rh2=-15
4 @ voicing.rh=-12

9 phrase | / | _ | _ breath | _ |
9 rh     | E5:v+8 C5^a-20 | ...
```

- `vel.p=52` sets the velocity of each `p` mark, and `vel.rh2=-15` is an offset for the voice `rh2` from bar 4.
- `voicing.rh=-12` makes each chord note of `rh` 12 softer than its main note, from bar 4: the top note, or the pitch marked `^m`. In a lower-staff lane the main note is the bottom note.
- `:v+8` is a note offset; `^a-20` starts a note 20 ticks early.
- In the `phrase` lane, `/` starts a phrase, and a drawn breath mark starts the next one.

`pno export` plays the score:

- **MIDI** (`.mid`) directly, with a track for each staff (`rh`, `lh`). Repeats, voltas and jumps are played out, ties are joined, and ornaments, grace notes, arpeggios, tremolos, fermatas, articulations, the pedal and the tempo are played.
  - Grace notes with a slash, and groups of grace notes, end on the beat. One grace note with no slash is an appoggiatura: it is on the beat, with half the value of its note, at most a quarter.
  - An arpeggio ends on the beat. The voices of one staff roll as one arpeggio.
  - A trill ends on its main note; an ornament on a chord is on its top note.
  - The tempo words "rit.", "ritard.", "riten.", "rall.", "allarg.", "slentando", "accel." and "string." (with "poco" or "molto"), "a tempo" and "Tempo I" change the tempo, in the `tempo` and `txt` lanes.
  - A breath mark, a caesura and a fermata on a barline make a short pause.
- **Audio** (`.wav`, `.mp3`, `.flac`, `.ogg`) with `--sound piano.sfz`: the same MIDI through sfizz, with 4 s for the release sound. The loudest point is at -1 dBFS, and the silence at the end is cut. Without `--sound`, MuseScore plays only the notation.
- **Video** (`.mp4`) with `--sound piano.sfz`: notes fall onto a keyboard, 1920×1080 at 60 fps, with the same audio. `rh` is green and `lh` is blue; `--colors 5CC478,5494E4` sets the two colours. Louder notes are brighter. A faint line marks each C and each bar. While the pedal is down, the background is violet and a violet strip shows above the keyboard; a bright violet line marks each press. The video starts 2 s before the music. `--sheet` (or `--sheet below`) adds the score in a band below the keyboard, black on white; `--sheet top` puts the band at the top of the frame, over the falling notes, at 40 % opacity. The band shows the system that plays, about 6 bars. A red line moves with the music across the notes, and a faint box marks the bar that plays. When the line goes to the next system, the band scrolls up. MuseScore draws the score.
- **`--bars 9-14`** plays only those bars, with the tempo in effect there. A range from bar 1 also plays the pickup (bar 0).
- **`--humanize`** adds small changes of timing and velocity, plays the melody (the top note of `rh`) a little earlier and louder than the other notes, and ends the piece with a ritardando if no slower tempo is in effect there. It prints its seed; `--seed N` repeats a performance.
- **`--phrase`** shapes each phrase: softer and a little slower at its start, louder and a little faster near its middle, and slowest at its end (3 % slower). `--phrase=2` doubles the curve; use more with phrase marks that you checked. A `//` mark starts a section, whose arch adds half of its value to the phrase arch. With no `phrase` line, pno finds the phrases from slurs of two bars or longer in `rh`, and else from groups of 2 to 8 bars (4 if no end is stronger) that end at a section end, a rest, a long note, a bass on the tonic or where an earlier theme comes back. A phrase starts at its upbeat. `pno phrase score.pno` writes the phrases that it finds into the file, with `//` at section ends, and prints why each starts, so you can correct them.

## Install

```sh
cargo install --path .
```

- Rust 1.85 or later.
- [MuseScore 4](https://musescore.org) for `render`, for PDF, PNG, SVG and audio export, and for `.mxl` and `.mscz` import. If it is not in the default place, set `PNO_MUSESCORE` to the path of its program.
- For audio with an SFZ piano (`--sound`): [sfizz](https://github.com/sfztools/sfizz/releases) (the Windows zip contains `sfizz_render.exe`; put it on the `PATH` or set `PNO_SFIZZ`), and [ffmpeg](https://ffmpeg.org) for `.mp3`, `.flac`, `.ogg` and `.mp4` (or set `PNO_FFMPEG`). A free SFZ piano: [Salamander Grand Piano](https://github.com/sfzinstruments/SalamanderGrandPiano) (CC BY 3.0).
- For `pno a2p`: Python 3.10 (`python` on the `PATH`, or set `PNO_PYTHON`) and ffmpeg. The first run makes a Python environment in `~/.cache/audio-to-pno` (about 1.3 GB, 3.8 GB with CUDA) and downloads the models.
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
| `export FILE OUT... [--humanize [--seed N]] [--phrase[=AMOUNT]] [--sound SFZ] [--bars B] [--colors RH,LH] [--sheet [below\|top]]` | MusicXML and MIDI directly; PDF, PNG, SVG and the other MuseScore formats through MuseScore, with no pedal marks (MusicXML, `.mxl`, `.mscz` and `.mscx` keep them, and all playback uses the pedal). Audio through MuseScore, or with `--sound` through sfizz (`.wav`, and `.mp3`, `.flac`, `.ogg` through ffmpeg). A video (`.mp4`) with `--sound`, and with `--sheet` the score under or over the notes (through MuseScore). With several output files, MIDI and the audio and video made with `--sound` play one performance; each option applies to the formats that use it. A file that cannot be written (for example one that another program has open) is named, and the other files are still written. See [Playback](#playback). |
| `render FILE [BARS] [-o OUT] [--dpi N]` | A trimmed PNG of a bar range or of the score. |
| `sound FILE [BARS] [--pedal] [--velocity]` | The pitches that sound at each onset, with held notes and the pedal. `--velocity` adds the MIDI velocity of each new note (`C5@80`). |
| `analyze FILE [BARS] [--all]` | The chord of each beat, dissonances that sound a beat or more between the hands and under the pedal, repeated bars. |
| `set FILE LINE...` | Replaces lines by bar and lane, or adds a lane to a bar. |
| `phrase FILE` | Writes the phrases that `--phrase` finds as `/` and `//` marks, and prints why each starts. |
| `bars insert\|delete\|copy`, `transpose` | Structural edits that are error-prone as text edits. |
| `spec [SECTION]` | Prints the format reference, for example `pno spec 3.7`. |
| `a2p ARGS...` | The evidence tool of the `audio-to-pno` skill (stems, beats, model notes, sung tune, chords, bass, comparison with the recording). `pno a2p -h` lists its commands. |

The editing commands write the file only if the result passes `check`.

## Limitations

| Area | Limit |
|---|---|
| Scope | One piano part: two staves (or one with `@ staves=1`), at most four voices on each staff. |
| Dropped by design | Layout coordinates, page style, fonts, colors, chord symbols, source voice numbers, and the placement of articulations. The import reports each one. |
| Stems and beams | Not kept by default; MuseScore draws them. `--keep stems,beams` keeps them. |
| Not supported yet | Lyrics, multi-bar rests, half pedal, a tuplet across a barline, notes shorter than a 256th, and more (SPEC section 3.12). The import stops and names the feature. It never drops it silently. |
| Rounded tuplets | The import reads exact note lengths, but directions, clefs and grace notes keep the rounded position of the source. |
| MuseScore | MuseScore 4 does not read a tie into another voice. |
| Not an engraver | `render` and PDF export need MuseScore. |
| Playback | A hairpin changes the sound only through the note velocities in the file. `--phrase` has two levels (phrase and section). The sizes of the pauses and of the phrase curve are not tuned by listening tests. `--sound` reads SFZ, not SF2. |

### Corpus coverage

Measured on 2026-11-19. A round trip is import, then export, with the same music as the source.

| Corpus | Files | Round trips | Named stops | Faults | Equal bars, all files | Equal bars, files that import |
|---|---|---|---|---|---|---|
| [ASAP](https://github.com/fosfrancesco/asap-dataset) (classical, Bach to Ravel) | 235 | 139 (59 %) | 94 | 2 | 49.4 % | 100.0 % |
| PDMX (other styles) | 105 | 76 (72 %) | 29 | 0 | 67.3 % | 100.0 % |

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
