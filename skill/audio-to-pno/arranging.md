# Arranging a recording for piano

Read this before step 4 of `SKILL.md` when the recording is not solo piano, or when the user asked you to arrange a solo piano recording.

You are the arranger. The aim is a piece that a pianist would want to play and that a listener knows as the song.

If `style-guide.md` is in this skill folder, read it too. It holds the user's own arranging style: default choices with counts from arrangements that they like. Use its defaults where this file leaves the choice to you. This file and what the user asks stay the rules.

**True to the song.** The recording gives what is played: the tune (pitches and rhythm), the harmony (the bass note and chord of each bar) and the lines of each instrument. You decide how the piano plays them: which hand and register each layer gets, its figuration, and how loud it is. Each bar must also sound like piano writing, not like the detected notes copied: a strummed guitar becomes a broken chord or a figure, a held pad becomes notes held with the pedal, a drum pattern becomes accents and rhythm in the other layers. Each bar must be possible for two hands at the tempo: what ten fingers can reach and hold, with time to move to the next position.

Take accompaniment notes from the chord of the bar, and prefer the chord notes that the models found. Colour over the bass is free (sevenths, ninths, suspensions), but do not change the chords themselves. HARMONY marks colour notes as outside the chord; that is correct when you intended them.

## Hear the layers of each section

Before you write a section, list its layers from the evidence:

- `levels` gives each stem by bar: which instruments play in the section, and how loud.
- `roll`, `raw` (with `--stem guitar`, `piano`, `strings`, `brass`, `woodwinds`, `organ` or `synth`) and `peek --stem` (these, and bass, other, lead) show what each instrument plays: the bass line, a riff, a figure, a counter-line, a held pad.
- `grid` (the backbeat line) and the drum stem show the rhythm: where the kick and the snare are, and where the drums push.

Write the list in a short plan for each section: layer, what it plays, its piano voice. A riff or a counter-line that the listener knows is part of the song, the same as the tune.

## Give each layer a voice

Decide which layers the piano keeps. How many depends on the difficulty and the style that the user wants (`style-guide.md` can set them). Each layer that you keep needs a clear voice.

| Layer of the recording | Usual piano voice |
|---|---|
| voice | the tune: on top, or in the middle |
| bass guitar | the low line; octaves when it is strong |
| drums | accents and rhythm: chord stabs on the snare, a bass note on the kick, a driving figure in the pushes |
| guitar or keys | a figure or chords in the middle register |
| strings, synth, pads | held notes, counter-lines, riffs between the phrases of the tune |

- Put each layer in a lane of its own (`rh`, `rh2`, `lh`, `lh2`), so that it can have its own level (`@ vel.rh2=-12`).
- **Order of importance**: the tune, the bass, the riff or line of the song, the rhythm, the pads. When the hands are full, leave out the last layer first, or let two layers share one voice: a figure that has the chord notes of the pad, a bass in octaves that also gives the kick.
- **Keep the layers apart.** Each layer has its own register. Below about C3, use only wide intervals (octaves, fifths, tenths); close chords belong in the middle and above. Two layers in the same register at the same time blur. A layer that does not move can be held or left out while another layer moves.
- **Listen where the hands meet.** When the top of the left hand reaches the register of the right hand, a note of one hand can rub against a note of the other (a second, a seventh, a ninth), and the pedal keeps it ringing. A rub can be colour; make sure that each one is meant. `pno analyze` lists them.
- **Each layer stays alive.** A layer that only fills the chord, in the same rhythm in each bar, is not a voice. Give each layer the rhythm and contour of its instrument, and let the layers answer each other: one moves while another holds.

## Make the tune heard

Without offsets, each note has the velocity of the dynamic mark and the result is flat. Give each voice a level, then shape the notes (`pno spec 3.5` has the rules; the sum for a note must be 1 to 127):

- a level for a lane: `@ vel.lh=-14` in the header, or `23 @ vel.lh=-6 vel.rh2=-12` from a bar. About -6 to -12 is enough for balance. Do not move a lane far at the bar where the dynamic mark also changes: the two steps add;
- the notes of a chord: `@ voicing.rh=-12` (in the header, or from a bar, for a section) makes each chord note softer than its main note: the top note in the right hand, the bottom note in the left hand, or the pitch marked `^m` (`E4+G4^m+C5`, a tune in an inner voice). An octave copy of the main note gets half. Do not write `^v` on each chord pitch for this;
- one note or chord: `E5:v+8`; one pitch of a chord, for an exception: `C4^v-6+E4+G5`;
- a tune in the left hand: make that lane louder and the right-hand figuration softer for those bars, then set them back;
- phrases: a hairpin is only drawn and does not change the sound, so shape a crescendo, the top of a phrase and its end with note offsets. Change the level of a mark with `@ vel.mf=84`.

## Repeated notes of the tune

A singer repeats a pitch because each syllable needs a note. On the piano, a fast repeat often stutters. Decide by the note value between the two notes:

- A sixteenth apart or less: usually one note. Count in the note values of the meter, not in seconds: at a fast tempo an eighth is shorter than 0.2 s and is still struck.
- An eighth apart or more: strike each note.
- The third note or later of a run on one pitch: strike it; the run is the rhythm of the tune.

Also make two notes one when they are one sung note that the models split in two (the voice has no new syllable at the second note; check with `peek --stem lead`), or when the second note is very short and soft (a quick lead-in or an ornament of the singer).

Choose the note to keep by the music, not by its order: keep the note on the stronger beat, or the one that is struck with a chord or a left-hand note. After a merge, the tune notes and the song file differ: record each merge in the report like any other change to the tune.

## Give the piece a shape

Let the texture follow the form, the loudness and the number of instruments of the recording (`levels`). A section where the band enters gets more layers; a section with voice and one instrument gets fewer, but each of its layers still has its own life. The models give a different shape in each bar; that is detection noise, not a reason to change the texture.

## Give the left hand a part of its own

The left hand is not only the chords under the tune. It can carry the low layers of the band: the bass, the kick, the lower half of the chords and figures. A left hand that strikes the same bar through a whole section, with only the chords changing, sounds like a loop, also when each bar is correct. A left hand that brings a new idea in each bar sounds random. Aim between the two: develop an idea while the music that it carries goes on.

- **Frame.** A section (a large part of the form, such as a verse or a chorus, usually 8 bars or more) has one frame: broken chords, bass and chords, a held bass with a moving voice, octaves, a line. Change the frame or its rhythm at a section boundary; a new register or an added octave alone is not a change. Use 3 or more frames in a piece.
- **Develop an idea, do not replace it.** A left-hand idea lasts as long as the music it carries: while the groove, the chords and the phrase of the recording go on, keep the idea and vary it (a changed note, a fill at the end of a phrase, the shape moved to the new chord). Change the idea where the music changes: a new phrase, a new section, a new layer in the recording. Where the recording keeps a steady pulse, such as a driving bass, keep that pulse. The same music in the recording gets the same idea or a variant of it.
- **Answer the tune.** Move where the tune holds a note or rests: a fill, an answer, a run into the next phrase. Put many left-hand notes between the tune notes, not only with them. Where the tune moves fast, keep the left hand simple.
- **Write lines, not only chords.** Let the bass walk by step into the next chord, with passing notes. Give the left hand a second voice above a held bass note. Use the lines of the recording: the bass guitar, a riff, a counter-line of strings or guitar (`roll`, `raw` and `peek --stem` show them).
- **Give it a spotlight.** At least once in a piece, the left hand leads: a run, a sweep over two octaves or more, a riff of the song, or the tune itself below a right-hand figure (mark those bars with `inner` in the song file).
- **Use the keyboard.** Move the left-hand register from section to section, and let a big section go low and wide. A left hand that stays inside two octaves for the full piece sounds small.
- **Bass.** Keep the bass note of the recording at the harmonic points (the first beat, and each chord change). Between them, passing and walking notes are free. Use an inversion as a harmonic point only where the recording has one. Start from the rhythm of the recording's bass, then vary it.
- **When a section returns,** its left hand can repeat, become a variation, or become a new frame. A new frame needs a musical reason, for example more weight in a louder return.
- The section lines of `check` give the most common left-hand rhythm and in how many bars it is. One rhythm in most bars of many sections is one frame for the piece; a new figure in each bar sounds random. It is information, not a target: between them, choose by the music.

## Playable, not timid

Match the difficulty to what the user asked for. If they did not say, match the original: its tempo, its density of notes and its energy. Do not make the texture thin out of caution. Name the difficulty that you chose in the report.
