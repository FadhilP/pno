---
name: audio-to-pno
description: Transcribe or arrange a recording (mp3, wav, flac, m4a) into a piano score in the `pno` format. Use it when the user gives an audio file and wants piano notes, a piano arrangement, sheet music or a MIDI of it. Do not use it for MusicXML, .mxl or .mscz input, or to edit a score that exists; the `pno` skill does those.
---

# audio-to-pno

This skill gives evidence and checks. It does not convert audio to a score in one step: the models make many errors, and you decide each bar. Expect one long session for a 2-minute piece.

`a2p.py` supplies what you cannot get by ear: the notes two models hear, the bar grid, the loudness of any pitch through time, and a comparison of your score with the recording. Read the `pno` skill first; you write and edit the score with it.

## Setup, one time on a machine

```
python ~/.claude/skills/audio-to-pno/scripts/setup_env.py
```

It makes a venv in `~/.cache/audio-to-pno/venv` (about 1.2 GB), installs the pinned packages (tested with Python 3.10), downloads the piano model (172 MB) and prints the path of the venv Python. Use that Python for `a2p.py`. `ffmpeg`, `pno` and MuseScore must be installed (`pno export` needs MuseScore).

```
PY=~/.cache/audio-to-pno/venv/Scripts/python        # bin/python on Linux and macOS
A2P="$PY $HOME/.claude/skills/audio-to-pno/scripts/a2p.py -w work"
```

The commands are for Bash. Shell variables do not stay between tool calls, so set `PY` and `A2P` again at the start of each call. `-w` is the folder for the files of one song. Use a new folder for each song.

## Workflow

### 1. Prepare (about 2 minutes on a CPU for a 100 s recording)

`$A2P prepare song.mp3` writes `src.wav`, four stems, `pt.json` (piano model) and `bp.json` (sustained-note model).

Read the report to learn what the recording is. Do not trust the title, and do not use notes, key or tempo that you remember for a song.

- Stems: a drum or vocal stem near 0% means that part is absent. It does not mean solo piano.
- "repeat a pitch within 0.25 s": a piano gives a low share. A high share (it was 32% for a recording with a tremolo lead and bass) means sustained or tremolo instruments, which the piano model hears as many short notes.
- For a song with vocals, the melody is in the vocal stem: `$A2P bp vocals`, then `roll --extra bp_vocals`. This path is not tested.

### 2. Fix the grid before any note

`$A2P grid` measures; `$A2P grid --set` saves its suggestion to `grid.json`; `--set beats_per_bar=4 t0=1.23` replaces fields. Each later command reads `grid.json`, so a wrong grid makes each later result wrong.

Decide these, in this order:

1. **Steady tempo.** The line `grid offset in each 10 s window` ends in `steady` or `NOT steady`. If it is not steady (rubato, a live take, a tempo change), stop and tell the user. A constant grid does not fit, and this tool has no other grid.
2. **Beat period.** The tracker often finds half or double the tempo. Test the other one with `--beat-s` (for example `--beat-s 0.75` when it found 0.375) and compare the two tables. Prefer a beat of 0.4 to 1.0 s for which the fastest regular notes are sixteenths. A suggestion of 6 beats in a bar with only beat 1 high usually means that the true beat is two times as long (3 beats in a bar). In the `onset lock` line, a high value at 3 and 6 with a low value at 2 and 4 means a triplet or compound beat (set `quarters_per_beat=1.5` for 6/8; not tested).
3. **Bar length and downbeat.** In the table, the correct bar length has one beat with a clearly higher change of harmony and bass. That beat is the downbeat.
4. **`t0`** is the time of beat 1 of bar 1. Do not use a pickup bar while you work: if the music starts before a downbeat, bar 1 starts with rests (`t0` is then earlier than the first note). You can change it to a pickup bar after the last check.

Confirm the grid with `roll` on some bars: bass notes must be at slot 0, and chord changes at bar starts. The key comes from the `pitch classes` line together with the bass notes of the first and last bars.

### 3. Read the evidence

```
$A2P roll 1-16            # each bar: notes of both models, a triad guess, the strongest low pitch
$A2P raw 12-13 --lo A4    # the same notes, not merged, with beat positions and velocities
$A2P peek 28 G5,B5,E6     # loudness of pitches through time; use it when the models disagree
```

Find the form first (which bars repeat), then the bass note and chord of each bar, then the melody.

- The piano model (`pt`) is better for onsets and for struck or plucked notes. `!` marks a strong note. `xN` marks N fast re-onsets of one pitch: that is one sustained note, and usually the melody instrument.
- The sustained-note model (`bp`) is better for the length of held notes and for a melody that `pt` cuts into pieces.
- Octave errors are common. Harmonics of a loud bass note show as notes: the fifth harmonic of E2 is G#4, which looks like a major third. The third harmonic of F#2 is C#4.
- The triad guess knows only major and minor triads. Read sevenths and inversions from the notes.
- A strong note that repeats in parallel bars is real. A weak note that only one model has, one time, is probably not.
- Parallel passages help: read an unclear bar together with the bar that repeats it.

### 4. Write the score

Write `song.pno` with the `pno` skill: `@ key`, `@ time`, a tempo mark, `ped*`, then all bars. `pno fmt --fix` splits long notes into legal tied values.

You are the arranger. The recording fixes two things: the tune (pitches and rhythm) and the harmony (the bass note and chord of each bar). All else is your decision: texture, register, figuration, which hand plays what, how loud each voice is. The aim is a piece that a pianist would want to play and that a listener knows as the song. Do not copy all detected notes, and do not use one safe pattern for each bar.

**Make the tune heard.** A piano has one timbre, so the tune needs help that the recording got from its instruments. You have three tools; use them together.

- Position. The top note is the easiest to hear. But the tune can be in the middle or in the left hand: a tenor melody below right-hand figuration, a tune that goes from one hand to the other, a bass line that sings for a phrase. Select the place that suits the passage.
- Velocity. Without offsets, each note has the velocity of the dynamic mark and the result is flat. Give each voice a level, then shape the notes:
  - a level for a lane: `@ vel.lh=-14` in the header, or `23 @ vel.lh=-6 vel.rh2=-12` from a bar. About -6 to -12 is enough for balance. Do not move a lane far at the bar where the dynamic mark also changes: the two steps add;
  - one note or chord: `E5:v+8`; one pitch of a chord: `C4^v-18+E4^v-18+G5` (soft harmony below a full top note);
  - a tune in the left hand: make that lane louder and the right-hand figuration softer for those bars, then set them back;
  - phrases: a hairpin is only drawn and does not change the sound, so shape a crescendo, the top of a phrase and its end with note offsets. Change the level of a mark with `@ vel.mf=84`.
  - The sum for a note must be 1 to 127. `pno spec 3.5` has the rules.
- Space. Keep the accompaniment out of the register of the tune, or make it thin where the tune is low.

**Translate, do not transcribe, what a piano cannot do.** A sustained or tremolo note decays on a piano. Possible answers: hold it and let the other hand move; repeat it; a tremolo or a trill; an octave; a rolled chord (`:arp`); a fill or an echo in the gap. A counter-melody from the recording can stay as a second voice (`rh2`, `lh2`), move to the other hand, or become a fill between phrases.

**Give the piece a shape.** Let the texture follow the form: a thin start, more motion or a wider register when a section returns, weight where the recording is loud (octaves in the bass, full chords, a doubled tune), air where it is quiet. Vary the figuration between sections: broken chords, a waltz bass, arpeggios that go through the two hands, block chords, a walking bass. A change must have a musical reason. The models give a different shape in each bar; that is detection noise, and it is not a reason.

**Give each section one left-hand figure.** There are two failures. One figure for the full piece: only the chord changes, and at the climax the bass only gets an octave, so the listener hears a loop. A new figure in each phrase: the texture never settles, and the piece loses its character. No figure is wrong; the amount of change is.

- A section is a large part of the form, such as a verse or a chorus, usually 8 bars or more. A phrase of 2 to 4 bars is not a section, and the second half of a section is not a new section.
- Keep one figure for the full section. In a section, only vary it: a fill, a held chord or a walk at the end of a phrase, where the tune rests. A variation keeps the rhythm or the contour of the figure. A different figure in the middle of a section, such as a waltz bass after broken chords, is not a variation.
- At a section boundary, change the rhythm or the contour. A new register or an added octave alone is not a change.
- When a section returns, its left hand can repeat, become a variation, or become a new figure. A new figure needs a musical reason, for example more weight in a louder return.
- Take the rhythm from the recording's bass and drums. A syncopation or an anticipation is better than even eighths.
- Let the bass line move: passing notes and pickups between the detected bass notes. Use an inversion only where the recording has one.
- `check` lists the left-hand rhythms with a count of bars for each. One rhythm in most bars of the piece is a loop. Many rhythms in one section is too much change.

**Playable, not timid.**

- One hand can take a tenth. A larger chord is possible when it is rolled or when the pedal holds the low note.
- A leap is fine when the pedal or the tempo gives time for it.
- The hands can cross or interlock when you intend it.
- Fast octaves, wide arpeggios and full chords are normal piano writing. Match the difficulty to what the user asked for; if they did not say, an intermediate player is the default.

**Stay honest to the recording.** Keep the tune and the bass as detected, and change them only on evidence (`peek`, a parallel bar). Take accompaniment notes from the chord of the bar, and prefer the chord notes that the models found: if a bar matches badly after you add notes, an added note is not in the chord of the recording. Record each liberty you take (a removed ornament, a moved octave, a new figure), because the report must list them.

### 5. Compare with the recording

`$A2P compare song.pno` exports the score through `pno` and prints three sections. Run it after each group of edits; it exports again each time.

| Section | What it measures | What it cannot tell you |
|---|---|---|
| HARMONY | Pitch classes of each beat, score against recording. | Nothing about octaves or the melody. It changes little when the right hand changes. |
| NOTES | Score notes that no model has, and strong model notes that the score does not have. | It is circular: you wrote the score from these models. Use it to find invented notes and forgotten notes, not as proof. |
| MELODY | For each half beat, if the high score notes are loud in the recording. | A held note that decayed shows as weak. Only a weak TOP note at its onset is a problem. |

MELODY is the only independent test of the tune. When it disagrees with the score, look with `peek`, then change the note. `--bars 9,18` adds the pitch-class profile of bars with a low HARMONY value, to see which note is too strong or absent.

### 6. Check the piano writing

`$A2P check song.pno --song song.json` counts dead spots, left-hand leaps, spans larger than a tenth, right-hand jumps, crossed hands and different left-hand rhythms. With a song file it also lists melody notes that are absent, melody notes below another note, melody notes that are not louder than the other notes struck with them, parallel bars that differ, and notes outside the chords. It prints the velocity range of the score; a range of one value means that no voice has a level.

The findings are places to look at, not rules. A crossing, a leap, a new figure or a tune below the top note is correct when you intended it. A melody note that is below another note or not louder than its accompaniment needs a velocity offset. Correct only what you did not intend.

```json
{
  "melody":    {"4:3": "B4", "4:3.5": "B4", "5:1": "C5", "5:2": "B4"},
  "chords":    {"1": "E G B", "6": "B D# F# A", "20": "E G B D# F# A"},
  "parallel":  [[5, 14], [6, 15]],
  "free_bars": "1-4,21,22,39-43"
}
```

`melody` is the tune as `bar:beat` (beat 1 is the first beat), in the octave where you put it, in either hand. `free_bars` are bars where a hold or a free figure is intended (intro, ending, a rest bar). Write this file from your reading of the recording before you re-voice anything, so that it does not follow the score.

Then render (`pno render song.pno -o work/page.png`) and look at the pages.

### 7. Export, and each later pass

The user judges by ear and by eye, so each version must reach them as files. After the first version, and after each pass that the user asks for, export the two files and confirm that their times changed:

```
pno export song.pno song.mid         # to listen
pno export song.pno song.musicxml    # to open in a notation program
ls -l song.mid song.musicxml
```

If a file time did not change, a program has the file open: export to a new name and tell the user. Do not finish a pass without the two exports, also when the user did not ask for them again.

Before a pass, copy the score (`cp song.pno work/v1.pno`, then `v2`, ...), so that the user can go back. Run `check` on the copy and on the new version to get a before and after table. After the pass, `diff` the `rh` lines against the copy: a re-voicing pass can remove a melody correction from an earlier pass without any check failing. If the pass moves the tune, keep the melody list of the copy and run `check` with it one time: it shows which melody notes moved.

### 8. Report

Give the user:

- the tempo, meter, key, number of bars, and what the recording is (a solo piano, or an ensemble that you arranged);
- the HARMONY mean and the lowest bars, and the before and after table of `check`;
- each place where the score leaves the recording for piano reasons, so that they can undo it;
- the notes you are least sure of;
- the names of the exported MIDI and MusicXML files, first in the report. You cannot hear the result; their ear is the last test.

## Traps

- After `pno export`, confirm that the file time changed. If a player has the file open, `pno export` can print `wrote` and write nothing. `compare` deletes its targets first for this reason.
- `pno sound` reports pitches one octave too high under an `8va` line. The MIDI export is correct. `check` removes the `8va` lane before it reads the score.
- Do not run `pno import song.musicxml` next to the score to test the export: it writes `song.pno` and replaces your score.
- `compare` reads the score through MuseScore. If you edit the score and do not run `compare` again, you read old numbers.

## Limits

Tested on one recording: 99 s, 3/4, steady tempo, no drums, no vocals. Not tested: 4/4 and compound meters, a pickup, a vocal melody, a recording with drums, rubato. Tell the user when a song is outside the tested range.
