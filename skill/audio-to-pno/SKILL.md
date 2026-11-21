---
name: audio-to-pno
description: Transcribe or arrange a recording (mp3, wav, flac, m4a, webm, opus, ogg) into a piano score in the `pno` format. Use it when the user gives an audio file and wants piano notes, a piano arrangement, sheet music or a MIDI of it. Do not use it for MusicXML, .mxl or .mscz input, or to edit a score that exists; the `pno` skill does those.
---

# audio-to-pno

This skill gives evidence and checks. It does not convert audio to a score in one step: the models make many errors, and you decide each bar.

`pno a2p` supplies what you cannot get by ear: the notes three models hear, the bar grid, the loudness of any pitch through time, and a comparison of your score with the recording. Read the `pno` skill first; you write and edit the score with it.

## Setup

`pno a2p` sets itself up on its first run, and again after a `pno` update that changes the setup: it makes a venv in `~/.cache/audio-to-pno/venv` with `python` from the PATH (or `PNO_PYTHON`; about 1.3 GB, tested with Python 3.10), and downloads three models (172 MB, 81 MB and 12 MB). With an NVIDIA GPU (`nvidia-smi` on the PATH) it installs the CUDA build of torch (the venv is then about 3.8 GB), and the models of `prepare` run on the GPU. The first `prepare` downloads the lead-voice model (204 MB) and the stem models of step `parts` (1.2 GB). `ffmpeg`, `pno`, MuseScore (PDF, `render`, `compare`) and sfizz (the video) must be installed. If `sfizz_render` is not on the PATH, set `PNO_SFIZZ` to its path.

```
A2P="pno a2p -w work"
```

The commands are for Bash. Set `A2P` again in each tool call: shell variables do not stay. `-w` is the folder for the files of one song; use a new folder for each song.

Each command prints only a summary and the places with a problem. `compare` and `check` print at most 12 rows for each list (`--rows N` changes it) and write all rows to a file in `work/`. Read that file only when you need the rows.

## Workflow

### 1. Prepare

`$A2P prepare song.webm` writes `src.wav`. Then it splits the stems and tracks the beats. Then a karaoke model writes `lead.wav`, the lead voice without backing vocals, for `tune`; and the three note models and the chord model read `backing.wav` (the bass stem plus the `other` stem), because a voice and drums give the models false notes, and basic-pitch also reads the bass stem. A second split model writes `guitar.wav`, `piano.wav` and `other.wav`, and two more models write `strings.wav`, `brass.wav`, `woodwinds.wav`, `organ.wav` and `synth.wav`: Transkun reads the piano stem, and basic-pitch reads the guitar stem and these five. On a GPU it takes a little less than the length of the song (211 s for a 4-minute song on an RTX 5070; the layer stems add 100 s of it); on a CPU, more than three times the length (the lead-voice model and the stem models take most of it; not measured on a full run). Give it the file as the user downloaded it: ffmpeg reads each audio format, and each lossy encode removes detail. From YouTube, `yt-dlp -f bestaudio URL` gives the original stream; `--audio-format mp3` encodes it again.

The report tells you what the recording is. Do not trust the title, and do not use notes, key or tempo that you remember for a song.

- Stems: a drum or vocal stem near 0% means that part is absent. It does not mean solo piano.
- A bass stem of about 10% or more means a bass instrument: step 3 reads its line with `$A2P bass`.
- `guitar`, `piano` and `other` come from the second split model: `other` is what stays after the voice, drums, bass, guitar and piano. `strings`, `brass`, `woodwinds`, `organ` and `synth` are layers of the mix; step 3 tells how far to trust them. A value near 0% (`silent` in `stems play`) means that the instrument does not play. On the test song (piano and voice), the instrument of the intro is in `other` and `brass`, and not in `piano`.
- `notes`: the note count of each model, and of each group of models that agree on a note. `t` is Transkun (a piano model), `p` is the first piano model, `b` is basic-pitch (sustained notes).
- "Transkun has N% of the notes that pt and bp agree on": 95% or more was always solo piano in the tests, and less than 89% was never solo piano. Between them, the other lines and the user decide.
- "N% of the bp notes repeat a pitch within 0.25 s": a piano gives less than 10%. A high value (30%) means sustained or tremolo instruments.
- With vocals (a vocal stem of about 10% or more), the melody is in the vocal stem: step 3 reads it with `$A2P tune`.
- Decide now if the recording is solo piano. If you are not sure, ask the user. Steps 3 and 4 depend on it: for solo piano, give `--view piano` to each `roll` and `raw`.
- `stems play`: the times in seconds where each stem is above -40 dB of the top of the mix (1 s frames). It gives the voice start, where the drums come in and go out, and the form. `the mix fades out from A s to B s`: the song ends with a fade. `$A2P levels` prints these lines again, also for a folder made before them.
- `tuning`: the instruments (`backing`) and the voice in cents from A440. On shifted audio it was right within 2 cents. `tune` corrects the voice for it. A value near ±50 cents puts the recording between two semitones: expect pitch errors of one semitone from the models, and tell the user.

### 2. Fix the grid before any note

`$A2P grid` prints two grids: the tracked grid (it follows the beat tracker and each tempo change) and the best constant grid. It lists the bars where they differ. `$A2P grid --set` saves the tracked grid, and `$A2P grid --constant` saves the constant grid. Each later command reads `grid.json`, so a wrong grid makes each later result wrong. Decide these, in this order:

1. **Which grid.** If the line says `steady`, use `--constant`. If the tempo changes, use the tracked grid. If the line `the tracker missed N beats` is there, look at those bars in step 3.
   - With drums, the `drum fit` line gives a third grid: the beat of the drum hits, at the phase of the note onsets. For each grid it gives the median offset of the strong drum hits and of the note onsets from the sixteenths (`drums/notes` in ms). `--constant` saves the drum fit when both offsets are smaller than for the constant grid; the line says which grid it saves. On a test song the drum fit matched a hand-made grid (beat within 0.1 ms, phase within 3 ms), where the constant grid was 57 ms late at the start and 42 ms early at the end. In 39 songs with drums, `--constant` chose the drum fit in 16, and against the annotated beats of 22 RWC songs the saved grid was never farther than the constant grid (up to 23 ms nearer). With swing or drums in another subdivision than the notes, the fit is not chosen.
   - The `drift` lines give the mean offset of the drum onsets and of the `other4` onsets from the sixteenths of each grid, for each 8 bars (+ = the onsets are after the grid). `other4` is the other stem of demucs (`grid` was tuned on it); it also has the guitar and the piano. Use the grid whose values stay near one number through the song. Values that grow or fall mean a wrong beat. The `other4` values also cover parts without drums. A value wraps at half a sixteenth.
2. **Beat level.** The tracker often finds a beat that is two, three or four times too fast or too slow (in half of the rendered test clips). Its tempo is not proof. Take the beat for which the fastest regular notes are sixteenths. In the `onset lock` line, the last division with a high value, before the values fall, gives the fastest regular notes. The note value of the beat comes from them, not from the tempo: a quarter of 0.36 s with sixteenths (6/4 at 164) is correct. Only when two levels fit, prefer a beat of 0.4 to 1.0 s.
   - The values are low to 4 and high at 8: `--set level=double` splits each beat.
   - The values are high at 1 or 2 and low at 4: `--set level=half` joins two beats.
   - Give a field two times for a factor of four: `--set level=half level=half`.
   - Compound meter (6/8, 12/8): with 3 or 6 beats in a bar, the tracked beat is the eighth note when the values are high at 2 and low at 4. Use `--set level=third quarters_per_beat=1.5 beats_per_bar=2`. When they are also high at 4, the tracked beat is the quarter note (3/4, 6/4) and needs no level field. If the beat is already the dotted quarter (high at 3 and 6, low at 2 and 4), use only `quarters_per_beat=1.5`. At a dotted-quarter beat, 6 is the sixteenths and 12 the 32nd notes: a high value at 12 means that the beat is the dotted half of 6/4 or 3/4.
   - The line `the tracker changed between two beat levels` means that the grid has the faster level.
   - Two levels can both be correct (4/4 at 150 with eighths, or at 75 with sixteenths). Tell the user which you used.
3. **Bar length and bar 1.** `--set beats_per_bar=4` changes the bar length. `--set shift=1` starts bar 1 one beat later (`shift=-1`: earlier). Do this when less than 80% of the tracked downbeats agree, or when `roll` shows the bass after slot 0. Do not use a pickup bar while you work: if the music starts before a downbeat, bar 1 starts with rests. You can change it to a pickup bar after the last check.
   - The `backbeat` line counts the kick and snare hits on each beat of the bar (`2:0/15`: beat 2 has 0 kicks and 15 snares). `the snare is on beats 2 and 4`: the bars start right or two beats off. The drums do not tell beat 1 from beat 3 (the kick was stronger on beat 3 in 6 of 22 RWC songs): take that from the tracked downbeats and the bass. `the snare is on beats 1 and 3`: give `shift=1` or `shift=-1`. `the snare is on beat N only`: the beat can be two times too fast (`level=half`), or the drums play half time. `the snare is between the beats`: `level=double`. The verdict `2 and 4` was right in all 20 songs with known bars where it came (19 RWC songs and the test song), and it agreed with the tracked downbeats in 12 other band songs. `1 and 3` came once, against 98% of the tracked downbeats, in a song with swing: confirm it with `roll`. There is no verdict for an odd number of beats in a bar.
   - The `chord change` line gives how much the chords of the chord model change at each beat of the bar (`1:.57 2:.19 3:.71 4:.23`). The drums do not tell beat 1 from beat 3, and the chords usually do: when another beat changes clearly more than beat 1, the line names it and the `shift` that moves bar 1 there. On 24 RWC songs with known bars, beat 1 led in 23 (in the other, beat 4 had 1.07 times beat 1), and with the bars two beats off the line named the right beat in all 24 and in the test song. A song whose chords change on each beat or anticipate the bar gives a weak line: confirm a shift with `roll`.
   - `lead starts at 1.2 s, 1 bar before bar 1`: a stem plays before bar 1, often an intro that the note models do not hear (the voice or the drums alone). Give the `shift` that the line prints, so that bar 1 starts before the music. Check with `levels` that it is music and not noise (a stem near -40 dB for a long time is noise).
4. **Bars of another length.** The line `bars of another length` means that the tracked downbeats moved to another beat, and the grid has one shorter or longer bar there. In tests, the bar was the right one in 6 of 10 cases, and at most one bar off in 8 of 10. Look at 3 bars to each side with `roll`: after the right bar, the bass is at slot 0 and the chords change at bar starts. To move a bar, or to add a bar that you hear, give all such bars: `--set bar=33:2 bar=59-66:3` (`bar=1:4` means none). The bar fields replace all found bars. A 6-beat bar can show as a 2-beat bar some bars later: use `bar=N:6`. If a beat near the found bar is two times as long as the others, the tracker lost a beat and the music has no odd bar. Keep the found bar, because the later bars need it. In the score, give that bar its own `time=`, and give the next bar the usual `time=`.

Fields go together, and the level fields come first: `$A2P grid --constant --set level=half beats_per_bar=4 shift=2`.

Confirm with `roll` on some bars at the start, the middle and the end: bass notes must be at slot 0, and chord changes at bar starts. If the bass leaves slot 0 in many bars of a slow, free recording, the tracker did not follow it: tell the user. The key comes from the `pitch classes` line together with the bass notes of the first and last bars.

### 3. Read the evidence, then write `song.json`

```
$A2P roll 1-16            # one line for each bar: chord, bass note, low notes, high notes
$A2P roll 1-16 --view piano   # solo piano: only the notes that Transkun has
$A2P raw 12-13 --lo A4    # the same notes with exact beat positions and velocities, re-onsets not merged
$A2P peek 28 G5,B5,E6     # loudness of pitches through time; use it when the models disagree
$A2P peek 14 D3,D4 --stem lead   # the same in one stem: lead, vocals, bass, other, drums, guitar, piano, strings, brass, woodwinds, organ, synth or backing
$A2P roll 10-13 --stem piano  # all notes of one instrument: piano (Transkun), or guitar, strings, brass, woodwinds, organ, synth (basic-pitch); also for raw
$A2P roll 9-12 --beats    # also the chord of each beat
$A2P levels 1-16          # the level of each stem and of the mix in each bar
```

A `roll` line is `5 Em E3 | 0:E2/1tb 2:B2/1 | 0:C5/4 4:B4/4! 8:E5/9x5pb | +5`.

- `Em` is the chord of the bar from a chord model: the root and the triad (major, `m`, `dim`, `aug`, `sus2`, `sus4`). `Bm>F#m` means that the two halves of the bar have different chords (even meters only). `?` marks a chord that the model is not sure of, and `-` is no chord. The model does not give sevenths and inversions: read them from the notes and the bass field.
- A note is `slot:pitch/length` in sixteenths. `!` marks a strong note. `xN` marks N fast re-onsets of one pitch: that is one sustained or tremolo note, and usually the melody instrument.
- The letters are the models that have the note. No letter means all three: that note is the most certain.
- `+5` counts the hidden notes of the bar. `--view all` shows them.

Which notes to trust:

- **Solo piano: `--view piano`.** It shows the notes that Transkun has. In the tests these had 99% of the true notes, and 98% to 100% of them were true. A note without `t` was false in 9 of 10 cases. Velocities and lengths are from Transkun. The lengths are key releases: with the pedal down, the sound is longer.
- **Other recordings: the default view, `mix`.** The piano models do not hear a lead instrument that is not a piano: in rendered ensembles, the notes of two models had 3 of 4 tune notes or fewer, and `b` alone had 9 of 10. So `mix` shows the notes of two models and each note of 0.25 s or longer that only `b` has. A long `b` note in the high part is often the tune. In the tests, 3 to 4 of 10 notes in this view were false: confirm the tune with `peek` and with parallel bars. Use `--view all` for a bar where the tune or the bass is absent.
- The chord is not from the shown notes: the chord model hears `backing.wav`. In pop songs its root and triad are right about 6 of 7 of the time; for solo piano and rendered ensembles, about 19 of 20. A chord without `?` has the right root in more than 9 of 10 bars. Where the bass field has a different note, that chord is still right in 4 of 5 bars: the bass then plays an inversion or a passing note. A chord with `?` (about 1 of 8) has the right root in about half of the bars, and in 2 of 3 where the bass field has the same note: confirm it with the notes and the parallel bars.
- The bass field is the first note of the bass stem that starts in the bar. Where the stem is silent (a solo piano, a bar where the bass rests), it is the lowest shown note that starts in beat 1, and `-` when there is none. It has the note name of the bass in more than 9 of 10 bars, and the octave in about 9 of 10.
- Velocities are from the piano models only. A value with `a` in `raw`, such as `(a43)`, is a `bp` amplitude; do not use it for dynamics.
- Octave errors are common, and the harmonics of a loud bass note show as notes.
- A strong note that repeats in parallel bars is real. Read an unclear bar together with the bar that repeats it.
- `--beats` adds a line under each bar: `beats | 1 G .58 (C .37) | 2 C .95 | ...`, the chord of each beat with its probability, and the second chord when it has 0.2 or more. Use it where the bar chord has `?` or `>`, or where the harmony moves inside the bar. In 24 RWC songs the root of the first chord was right on 88% of the beats. A second chord came on 1 beat in 6; there the first chord was right in 6 of 10, the second in 4 of 10, and one of the two in 86%.
- `peek --stem lead` reads one stem instead of the mix. Use it for the octave of a low voice: in the mix the instruments hide it, and in `lead.wav` the sung pitch is louder than the octave below it. `--stem bass` does the same for the bass.
- **One instrument: `--stem piano`, `--stem guitar` and the layer stems** (`roll` and `raw`). Use it to list the layers of a section (`arranging.md`): the piano figure, the guitar riff, a string pad, a horn line, and where each plays. It shows all notes of one model on that stem, and no notes where the stem is below -40 dB. For a folder made before this feature, run `$A2P step parts`. In 12 RWC pop songs, against the notes of each instrument:
  - piano: about 4 of 5 notes were true, and about 4 of 5 piano notes were found (F .81; the `t` notes of `roll`: .55). In 3 of 11 songs it was below .7 (.16 in one).
  - guitar: about 4 of 10 notes were true, and about 4 of 10 guitar notes were found (F .39). Strummed chords give many false notes. Use it for the rhythm and register of a riff, and confirm the pitches with `peek --stem guitar`.
  - Where the instrument did not play, the stem was usually below -60 dB and gave almost no notes (0 to 15 piano notes in a song without a piano).
  - `strings`, `brass`, `woodwinds`, `organ`, `synth`: trust them for when the instrument plays, its register and its rhythm. Do not trust their pitches: in 24 RWC songs, note F of the notes that `--stem` shows was strings .27 (11 songs), brass .55 (5), woodwinds .66 (3), organ .14 (10), synth .30 (15); without the -40 dB gate, .10 to .39. Brass rests on 5 songs and woodwinds on 3: take their numbers as weak. Confirm each pitch with `peek --stem`.
  - `strings` misses quiet string parts, and it failed on 2 of 11 songs (F .03 or less): on one the stem was silent, on the other it kept other instruments where no strings played. Its notes of a string part are a little better than those of `other` (F .22 against .18, both without the gate).
- `levels` gives the level of each stem and of the mix in each bar, in dB below the top of the mix (`-` = below -60 dB, the stem is silent). Use it to find the form (where the drums stop, where the voice comes in) and the dynamics: the texture of the piano follows the loudness of the mix. Without `grid.json` it gives one row for each 4 s. The `guitar` and `piano` stems were above -40 dB in 99% of the seconds where the instrument played, and in about 1 of 8 of the seconds where it did not (bleed of another instrument). For the layer stems, the share of the seconds above -40 dB where the instrument played / did not play: `brass` .95 / .06, `woodwinds` .86 / .05, `synth` .86 / .09, `organ` .94 / .18, `strings` .84 / .22 (`other` .95 to 1 / .57 to .86). So `brass`, `woodwinds`, `synth` and `organ` above -40 dB mean that the instrument plays; `woodwinds` and `synth` can miss a quiet part. `strings` above -40 dB means strings in 9 of 11 songs; in the other 2 it was loud in most of the seconds without strings, and it misses quiet string parts.

**The bass line: `bass`.** `$A2P bass 1-16` prints the notes of the bass stem in the `tune` format (`~` and `?` as there). When the bass stem is silent it stops with "the bass stem is silent": read the bass from `roll`. In pop songs about 7 of 10 of its notes are a bass note within one sixteenth: its onsets are often one slot late, so prefer the beat. Notes with `?` are true less often. A long note can show as some short notes of one pitch. Low `roll` notes that are not in this line are mostly chord notes: only half of the `roll` notes below E3 were bass notes.

**A sung tune: `tune`.** The note models do not read the vocal stem, so `roll` does not show the voice. `tune` follows the pitch of the lead voice (`lead.wav`: the vocal stem without backing vocals) and gives one note at a time:

```
$A2P tune                 # each sung bar; also writes work/tune.json, a draft melody without the ? notes
$A2P tune 9-16 --octave 1 # some bars, one octave higher (for a low voice)
```

A `tune` line is `29 | 0:C#5/4^ 4:D4/2~ 6:D3/1? 7:D4/1~? 8:E5/3^? 12:B4/7`, in the `roll` slots. The marks:

- `^`: the singer starts the note low (a scoop): one semitone low for up to 0.45 s, or two semitones low for up to 0.33 s, and no new syllable starts at the step up. `tune` writes the note at the start of the scoop, where the beat is.
- `~`: the onset is late in its slot, so it can belong to the next slot.
- `+` or `-`: the steady part of the note is 45 cents or more above or below the written pitch, so it is between two semitones. In 24 pop songs, 1 note in 10 had this mark; 6 of 10 of them were not the written pitch, and then usually the semitone in the direction of the mark.
- `?`: the note is doubtful. It is shorter than 0.16 s; or 12 dB or more softer than a note within 1.5 s (a fading end, a breath, bleed); or an octave error (the stem has almost no sound at the pitch and much an octave higher); or an octave or more from the reliable notes around it.

The first line gives the sung bars, the range and the count of each mark.

- A bar with no line is not sung. Take the tune there from `roll` (an intro or a solo is often an instrument).
- In pop songs, about 3 of 4 notes without `?` are the sung note in pitch and slot, and 8 of 10 within one slot; above 135 BPM, about 2 of 3. About 1 in 5 notes without `?` is false, and less than half of the `?` notes are true. Correct the rest with the parallel phrase.
- Above 135 BPM (a sixteenth shorter than 0.11 s), `tune` writes the notes on eighths (even slots): a sung sixteenth there is lost. `bass` stays on sixteenths.
- In these rules, a parallel phrase is the same phrase sung again, or played by an instrument (an intro or an outro often plays the tune).
- `=25,41` after a bar: the voice sings the phrase of the bar again in bars 25 and 41. In pop songs 19 of 20 of these pairs were true, but `tune` finds only about half of the repeats: a bar without `=` can have one. Put the pairs that you confirm into `parallel` of `song.json`.
  - A note that a `=` bar also has (same pitch, at most one slot away) is true in 9 of 10 cases; a note that no `=` bar has, in less than half. For a `?` note: 7 of 10, and 2 of 10.
  - Do not add a note only because a `=` bar has it: it was right in 4 of 10 cases.
  - Move a note to another slot only when two or more `=` bars have it at that slot and no `=` bar has it at its own slot (right about 3 times for each wrong). One `=` bar with another slot shows nothing.
- `inst 1` under a bar: an instrument plays the phrase of the bar in bar 1. The line has the top notes of that bar as slot:pitch, in the octave of the tune, with the model letters of `roll`. `tune` finds this only where the note models hear the instrument clearly (piano and voice); in band recordings it finds almost none, also where an instrument plays the tune. The phrase often continues in the next bars: read `roll` there.
- Take the rhythm from an `inst` line: its onsets are sharp, and the voice's are not. When a note there that `t` or `p` has (no letter, or a letter `t` or `p`) is one or two slots from the `tune` note of the same pitch, write its slot. A note with only `b` has no sharp onset. Tested on one song.
- Without an `inst` line, do not move a `tune` note to a `roll` note of the same pitch: in pop songs that was wrong 2 times in 3.
- A `^` note: keep it at its slot. Write the lower note at that slot and the main note one slot later only when a parallel phrase has the lower note. A real short pickup a step below the next note is also joined and marked `^`: about 1 in 5 `^` notes is one or two slots early, and then the lower note was a real note.
- A `~` note: decide the slot with the parallel phrase. Without one, prefer the beat or the eighth position.
- A `+` or `-` note: choose between the written pitch and the semitone above (`+`) or below (`-`) with the chord of the bar and the parallel phrase.
- A `?` note an octave below the line is usually an octave error: write it an octave higher when a parallel phrase agrees. Some octave errors still have no `?`: read the line around each note.
- Note lengths were not tested. A length to the next onset is safer than a long held value.
- Notes with no gap between them can be one syllable (a melisma): keep the first note and the long ones.
- Write the syllables as the tune notes. A note shorter than 0.2 s that starts with no new syllable (no `'` at it in `--contour`) is usually a slide or a turn of the singer: in pop songs it was a real note in 4 of 10 cases (3 of 10 above 135 BPM). Write it as a grace note or leave it out, unless a `=` bar has it. A short note that starts a syllable was real in 6 of 10.
- `tune` starts a new note at each consonant inside one pitch (a dip in the level above 1 kHz); it finds about 6 of 10 repeated syllables on one pitch. A soft end or a breath gets `?`.
- Add a `?` note only when a parallel phrase has it. Many `?` notes and leaps of a fourth, fifth or octave in one phrase mean backing vocals: follow the lead voice.
- The line is in the sung octave. Put the tune where the piano carries it; `--octave 1` prints and saves it one octave higher.
- Copy `work/tune.json` into `song.json` and correct it there. `tune` writes over `tune.json` each time.
- `--contour` adds a line under each bar, `f0 | 0[D4 D4+] 5['. C#4-] 7[B3 A#3 'A3-]`: the pitch of the voice in each slot in 4 steps, as `tune` reads it (corrected for the tuning). `+` or `-` is 30 cents or more above or below, `'` a new syllable (a consonant), `.` no voice. Use it to see what `tune` did with a slide, a scoop, a held note or a repeated syllable before you change a note.
- `--fine` adds a row for each slot in 3 steps: `pitch level/bright`, in dB below the top of `lead.wav`. A dip in `bright` (the level above 1 kHz) is a consonant; a low `level` is a breath, a rest or a fading end. Use it for one or two bars. The two views show the input of `tune`; they were not tested on their own.

Find the form first (which bars repeat), then the bass note and chord of each bar, then the melody. Write what you found to `work/song.json` now, before the score. `compare` and `check` test the score against this file, so it must come from the recording and not from the score.

```json
{
  "melody":    {"4:3": "B4", "4:3.5": "B4", "5:1": "C5", "5:2": "B4"},
  "chords":    {"1": "E G B", "6": "B D# F# A", "20": "E G B D# F# A"},
  "parallel":  [[5, 14], [6, 15]],
  "free_bars": "1-4,21,22,39-43",
  "inner":     "14-21",
  "sections":  {"intro": "1-4", "verse 1": "5-12", "chorus 1": "13-20"}
}
```

`melody` is the tune as `bar:beat` (beat 1 is the first beat), in the octave where you will put it, in either hand. `free_bars` are bars where a hold or a free figure is intended (intro, ending, a rest bar). `inner` are the ranges where the tune is intended below the top line (an inner voice or the left hand): `check` does not list those notes as below another note. On the test song this removed 30 and 46 intended findings of two arrangements and changed no other finding. A range is `bar` or `bar:beat`, both ends included: `13:4.5-25,30`. `sections` gives the form: a name and one bar range for each section of the recording, each name once (`verse 2`, `chorus 2`); `check` then prints a line for each section.

To put a part of the tune in another octave than the sung one (a verse an octave below the chorus), run `$A2P octave work/song.json 29:3.5-38 1` (`-1` moves down). It moves the melody notes in the ranges and adds the move to `octaves` in the file. `compare` and `song` then test the sung pitch there: on the test song MELODY marked 54 notes weak without the record and 23 with it, the same as with a copy of the file in the sung octave. Do not change `octaves` by hand. A note that you move by hand is tested at its new pitch.

Then run `$A2P song work/song.json` before you write a note of the score. It tests the file against the recording and prints each finding with its count, as `check` does (all rows in `work/song.txt`):

- entries that are not valid: a bar that the grid does not have, a beat that is not on a sixteenth, a pitch or a chord tone that is not a pitch name;
- bars with no chord: `check` does not test their notes;
- melody notes outside the chord of their bar;
- melody notes where the voice sings another pitch (`2:2.75 D5<G4`); octaves do not count, so a tune moved with `--octave` passes. On 24 pop songs it marked 6 of 100 correct notes, 7 of 10 notes a semitone wrong and 9 of 10 notes a tone wrong;
- melody notes where the voice does not sing and no note model has the pitch within one slot (an instrument tune that `roll` does not show);
- melody notes that are weak in the recording at their onset: the MELODY test of `compare`, with its trust level (step 5).

On the test song it found the same notes as a hand check. Look at each finding with `tune --contour`, `roll` or `peek`, then correct the file or keep the note.

### 4. Write the score

Write `song.pno` with the `pno` skill: `@ key`, `@ time`, a tempo mark, `ped*`, then all bars. `pno fmt --fix` splits long notes into legal tied values.

To write parts in the slot form of `roll` and `tune`, do not write a generator. Write a draft with lines `BAR LANE slot:pitch/sixteenths ...` (lanes `rh`, `lh`, `rh2`, `lh2`) and all other lines in pno form, then run `$A2P cells draft.txt > song.pno` and `pno fmt --fix song.pno`. It writes each slot line as the bar cells of the grid and copies the other lines. Marks after the length (`!`, `x4`, model letters, `^`, `?`) are ignored; pno marks in the pitch stay (`0:F#6:v+4(/2`, `0:~A4/4`). Notes on one slot make a chord, a note ends where the next one starts, and a note longer than the bar goes on with a tie into the next bar of the lane, also a bar that the draft has no line of that lane for. Gaps are rests in `rh` and `lh`, one for each beat or part of a beat, and `_` in the other lanes. Write the lines of a bar together, in bar order. On the 42 bars of a test arrangement it gave the same score as the generator of the agent.

**Solo piano: transcribe.** Write what the pianist plays: the notes, voicing, register, figuration and hand split, with velocities that follow the velocities in `raw` (`pno spec 3.5` has the syntax). Do not add, remove or re-voice notes to make a better piece.

**All other recordings, or when the user asks for an arrangement: arrange.** Read `arranging.md` in this skill folder before you write a note. Keep the tune and the bass as detected, and change them only on evidence (`peek`, a parallel bar). Record each change to the tune, the bass or a chord: the report must list them.

**Mark slurs and articulations** in both cases. The export plays the marks (`pno spec 3.7` has the syntax).

- Slurs over the phrases of the tune. `--phrase` takes a slur of two bars or longer in `rh` as a phrase.
- Articulations: mark only what you can hear (`:st`, `:ten`, `:ac`). Do not mark each note.
- Do not write a `phrase` line or run `pno phrase` unless the user asks for manual phrasing. `--phrase` finds the phrases itself.

### 5. Compare with the recording

`$A2P compare song.pno --song work/song.json` exports the score through `pno` and prints three sections. Run it again after each group of edits: its output is not valid for a score that changed. All rows are in `work/compare.txt`.

| Section | What it prints | What it cannot tell you |
|---|---|---|
| HARMONY | Pitch classes of each beat, score against recording: the mean and the lowest bars. | Nothing about octaves or the melody. It changes little when the right hand changes. |
| NOTES | The count of score notes that no model has, and of strong piano-model notes that the score does not have. It lists only the notes that are there in no octave. It also counts the sung notes of `tune` as model notes. | It is circular: you wrote the score from these models. Use it to find invented notes and forgotten notes, not as proof. |
| MELODY | The tune notes of the song file that are weak in the recording at their onset, as `bar:beat pitch<louder pitch near it`. With `octaves` in the song file, it tests the sung pitch of those notes. Notes whose louder pitch is one octave away are one line of bar ranges: a tune in the wrong octave, or an octave doubling in the recording. | It is not proof. On 24 pop songs it marked 1 of 6 correct notes, 7 of 10 notes a semitone wrong and 9 of 10 notes a tone wrong. A low tune below loud harmony is marked more often. |

MELODY is the only independent test of the tune. Look at each marked note with `peek`, then change the note or keep it. Without `--song`, MELODY tests the top score notes from A4 and cannot see a tune below that. `--bars 9,18` adds the pitch-class profile of bars with a low HARMONY value, to see which note is too strong or absent. If a bar matches badly after you add accompaniment notes, an added note is not in the chord of the recording.

The score audio has the first tempo of the score. `compare` does not follow a later tempo mark in the score.

### 6. Check the piano writing

`$A2P check song.pno --song work/song.json` prints two classes. **Findings**, with their count: melody notes of the song file that are absent, below another note (not in the `inner` ranges), or not louder than the notes struck with them, and hands that overlap or cross. The line `[0]` names the findings with no rows. **Information**, lines that start with `-` and have no count: dead spots (1.5 s or more with no new note), left-hand leaps and right-hand jumps of more than an octave in less than 0.25 s. A count is not a target: do not change the music only to remove a row. With the melody of the song file it also prints the velocity range of the score; a range of one value means that no voice has a level. With `sections` in the song file, it first prints a line for each section: the new notes in a beat of each hand (the build from section to section), how many left-hand bars vary the bar before (and how many copy it exactly), the most common left-hand rhythm and in how many bars it is (one rhythm in most bars of many sections is one frame for the piece), the level of the mix in dB against the mean velocity of the score (a louder section needs a higher velocity), and for each stem with notes in the section, how many of its notes the score strikes (the pitch class within one slot: a layer that you kept has a high count, a layer that you left out a low one) and, in brackets, the beats of the bar where the score strikes fewer than half of them (a rhythm of the layer that the score does not have). The stem notes are rough (see step 2), so compare sections and runs, not single values. All rows are in `work/check.txt`.

`pno fmt` and `pno check` warn about a span of more than a tenth in one hand, except in a rolled chord (`:arp`, `pno spec` 3.8); roll a wide chord that the hand plays, or split it between the hands. The findings are places to look at, not rules. A crossing, a leap, a new figure or a tune below the top note is correct when you intended it; in a solo piano transcription, a finding that matches the recording is correct. Correct only what you did not intend. A melody note that is below another note or not louder than its accompaniment needs a velocity change: a `voicing.` level for the lane, `^m` on a tune note inside a chord, or a `vel.` level (arranging.md, "Make the tune heard").

`pno sound` reports pitches one octave too high under an `8va` line (the MIDI export is correct, and `check` removes the `8va` lane before it reads the score).

Then render (`pno render song.pno -o work/page.png`) and look at the pages.

### 7. Export, and each later pass

The user judges by ear and by eye, so each version must reach them as files. After the first version and after each later pass, also when the user did not ask again:

```
pno export song.pno song.mid song.musicxml song.pdf song.mp4 --sound piano.sfz --humanize --phrase --sheet top --colors RRGGBB,RRGGBB
```

- One command writes the four files: the MIDI to listen, the MusicXML for a notation program, the PDF to read and print (through MuseScore), and the video to watch. The MIDI and the video play the same performance.
- The video shows the notes falling onto a keyboard, with the audio of a sampled piano. It needs an SFZ piano: use the one that the user gave before; if you do not know one, ask the user one time for its path. Choose the two colours for the character of the recording, as the `pno` skill says (step 7). A video takes about half a minute for each minute of music.
- `pno export` prints `wrote` for each file, and names each file that it cannot write, for example one that a player has open. Export that file to a new name and tell the user.
- `--humanize` prints its seed; give it to the user (`--seed N` repeats the performance). Use `--phrase` with no amount: a larger amount is for phrase marks that the user wrote.

Before a pass, copy the score (`cp song.pno work/v1.pno`, then `v2`, ...), so that the user can go back. Run `check` on the copy and on the new version to get a before and after table. After the pass, `diff` the `rh` lines against the copy: a re-voicing pass can remove a melody correction from an earlier pass without any check failing. If the pass moves the tune, write a new `song.json` for the new version, and run `check` on the new version with the old file one time: it shows which melody notes moved.

### 8. Report

Give the user, in this order:

- the names of the exported MIDI, MusicXML, PDF and video files. You cannot hear the result; their ear is the last test;
- the tempo, meter, key, number of bars, which grid you used, and what the recording is (a solo piano, or an ensemble that you arranged, and for which difficulty);
- the HARMONY mean and the lowest bars, and the before and after table of `check`;
- each place where the tune, the bass or a chord differs from the recording, so that they can undo it;
- the notes you are least sure of.

## Limits

The numbers in this file come from rendered piano and ensemble clips, solo singing, two real songs and 24 pop songs (62 to 182 BPM) with known notes. Known failures:

- Slow, free piano playing: the notes were correct, but the tracked grid was not. The tracker changed between two beat levels.
- The beat level and the first beat of bar 1 are often wrong. The grid fields correct most of them; some need `shift`, and a few grids no field corrects.
- A sung tune (`tune`): a `^` note can be one or two slots early, and a note reached by a step up with no gap one or two slots late. A repeated syllable on one pitch with a soft consonant stays one note. Backing vocals that the lead-voice model keeps give octave errors.
- A synth bass can go to the `other` stem. Then the bass field and the `bass` line are poor, and nothing detects it: compare the bass field with the low `roll` notes.
- Drums can leak into a bass stem with a sparse bass part: then about half of the line is false (the bass field of the bar stays correct).
- Velocities come from `backing.wav`, so a song with a voice and drums has fewer `!` notes than before.
- Bars of another length: no false one in 93 songs with regular bars. `grid` does not find a section in another meter (it finds only the net move: give the section with `bar=A-B:n`), a 2-beat bar when the tracker marks half bars as downbeats, or a change when the tracker keeps its bar phase.
- Not tested: a pickup, swing, compound meters for `tune`, an upright bass, the lengths of `tune` notes.

Tell the user when a song is outside the tested range.
