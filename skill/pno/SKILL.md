---
name: pno
description: Read, edit and check piano scores in the `pno` text format with the `pno` CLI. Use it when a task changes notes, rhythm, voices, dynamics or structure of a piano score, or when a MusicXML, .mxl or .mscz piano file must be edited.
---

# pno

A `pno` file is a piano score as text: one line for each bar and lane. A text edit of a line is a score edit.

## Workflow

1. A MusicXML, `.mxl` or `.mscz` file: `pno import score.musicxml` writes `score.pno`. The report lists what the format does not keep.
2. Read only the bars you need: search for lines that start with the bar number (`^9 `). A pickup bar is bar 0.
3. Before you change the music, look at it. `BARS` is a range or a list (`9-10,13-14`); without it, all bars.
   - `pno sound score.pno 9-10,13-14`: the pitches of each hand at each onset (`lh ... | rh ...`). A `~` after a pitch is a note that sounds from before. `--pedal` adds the notes that the pedal holds (`*`).
   - `pno analyze score.pno`: the chord of each beat, strong dissonances between the hands (`--all` adds the mild ones), and bars that repeat an earlier bar.
4. Edit the lines. Replace a line by its bar and lane: `sed -i 's/^9 \+lh .*/9 lh | C3 G3 | E4 G3 | C3 D3 E3 G3 | E4 C3 |/' score.pno`. A `sed` with no match changes nothing and says nothing, so read the result.
5. `pno fmt score.pno` until it prints `ok: N bars, M warnings`. It writes the canonical form (column padding too), then checks the full file. Each error gives the bar, the lane and the cell; most give a correction. Warnings (hands cross, a wide span in one hand, a note outside the piano) do not stop the work, but read them.
6. Render one time, after the edit, if the look matters (beams, voices, marks): `pno render score.pno 9-14 -o bars.png`, then look at the image. Line breaks in a range render are not the line breaks of the score; for the page layout, use `pno render score.pno`.
7. If the user needs MusicXML or a PDF: `pno export score.pno out.musicxml` (or `out.pdf`). For MIDI: `pno export score.pno out.mid`. For audio with a sampled piano: `pno export score.pno out.mp3 --sound piano.sfz` (needs sfizz, and ffmpeg for formats other than `.wav`). Tempo words such as "rit." and "a tempo" always play. `--bars 9-14` plays only those bars. `--humanize` adds small changes of timing and velocity and a final ritardando; it prints its seed, and `--seed N` repeats a performance. `--phrase` shapes each phrase with tempo and velocity; `--phrase=2` doubles the curve. The `phrase` lane gives the phrases: `/` starts a phrase, `//` a section, and `breath` and `caesura` start a phrase and are drawn after the note that ends there (`pno spec 3.5`). Without that lane, `--phrase` finds the phrases; `pno phrase score.pno` writes them as `/` and `//` marks and prints why each starts, so you can correct them.

`sound`, `analyze`, `render` and `export` stop with the errors of `check` if the file has errors. `pno check score.pno 9-10` shows only the messages of bars 9-10, but prints `ok` only if the full file is correct.

Structural edits: `pno bars insert|delete|copy` and `pno transpose`. To change bars, prefer `pno set score.pno '9 lh | C3 | - | - | - |' '9 rh2 | ...'`: each line replaces the line with the same bar and lane, or adds that lane to the bar, and the command prints the changed lines. The file must be formatted (`pno fmt`) and correct; these commands write it only if the result passes `check`.

## Grammar

```
@ format=1
@ key=C

1  ped*   | P | - | - | - |
1  rh     | E5 | - D5 | C5 B4 | - |
1  lh     | C3 G3 | E4 | A2 E3 | C4 |
1  rh.dyn | p | _ | _ | _ |
```

- A line is `<bar> <lane> | cell | cell | ... |`. A cell is one beat of the time signature: a quarter in 4/4, a dotted quarter in 6/8. With no `@ time=` line the time is 4/4.
- The tokens in a cell share the beat equally: `| C4 D4 |` is two eighths in 4/4. A plain cell has 1, 2, 4, 8, 16 or 32 slots; a dotted cell has 1, 3, 6, 12 or 24.
- Pitches are absolute: `Bb2`, `C#5`. A chord is one token: `C4+E4+G4`.
- `-` continues the note before it. The full run must be one written value; `check` tells you if not.
- `~` before a pitch ties it to the same pitch before it. Across a barline a held note needs `~`, not `-`: `| ... C5 |` then `| ~C5 ... |`.
- `r` is a rest, `R` a rest for the full bar, `_` is nothing in that voice or lane.
- `_C5` is a note that sounds and is not drawn; `C5:v0` is a note that is drawn and does not sound.
- Lanes: `rh` and `lh` are the first voice of each staff; `rh2`, `lh2` are more voices. Marks: `rh.dyn` (`p`, `mf`, `sfz`, or a text: `"cresc."`), `rh.hp` (`<`, `>`, `-`), `rh.txt` (`"dolce"`), `tempo`, `ped` and `ped*` (`P` press, `C` change, `-` hold). `ped*` repeats in each bar after it.
- Token marks: `:st` staccato, `:ac` accent, `:ten`, `:ferm`, `:arp`. Slur: `(` after the first note, `)` after the last. Two slurs on one note: `C5:su((d` (`:su` is the outer slur, `d` the inner one).
- Tuplet: `3[C4 D4 E4]` in one slot or cell.
- Accidental signs follow from the key and the bar, as in printed music. Write the pitch only.

For other notation, `pno spec 3.7` prints one section of the format: 3.2 grid rules, 3.3 lanes (clef, ottava, tempo, pedal), 3.4 presentation overrides (stems, small notes, cross-staff), 3.5 playback (velocity, offsets), 3.6 meter and bars (pickups, cadenzas), 3.7 marks, spans and lane tokens (grace notes, trills), 3.8 structure and header (repeats, voltas), 3.9 canonical form and errors, 3.10 symbols, 3.11 examples, 3.12 what version 1 does not have.
