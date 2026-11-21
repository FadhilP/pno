# Playback and export

The user-facing rules of playback are in the README section "Playback". This document shows how the code does it.

## Export dispatch (`main.rs`)

`pno export FILE OUT...` can write several files in one run. Each output file gets a kind from its extension (`output_kind`):

| Kind | Extensions | Made from |
|---|---|---|
| `Midi` | `.mid`, `.midi` | The performance without a release tail |
| `Audio` | `.wav`, `.mp3`, `.flac`, `.ogg` | With `--sound`: the performance with a release tail, through sfizz. Without: MusicXML through MuseScore |
| `Video` | `.mp4` | The performance with a release tail (needs `--sound`) |
| `MusicXml` | `.musicxml`, `.xml` | MusicXML with pedal marks |
| `ScoreFile` | `.mxl`, `.mscz`, `.mscx` | MusicXML with pedal marks, through MuseScore |
| `Notation` | every other extension (PDF, PNG, SVG) | MusicXML without pedal marks, through MuseScore |

1. The Export arm checks the options against the kinds (for example, `--colors` needs a video, and `--humanize` needs MIDI, audio or video).
2. It builds each shared product only once, and only if an output needs it: the plain performance, the performance with a tail, the MusicXML with pedal marks, and the MusicXML without them (`Sources`).
3. `export_one` writes each file. A failed file is reported, and the other files are still written. So MIDI, audio and video of one run play one performance.

## The performance (`perform.rs`)

`perform(analysis, options)` returns a `Performance`: the played notes (`Played`: on, off, key, velocity, staff), the pedal, the tempo map, the meters and the bar times. `Options` has `humanize` (off or a seed), `shaping` (off or `--phrase` amount), `bars` (a range) and `tail` (none, or the release time for sfizz).

The stages, in order:

1. **Play order.** `play_order` plays out repeats, voltas and jumps. After a jump, repeats do not play again, and each volta plays its last ending.
2. **Phrase arcs** (only with `--phrase`). `Arcs` gives a tempo and velocity curve for each phrase.
3. **Tempo map.** Tempo marks, tempo words (`rit.`, `accel.`, `a tempo`, `Tempo I`), fermatas and pauses become a map from score time to ticks.
4. **Window.** With `--bars`, only that range plays, with the tempo in effect there.
5. **Notes.** `leaf_notes` makes the raw notes of each written note, with grace notes and notes after. The velocity comes from `Analysis::velocity`, before any timing change.
6. **Ties.** `join_ties` joins tied notes.
7. **Figures.** `play_figure` plays ornaments and tremolos. Each ornament note after the first is softer.
8. **Timing.** `timed` adds the attack offsets, spreads arpeggios so that they end on the beat, and, with `--humanize`, changes timing and velocity a little and plays the melody early and louder.
9. **Overlaps.** `remove_overlaps` merges two strikes of one key that are very near, and else cuts the earlier note just before the next one.
10. **Pedal.** The pedal lanes become CC 64 events, with a short gap at each pedal change.
11. **Tail.** For audio and video, the release time of the last notes is added, because sfizz stops at the end of the MIDI.

## MIDI (`midi.rs`)

A format 1 file with three tracks: a conductor track (tempo and time signatures), `rh` and `lh`. Both staves use one channel, so the pedal holds both. Note-offs have velocity 64. At one tick, note-offs come before controls, and controls come before note-ons.

## Audio (`audio.rs`)

`render` writes the MIDI to a temporary file and runs `sfizz_render` with the SFZ instrument (`PNO_SFIZZ`, else the `PATH`). It normalizes the WAV so that the loudest point is at -1 dBFS, and it converts other formats with ffmpeg (`PNO_FFMPEG`).

## Video (`video.rs`)

`render` draws each frame (falling notes, a keyboard with the held keys lit, bar lines, the pedal) and pipes raw RGB frames to ffmpeg (H.264). `mux` adds the audio (AAC), delayed by the lead-in of the video. The audio and the frames render at the same time in two threads.

With `--sheet`, `sheet.rs` adds a band with the score. One MuseScore run (a job file) writes the measure boxes (`.mpos`), the note columns (`.spos`) and the pages (PNG) of the MusicXML without a title frame. Each system is cut from its page (from the middle of the gap above to the middle of the gap below), kept as ink alpha, and shrunk to fit the band. The band draws black ink on white paper: opaque below the keyboard, at 40 % opacity at the top, where it lies over the falling notes. The cursor follows the performance, not MuseScore's timing: each played bar maps to its written measure, and the written onsets of the bar (`perform::bar_onsets`) map to the note columns of the measure, in order. The times come from the tempo map of the performance, so rubato, fermatas, repeats and `--bars` move the cursor too. If the number of onsets and columns of a bar is not the same, the cursor moves in a line across that bar; other bars are not changed (`sheet::anchors`).

## MusicXML (`musicxml.rs`, `musicxml/`)

- **Export.** `musicxml::export(analysis, scope, pedal_marks)` writes one measure for each bar: attributes, notes and chords, notations, directions, tempo, and the pedal (only with `PedalMarks::Show`).
- **Import.** `musicxml::import` reads the file (`read`), builds the `.pno` model (`build`), fixes ties, slurs and velocities, writes the canonical text (`fmt::format`), and parses the result again to verify it. It returns the text and a report of each dropped element (`import/report.rs`).

## MuseScore (`render.rs`)

`find_musescore` uses `PNO_MUSESCORE`, then the `PATH`, then the standard install places. `convert` first deletes old output files, because MuseScore reports success when it cannot replace a file. Then it runs MuseScore and collects the output (one file, or numbered pages). `pno render` and the PDF, PNG, SVG, `.mscz` and audio exports use it. `convert_all` makes several outputs of one input in one MuseScore run, for the video sheet.
