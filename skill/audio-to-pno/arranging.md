# Arranging a recording for piano

Read this before step 4 of `SKILL.md` when the recording is not solo piano, or when the user asked you to arrange a solo piano recording.

You are the arranger. The aim is a piece that a pianist would want to play and that a listener knows as the song.

**True to the song, written for the piano.** The recording gives what is played: the tune (pitches and rhythm) and the harmony (the bass note and chord of each bar). You decide how the piano plays it: texture, register, figuration, which hand plays what, how loud each voice is. Each bar must be possible for two hands at the tempo: what ten fingers can reach and hold, with time to move to the next position. Each bar must also sound like piano writing, not like the detected notes copied: a strummed guitar becomes a broken chord or a figure, a held pad becomes a chord held with the pedal. When the recording and the hands disagree, keep the tune and the bass, and make the rest simpler.

Take accompaniment notes from the chord of the bar, and prefer the chord notes that the models found. Colour over the bass is free (sevenths, ninths, suspensions), but do not change the chords themselves. `check` and HARMONY mark colour notes as outside the chord; that is correct when you intended them.

## Make the tune heard

Without offsets, each note has the velocity of the dynamic mark and the result is flat. Give each voice a level, then shape the notes (`pno spec 3.5` has the rules; the sum for a note must be 1 to 127):

- a level for a lane: `@ vel.lh=-14` in the header, or `23 @ vel.lh=-6 vel.rh2=-12` from a bar. About -6 to -12 is enough for balance. Do not move a lane far at the bar where the dynamic mark also changes: the two steps add;
- one note or chord: `E5:v+8`; one pitch of a chord: `C4^v-18+E4^v-18+G5` (soft harmony below a full top note);
- a tune in the left hand: make that lane louder and the right-hand figuration softer for those bars, then set them back;
- phrases: a hairpin is only drawn and does not change the sound, so shape a crescendo, the top of a phrase and its end with note offsets. Change the level of a mark with `@ vel.mf=84`.

## Give the piece a shape

Let the texture follow the form and the loudness of the recording. The models give a different shape in each bar; that is detection noise, not a reason to change the texture.

## Give each section one left-hand figure

There are two failures. One figure for the full piece: only the chord changes, and at the climax the bass only gets an octave, so the listener hears a loop. A new figure in each phrase: the texture never settles, and the piece loses its character. No figure is wrong; the amount of change is.

- A section is a large part of the form, such as a verse or a chorus, usually 8 bars or more. A phrase of 2 to 4 bars is not a section, and the second half of a section is not a new section.
- Keep one figure for the full section. In a section, only vary it: a fill, a held chord or a walk at the end of a phrase, where the tune rests. A variation keeps the rhythm or the contour of the figure. A different figure in the middle of a section, such as a waltz bass after broken chords, is not a variation.
- At a section boundary, change the rhythm or the contour. A new register or an added octave alone is not a change.
- When a section returns, its left hand can repeat, become a variation, or become a new figure. A new figure needs a musical reason, for example more weight in a louder return.
- Take the rhythm from the recording's bass.
- Use an inversion only where the recording has one.
- `check` lists the left-hand rhythms with a count of bars for each. One rhythm in most bars of the piece is a loop. Many rhythms in one section is too much change.

## Playable, not timid

Match the difficulty to what the user asked for. If they did not say, match the original: its tempo, its density of notes and its energy. Do not make the texture thin out of caution. Name the difficulty that you chose in the report.
