# pno format specification

Version 1. Section numbers follow the implementation plan.

## 3.1 Example

Bars 0 to 3 of Chopin's Étude Op. 10 No. 3 ("Tristesse"), from a CC0 transcription in the PDMX dataset, without its playback data. The right hand has three voices and the left hand two. The slur that starts in bar 3 ends in bar 4, so the example is not a complete file:

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

3 rh.dyn | _ mf:above | fp:above _ _ mp:above |
3 rh.hp  | <:above >:above | _ >:above - _ |
3 rh     | ~G#4 A4 A4 G#4) | C#5:su( - - B4 |
3 rh2    | G#3 E4 B3 E4 | D#4 A4 B3 D#4 |
3 lh     | E2 B2 - B2 | B1 B2 - B2 |
3 lh2    | E2 | B1 |
```

The tests use bars 0 to 16 of the same transcription: `tests/fixtures/tristesse.musicxml`, imported as `tests/fixtures/tristesse.pno`. That copy has two changes:

- The metronome marks and the tempo changes after bar 0 are removed, because the transcription uses them only for playback.
- The hairpins are removed, because the transcription writes some hairpin ends before their starts.

## 3.2 Grid rules

| Item | Rule |
|---|---|
| Line | `<bar> <lane> \| cell \| cell \| ... \|` |
| Cell | One beat of the time signature: a quarter in 4/4, a dotted quarter in 6/8. Irregular meters declare their beat pattern, for example `time=7/8(2+2+3)` (section 3.6). |
| Slot | The slots in a cell share the beat equally. One whitespace token is one slot. A tuplet group, a tremolo group and a text in quotes each count as one token. A grace group has no space and is a part of its note token. |
| Slot count | 1, 2, 4, 8, 16, 32 or 64 in a plain cell; 1, 3, 6, 12, 24 or 48 in a dotted cell (section 3.6). Other counts are an error unless a tuplet mark is present. |
| Pitch | Letter, accidentals, octave: `Bb2`, `C#5`, `F##3`. Always absolute. |
| Chord | One token, pitches joined with `+`: `C4+E4+G4`. |
| `-` | Continues the same written note. The full run must be one written value, for example a dotted quarter (section 3.6). If not, `check` gives an error, so the writer must put the tie at the correct place. `pno fmt --fix` does this split by the standard rule. |
| `~` | Prefix on one pitch: this pitch starts a new written note that is tied to the same pitch before it. See "Ties" below. |
| `r` | Rest. `-` extends it to a longer written rest. |
| `R` | A rest for the full bar, as the only cell of its line (section 3.6). |
| `_` | Nothing in this voice or lane at this time. |
| `_` before a pitch | The pitch sounds and is not drawn (MusicXML `print-object="no"`): `_C5`, `C4+_C5`, `~_C5`, `{_D5}C5`. It takes each mark of a normal note. It has no accidental sign and does not change the alteration in effect. |
| `x` | Time that is not in the bar. See "Incomplete bars" below. |
| Tuplet | A bracket group with its number: `3[C4 D4 E4]`. See "Tuplet groups" below. |
| Marks on one pitch | Suffix `^name` directly after that pitch: `^f3` (finger 3), `^v-48` (velocity offset, section 3.5). In `E4^v-20+G4^v-20` each pitch has its own suffix. |
| Marks on the full token | Suffix `:name` at the end of the note or chord: `:ferm`, `:st` (staccato), `:ac` (accent), `:ten`, `:tr`, `:arp`, `:v+8` (velocity offset of the full chord). In `C4+E4+G4+C5:ferm` the fermata is on the chord. Section 3.7 has the full list. |
| Slur | `(` after the first note and `)` after the last note. Details: section 3.7. |
| Grace notes | Prefix on the main note, no slot: `{D5,E5}F5`. Details: section 3.7. |

### Ties (`~`)

- Write each tied pitch: `~A5`, `~A4+~C5`, or `~C5+E5` when only C5 is tied. There is no `~` without a pitch.
- A tie across a barline must use `~`. A `-` there is an error.
- `check` makes sure that the tie has a start. The same pitch must do one of these:
  - sound directly before it in this lane;
  - end at this time in a different lane (a part that changes its voice);
  - be in the grace group of this token;
  - have the mark `^t` earlier (section 3.7).
- In the first bar of a second volta, the bar before the first volta counts as the bar before.

### Incomplete bars (`x`)

- An incomplete bar has only the cells it uses.
- `x` fills the unused start of its first cell (a pickup, or the second part of a split bar) or the unused end of its last cell (the first part of a split bar).
- All lanes of the bar have the same `x` slots.
- The bar length comes from the `x` slots. There is no separate attribute.

### Tuplet groups

- One group is one written tuplet bracket. It takes the place of one slot, or of full cells when it has `|` inside: `3[C4 - D4 | - E4 -]` is one quarter-note triplet across two beats.
- The number of slots in the group is the tuplet number times a power of 2, so `3[C4 - D4 E4 F4 -]` is permitted.
- In a voice lane, a `-` directly after a group continues the group over that slot. When `-` is the only token of the cell, it continues the group over that cell: `| 4[F#3 A3 E3 E#3] | - | - |` is four notes in three beats.
- Use `|` in the group when each cell boundary is a slot boundary of the group. If not, use `-`.
- A group can be a slot of a group.
- Section 3.6 gives the ratio of each number. Section 3.3 gives the rules for a group in a lane that is not a voice lane.

## 3.3 Lanes

| Lane | Content |
|---|---|
| `rh`, `lh` | Voice 1 of the upper and lower staff. |
| `rh2`, `lh2` | More voices on the same staff, each with its own rhythm; up to `rh4` and `lh4`. `_` fills the time where the voice has nothing. When a staff has more than one voice lane in a bar, the stems follow the voice: up for the first and third, down for the second and fourth. This is the default, so these stems need no mark. |
| `ped` | Detailed pedal marks: `P` press, `C` change, `-` hold. See "Pedal" below. |
| `ped*` | The pedal rule: a `ped` line that repeats. See "Pedal" below. |
| `rh.dyn`, `lh.dyn` | Dynamic marks of that staff: `p`, `mf`, `sfz`. Default placement: below the staff. |
| `rh.dyn2`, `rh.hp2`, `rh.txt2` (and `lh`) | A second mark of that type at the same time on that staff; `rh.dyn3`, `rh.hp3`, `rh.txt3` a third. Present only in the bars that need them. |
| `rh.hp`, `lh.hp` | Hairpins of that staff: `<` or `>` at the start, `-` while it continues. Default placement: below the staff. |
| `rh.txt`, `lh.txt` | Text expressions of that staff. Default placement: above for `rh`, below for `lh`. |
| `tempo` | Tempo marks, above the upper staff. |
| `phrase` | Phrase marks of the two staves, for playback: `/` starts a phrase, `//` starts a section (a phrase of phrases), `breath` and `caesura` start a phrase and are drawn. Section 3.5 gives the details. |
| `rh.clef`, `lh.clef` | Clef changes, also in the middle of a bar. |
| `rh.8va`, `lh.8va` | Ottava lines. |
| `rh.trl`, `lh.trl` | Trill lines over several notes. |
| `@` | Attributes, not a grid: `key=`, `time=`, `barline=`, `volta=`, `break=` and others. Without a bar number they are the score header. Section 3.8 gives the list and the range of each attribute. |

- A token in the `dyn`, `hp`, `txt` and `tempo` lanes takes `:above` or `:below` when its placement differs from the default of its lane.
- A lane with no content in a bar is omitted.
- Section 3.7 gives the tokens of each lane.

### Tuplet groups in lanes that are not voice lanes

A tuplet group is permitted in each grid lane, for a mark that starts or ends inside a tuplet (for example a pedal mark). In a lane that is not a voice lane:

- A group is a time grid for one slot. It has no `|` inside.
- A `-` after the group continues the span: `rh.hp | 3[_ < -] | - | 3[- - _] | _ |` is a hairpin from the second triplet eighth of beat 1 to the third of beat 3.
- Canonical form: a plain cell if possible, then the smallest tuplet number, then the fewest slots.
- The import stops at a mark at a time that no group shows.

### Pedal

- `P` is a press, `C` a change and `-` a hold. The pedal ends where the `-` run ends.
- A new `P` during a hold is a stop and a new start.
- `C` when no pedal is held is the same as `P`. So the rule `ped* | C | - | - | - |` is correct from its first bar.
- `-` when no pedal is held is an error.
- A pedal that is down at the end of the file ends at the end of the last bar.
- Placement: below the lower staff. The import moves a pedal mark of the upper staff, or one above the lower staff, there.
- `@ pedal-style=line` or `sign` sets the look. With `pedal-style=sign`: `P` during a hold prints a release star and a new "Ped."; `C` prints a new "Ped." with no star. The import reads a "Ped." during a hold as `C`.

**The pedal rule (`ped*`):**

- A `ped*` line has the same cells and tokens as `ped`, so a rule can use each beat and slot: `| P | - | - | - |` is one pedal per bar, `| P | - | P | - |` is one per half bar, `| C | - | - | - |` is a change at each barline.
- It applies to its bar and to each bar after it, until the next `ped*` line or the next `time=` change.
- It does not apply to an incomplete bar (a bar with `x`) or to a bar with `cells=`.
- A `ped*` line with only `_` cells stops the rule.
- A `ped` line in a bar replaces the rule for that bar only.
- With no `ped*` line there is no pedal. A score with a pedal in each bar needs one line.
- The import finds the rule. In each range with one time signature, the most frequent `ped` line becomes the `ped*` line if more than half of the bars have it. The bars that differ get a `ped` line. A bar with no pedal gets a `ped` line with only `_` cells.

## 3.4 Presentation overrides

- **Beams:** `:bb` (beam begins) and `:be` (beam ends) put a beam at the place that the writer wants. `:b1` breaks the secondary beams after this note; the primary beam continues.
- **Stems:** `:up` and `:down`.
- **Direction of a curve or bracket:**
  - `:su` and `:sd` for the slur that starts on this token.
  - `^tu` and `^td` for the tie that starts on this pitch.
  - `3u[` and `3d[` for a tuplet bracket.
  - When a token starts two slurs, `:su` and `:sd` apply to the outer one, and `u` or `d` after the second `(` gives the direction of the inner one: `C5:su((d`.
- **Tuplet with no printed number and no bracket:** `3h[`.
- **Small notes** (a cadenza, a fioritura): `:small` on the token. Grace notes that are smaller than other grace notes have `:small` on the grace note: `{/F5:small}G5`. The notes sound. A cue note of MusicXML (`<cue/>`) is this.

### Accidental signs

- Display, directly after the pitch: `!` forces the sign, `?` puts it in parentheses.
- The pitch mark `^hide` hides a sign that the default rule gives: `F#4^hide`.
- Default rule: each staff position has an alteration in effect. At the start of a bar it is that of the key signature. Then it is that of the last note on this position in this bar. A note has a sign when its alteration differs from the one in effect.
  - The voices of one staff count together, and grace notes count.
  - A note that ends a tie has no sign and does not change the alteration in effect.
  - A cross-staff note counts on the staff of its lane.
  - Below an ottava line, the position is that of the written octave.
  - Two notes on one position at the same time have one sign.

## 3.5 Playback data

Playback data uses the same rule as presentation: the file stores a value only where it differs from what the notation implies.

### Velocity

The velocity of a note is the sum of four levels:

| Level | Syntax | Default (not stored) |
|---|---|---|
| The dynamic mark in effect | `@ vel.p=52` sets the velocity of the `p` marks: in the header for the full piece, or with a bar number from that bar until the next `vel.p=`. | The table below. |
| The voice | `4 @ vel.rh2=-15`: an offset for the lane `rh2`, from that bar until the next `vel.rh2=`. | 0 |
| The voicing | `30 @ voicing.rh=-12`: an offset for each note of a chord in the lane `rh` but its main note, from that bar until the next `voicing.rh=`. | 0 |
| The note | `:v+8` on the token: `E5:v+8`, `E5+A5:v-12`. When the pitches of a chord differ, `^v` on each pitch that has an offset: `C4+E4^v-20+G4^v-20`. | 0 |

**Level of the dynamic marks:**

- **Table of the dynamic marks:** `pppp` 16, `ppp` 26, `pp` 38, `p` 48, `mp` 58, `mf` 68, `f` 80, `ff` 92, `fff` 106, `ffff` 120. Before the first dynamic mark the level is 68. The values are a proposal.
- **Marks for one moment:** `sf`, `sfz`, `fz`, `rf` and `rfz` (92) and `sffz` (106) apply only to the notes in their slot; then the level before the mark applies again. `fp` (80) and `sfp` (92) apply to the notes in their slot; then the level is that of `p`.
- **Mark in effect:** a dynamic mark applies to the two staves. When the two `dyn` lanes have a mark in the same slot, each mark applies to its own staff.
- **No level change:** a mark in a `dyn2` or `dyn3` lane and a hairpin do not change the level. Note offsets give their sound.
- **Range of `vel.p=`:** the level of a note is the table value, at the bar of the note, of the mark that is in effect. So `9 @ vel.p=52` also changes a `p` that is in effect from an earlier bar.
- **Two marks with one name in one bar:** when a source has two marks with the same name and different velocities in one bar, the import keeps the velocity of the first mark for that bar. It gives the difference as note offsets and names the case in its report. The note velocities stay equal; the velocity of the second mark element does not.

**Voice and note offsets:**

- **Range of a voice offset:** `@ vel.rh=+10` in the header applies to the full piece. With a bar number it applies from that bar. `vel.rh2=0` ends an offset.
- **One form:** `:v` when all pitches of the token have the same offset (also for one note), `^v` if not. Never write the two on one token. A note offset of 0 is not written.
- **Voicing:** the main note of a chord is the pitch with `^m` (`E4+G4^m+C5`), else the top pitch in an upper-staff lane and the bottom pitch in a lower-staff lane. The other pitches of the chord get the voicing offset of the lane, and a pitch an octave or more from the main note with the same name gets half of it. A note alone has no voicing offset. A chord has one `^m` at most; it is an error on two pitches. Range and end as for `vel.rh=`.
- **Silent note:** `:v0` or `^v0`, with no sign, is the only absolute value: the note is drawn and does not sound (MusicXML `dynamics="0"`). Levels and offsets do not apply to it.
- **Unit:** MIDI velocity. A sum that is not in the range 1 to 127 is an error (a `v0` note has no sum).

**MusicXML:**

- **Conversion:** MusicXML stores a percentage of velocity 90, so `dynamics="35.56"` is velocity 32. The conversion is exact in both directions for each whole velocity. A source value that is not a whole velocity gives an offset with decimals.
- **What the export writes:** the velocity of each dynamic mark, and the velocity of a note only when its sum differs from the level of the mark in effect. It writes the `<miscellaneous>` field only when the file has `vel.` or `voicing.` lines.
- **MusicXML to `pno` and back** gives the same velocity for each mark and each note, because the sum is the same number. The import report lists what the format does not keep.
- **No velocity in the source:** a dynamic mark with no velocity has the table velocity, and a note with no velocity of its own has the level of that mark. The import report gives the count.
- **`pno` to MusicXML and back:** MusicXML has a velocity for each mark and each note, but no place for a voice offset. So `pno export` also writes the `vel.` and `voicing.` lines into the `<miscellaneous>` block of the MusicXML file, which is the standard place for the data of a program.
  - The import reads them and gives the same `vel.` and `voicing.` lines back. A note that changed in a different program gets a new note offset.
  - MusicXML has no place for `^m`: where `^m` is not on the default main note, the import gives `^v` offsets with the same sound.
  - Each line is a field with its own name (`pno-vel-1`, `pno-vel-2`, …), because MuseScore keeps only one field for each name.
  - MuseScore 4.7.6 keeps the fields when it converts a MusicXML file with its command line, and when it saves the file as `.mscz` first.
- **A file with no such block** (from a different program, or if a program removed the block; the levels cannot return then): the import uses a fixed rule.
  - A mark velocity that differs from the table becomes a `vel.` line at that bar.
  - If more than half of the notes of a lane have the same offset, that offset becomes the `vel.` line of the lane in its first bar.
  - The sound is exact. Only the division between the levels can differ from a file that a person wrote.

### Phrases

The `phrase` lane gives the phrases that `pno export --phrase` shapes: `5 phrase | _ / | _ |` starts a phrase on the second eighth of bar 5 in 2/4. Each mark starts a phrase. The phrase ends at the next mark or at the end of the score. The start of the score also starts a phrase.

| Mark | Starts | Drawn |
|---|---|---|
| `/` | A phrase | No |
| `//` | A phrase and a section | No |
| `breath` | A phrase | A breath mark after the note that ends at its time |
| `caesura` | A phrase | A caesura after the note that ends at its time |

- **Drawing:** a `breath` or `caesura` is drawn on the note that ends at its time, in the first voice lane in canonical order that has one. If no note ends there, it is an error. A mark at the start of a bar is drawn on the last note of the bar before.
- **Sound with `--phrase`:** each phrase has an arch of velocity (6 softer at its ends, 6 louder at 60 % of its length) and of tempo (1 % slower at its start, 1 % faster at its peak, 3 % slower at its end). A section adds half of its own arch. `--phrase=AMOUNT` multiplies the two arches (above 0, at most 4).
- **Sound with or without `--phrase`:** before a `breath` the last 32nd is held at half the tempo, and before a `caesura` the last 16th at a quarter of the tempo. The note before the mark ends where the hold starts. A fermata on a barline holds the last quarter of the bar at half the tempo.
- **No `phrase` line:** `--phrase` finds the phrases from slurs and bar groups. `pno phrase FILE` writes the phrases that it finds as `/` marks, and `//` where a section ends, to correct them.
- **Round trip:** the export writes `breath` and `caesura` as `<breath-mark/>` and `<caesura/>` in the `<articulations>` of the note. It writes `/` and `//` as `<miscellaneous>` fields `pno-phrase-1`, `pno-phrase-2`, … with the value `<bar index> <time in the bar> <mark>`, for example `4 1/8 /`. The bar index counts from 0, and the time includes the `x` time. The import reads both back.

### Other playback data

| Data | Syntax | Default (not stored) |
|---|---|---|
| Tempo in beats per minute | In the `tempo` lane: `"Andante"=72` | The number in the metronome mark, changed by the tempo words (below). |
| Attack and release offset of one note | Suffix on that pitch: `C4^a-20^r+15` | No offset: the note starts and ends where the notation says. |
| MIDI program, volume, pan | Header: `@ program=`, `@ volume=`, `@ pan=` | Piano, with the default volume and pan. |

- **Tempo words:** the export reads them in the `tempo` and `txt` lanes.
  - "rit.", "ritard.", "riten.", "rall.", "allarg." and "slentando" go to 75 % of the tempo over their length.
  - "accel." and "string." go to 120 %.
  - "poco" halves the change. "molto" makes it 1.6 times.
  - "a tempo" or a metronome mark ends the change.
  - "Tempo I" goes back to the first metronome mark.
- **Timing unit:** 1/480 of a quarter note, the usual MIDI resolution. A negative value is earlier. MusicXML gives `attack` and `release` in the divisions of each file, so a value that is not a whole number after conversion is stored with decimals.

## 3.6 Meter and bars

### Time signatures

| Lower number | Upper number | One cell | Cells per bar |
|---|---|---|---|
| 1, 2 or 4 | Each | One note of the lower number: a quarter in 3/4, a half in 2/2 | The upper number |
| 8, 16 or 32 | 3, 6, 9, 12, 18 or 24 | Three notes of the lower number: a dotted quarter in 6/8, a dotted eighth in 12/16 | The upper number divided by 3 |
| 8, 16 or 32 | 2 or 4 | Two notes of the lower number: a quarter in 4/8 | The upper number divided by 2 |
| 8, 16 or 32 | Each other number | A pattern is necessary: two notes for each 2 and three notes for each 3 | `time=7/8(2+2+3)` has 3 cells |

- **Symbols:** `time=c` is 4/4 with the common-time symbol. `time=cut` is 2/2 with the cut-time symbol. On another meter, `:c` or `:cut` after it shows that symbol and keeps the cells of the meter: `time=4/4:cut`.
- **Additive display:** `time=2+2+3/8` prints the pattern as the time signature. `time=7/8(2+2+3)` prints 7/8.
- **Import of an irregular meter with no pattern:** the pattern comes from the beams of the first bar in that meter. With no beams it is twos and then one three. The import report names the pattern.

### Divisions of a cell

- **Slot counts:** the row "Slot count" of section 3.2. Each other division is a tuplet, for example a duplet `2[C4 D4]` in 6/8.
- **Tuplet ratios:** in a plain span, `3[` is 3:2, `5[` is 5:4, `6[` is 6:4 and `7[` is 7:4. In a dotted span, `2[` is 2:3 and `4[` is 4:3. Each other case has the ratio in the group: `13:12[`, `7:6[`. A ratio that is equal to the default is not written. In `a:b[`, the span of the group divided by b must be one plain note value.
- **Length of a `-` run:** one plain note value, or one with one, two or three dots.

### Bars

- **A bar that is longer than its time signature** (a cadenza): `32 @ cells=15` gives this bar 15 cells of the current meter. The time signature does not change and nothing is printed. Each grid lane of the bar has 15 cells. When the bar ends inside a cell, the lanes end with `x` for the rest of that cell, as the first part of a split bar does: `32 @ cells=5` and `| … | E5 F5 G5 x x x x x |` in 4/4 is a bar of four quarters and three 32nds.
- **A bar that is shorter by full cells at its end** (for example the last bar after a pickup) also uses `cells=`.
- **Whole-bar rest:** a line with one cell, `5 lh | R |`, in each meter.
- **Voice lines:** each bar has an `rh` line and an `lh` line. A line `rh2`, `rh3`, `rh4` (and the same for `lh`) is present only in the bars where that voice has content.
- **Bar numbers:** the order of the bars in the file is authoritative.
  - The labels are 1, 2, 3 and so on. A pickup bar is 0.
  - The second part of a split bar has the number of the first part and the letter `b` (`8`, then `8b`).
  - `check` makes sure that the labels are correct, and `fmt --fix` writes them again.
  - The import numbers the bars by this rule. The export writes these numbers.
- **Voices of MusicXML:** on each staff, the voices become `rh`, `rh2`, `rh3`, `rh4` (or `lh` to `lh4`) in the order of their voice numbers. The export writes the voices 1 to 4 on the upper staff and 5 to 8 on the lower staff.

## 3.7 Marks, spans and lane tokens

### Marks on one pitch (`^`)

| Mark | Meaning |
|---|---|
| `^f3` | Finger 3. |
| `^lh`, `^rh` | The note is drawn on the other staff (cross-staff). The note stays in its voice lane, so the beam and the rhythm stay in one line. Also permitted on `r`. |
| `^t` | A tie starts on this pitch and ends on a later `~` of the same pitch in this lane, with other notes between: `C4^t E4 G4 ~C4`. Not necessary for a tie to the next note. |
| `^hide` | The accidental sign is hidden (section 3.4). |
| `^lv` | A laissez-vibrer tie: a tie with no end note. |
| `^gl`, `^sl` | A glissando from this pitch to the next chord of the lane: `^gl` is a wavy line, `^sl` a straight line. `^glt` and `^slt` add the text "gliss.". One glissando ends on the outer pitch of that chord in its direction: the top pitch when it goes up, the bottom pitch when it goes down. Several glissandi on one chord end on a chord with one pitch for each, from low to high. |
| `^v`, `^a`, `^r` | Playback data (section 3.5). The velocity offset of a full token is `:v`. |
| `^m` | The main note of the chord for the voicing offset (section 3.5). |

### Marks on the full token (`:`)

| Group | Marks |
|---|---|
| Articulation | `:st` staccato, `:stm` staccatissimo, `:ten` tenuto, `:ac` accent, `:marc` marcato, `:marcd` marcato with the point down. Portato is `:st:ten`. |
| Fermata | `:ferm`, and `:fermi` for the inverted sign below the staff. Also permitted on `r` and `R`. |
| Ornament | `:tr` trill, `:trl` trill with a wavy line for the full value of the note and of the notes tied to it (a longer line is `:tr` and a `trl` lane), `:mord` mordent (with the stroke), `:pral` inverted mordent (no stroke), `:lmord` and `:lpral` for the long forms, `:turn`, `:iturn` inverted turn, `:dturn` and `:diturn` for a turn between this note and the next. |
| Accidental on an ornament | The sign directly after the ornament name, with `n` for a natural: `:tr#`, `:trb`, `:trn`. A sign below the ornament is after `/`: `:turn/#`, `:turnb/n` (a flat above and a natural below). |
| Arpeggio | `:arp`, `:arpu` with an up arrow, `:arpd` with a down arrow. `:arp2` on the tokens of the two staves is one arpeggio line across both staves. `:arp` marks at one time in two lanes of one staff are one line. |
| Note size | `:small` (section 3.4). |
| Tremolo on one note | `:trem1`, `:trem2`, `:trem3`: the number of strokes. |

### Groups and spans in a voice lane

| Item | Rule |
|---|---|
| Slur | `(` after the first token and `)` after the last token. Slurs in one lane pair by nesting. A token can end one slur and start the next: `A5)(`. A token can start two slurs: `C5((`. Only a second or later `(` on a token can have a direction (`((u`, `((1d`); the first takes `:su` or `:sd` (section 3.4). A slur can start or end on a grace note: `{/D5(}C5)`. |
| Slur with a number | `(1` and `)1` name a slur. Use a number only for two slurs that overlap with no nesting, or for a slur that ends in a different lane. A number is unique among the slurs that are open at that time. A slur that ends at a time is not open for a slur that starts at that time. |
| Grace notes | A group with no slot, directly before its main note: `{D5,E5}F5`. `/` gives the slash: `{/D5}F5`. A grace note can be a chord: `{D5+F5}G5`. A group directly after a note is an after-grace: `D5{C5,D5}`. A grace note takes each pitch mark and each `:` mark of a normal note. The export writes an after-grace with `steal-time-previous`. The import writes an after-grace for that attribute, or when no note follows in the bar. |
| Value of a grace note | An eighth for one grace note and a sixteenth for more than one. A different value is a mark on that grace note: `{D5:4}F5` (a quarter), `:2`, `:8`, `:16`, `:32`, `:64`. |
| Tremolo between two notes | A bracket group with `t` and the number of strokes: `t3[C5+E5 G5+C6]`. As for a tuplet, the group takes one slot, or full cells with a bar sign inside. Each of the two notes is written with the value of the full group. For a group of more than two cells, `-` fills the slots and each note has half of them: `t3[C3 \| - \| G3 \| -]`. |

### Tokens of the other lanes

| Lane | Tokens |
|---|---|
| `rh.dyn`, `lh.dyn` | The standard dynamic marks: `pppp` to `ffff`, `mp`, `mf`, `sf`, `sfz`, `sffz`, `sfp`, `fp`, `fz`, `rf`, `rfz`. A mark can have a text before or after it: `p"dolce"`, `"più"f`. A text with no standard mark is a dynamic text (MusicXML `<other-dynamics>`): `"cresc."`, `"sempre pp"=33`. `=33` sets the level to velocity 33 from this slot, for the two staves, never for one moment only. With no `=`, the text changes no level. The text is not read: `"p"=70` is a text. Write a text about loudness in the `dyn` lane and each other text in the `txt` lane. |
| `rh.hp`, `lh.hp` | `<` crescendo, `>` diminuendo, `-` while the hairpin continues. |
| `rh.txt`, `lh.txt` | A text in quotes. `:box` after it draws a frame around the text (`"4":box`); `:mark` makes it a rehearsal mark (`"A":mark`). |
| `tempo` | `q=66` is a metronome mark; the note letters are `w`, `h`, `q`, `e`, `s`, and a dot makes the value dotted (`q.=44`). `"Andante"` is a text. `"Andante"q=72` shows the text and the mark. `"Andante"=72` shows only the text and plays 72 quarter notes per minute. `=72` shows nothing and changes the playback tempo. `_e=110` shows nothing and plays 110 eighth notes per minute; `=72` is the short form of `_q=72`. Parentheses show the mark in parentheses: `"Andante"(q=72)`. |
| `rh.clef`, `lh.clef` | `treble`, `bass`, `alto`, `tenor`, in the slot where the clef starts. `treble8va`, `treble8vb`, `bass8va` and `bass8vb` are the clefs with a small 8 above or below; the pitches stay the pitches that sound. With no token, `rh` starts with `treble` and `lh` with `bass`. A clef applies until the next clef token of that staff. |
| `rh.8va`, `lh.8va` | `8va`, `8vb`, `15ma`, `15mb` at the start, `-` while the line continues. Pitches are always the sounding pitches: an ottava line changes only how the notes are drawn. |
| `rh.trl`, `lh.trl` | `tr` where the wavy line starts, `-` while it continues. A note of that staff starts at `tr`; its trill sign is `:tr` on that note. The line ends at the end of its last `-`: the export puts its end on a note of that staff that ends there, in the lane of its first note if one does. |

- **Texts:** in each text, `\"` is a quote sign and `\n` is a line break. A bar sign in quotes is text.
- **Extension lines:** in the `txt` and `tempo` lanes, a `-` run after a text is its dashed extension line: `"cresc." - -`, `"rit." - -`.
- **Pedal tokens:** section 3.3.

### Spans across a barline

In the lanes `ped`, `hp`, `txt`, `tempo`, `8va` and `trl`, a `-` in the first slot of a bar continues the span of the bar before it. In a voice lane this is an error: a note that crosses a barline uses `~` (section 3.2).

## 3.8 Structure and header

Header lines have no bar number and one attribute each. A bar can have one `@` line with one or more attributes: `11 @ repeat=end volta=1`.

| Attribute | Values and meaning |
|---|---|
| `format=` | `1`. The first line of each file. |
| `title=`, `subtitle=`, `composer=`, `arranger=`, `lyricist=`, `copyright=` | A text in quotes. Header only. |
| `key=` | `F` (major), `Dm` (minor). When the source gives no mode: the number of signs, `1b`, `2#` or `0`. Applies from its bar. |
| `time=` | Section 3.6. Applies from its bar. |
| `barline=` | The barline at the end of this bar: `double`, `final`, `dashed`, `dotted`, `none`. `:ferm` adds a fermata on the barline: `barline=double:ferm`, or `barline=normal:ferm` for a normal barline. |
| `cells=` | The number of cells of this bar when it is longer than its time signature, or shorter by full cells at its end (section 3.6). This bar only. |
| `repeat=` | `start` (at the start of this bar), `end` (at its end), or `start,end`. `end:3` plays the range three times. |
| `volta=` | Each bar below a volta bracket has it: `volta=1`, `volta=1,2`. Adjacent bars with the same value are one bracket. The bracket end is closed when its last bar has `repeat=end`; if not, it is open. `:open` or `:closed` on the last bar overrides this. |
| `sign=` | `segno` or `coda`, at the start of this bar. |
| `jump=` | At the end of this bar: `dc`, `ds`, `dc-fine`, `ds-fine`, `dc-coda`, `ds-coda`, `tocoda`, `fine`. |
| `break=` | `system` or `page`: a new system or page starts with this bar. |
| `pedal-style=` | `line` or `sign`. Header only. |
| `staves=` | `1`: the score has only the upper staff, so it has no `lh` lanes; the `ped` lane is below that staff. With no `staves=`, a score has two staves. Header only. |
| `vel.p=`, `vel.mf=` and so on | The velocity of that dynamic mark (section 3.5). Header, or from its bar. |
| `vel.rh=`, `vel.rh2=`, `vel.lh=` and so on | The velocity offset of that voice lane, with a sign (section 3.5). From its bar. |
| `voicing.rh=`, `voicing.lh=` and so on | The offset of the chord notes but the main note in that voice lane, with a sign (section 3.5). From its bar. |
| `program=`, `volume=`, `pan=` | Playback (section 3.5). Header only. |

### Header texts from MusicXML

- Each field has one text, and the import keeps the first text that it finds.
- A movement title replaces a work title.
- A page text with no type that repeats a header text is dropped.
- Each other page text with no type goes to the field of its place on the first page:
  - lower half of the page: `copyright`;
  - right: `composer`;
  - left, or no `justify`: `lyricist`;
  - center: `title` for the texts with the largest font, `subtitle` for the others.
- The texts of one field are joined with `\n` in file order.
- A composer name at the left becomes `lyricist`, because the place decides, not the meaning.
- The report counts each text that is not kept.

## 3.9 Canonical form and errors

`fmt` writes this one form:

- **Line order in a bar:** `@`, `ped*`, `tempo`, the mark lanes of the upper staff (`rh.8va`, `rh.clef`, `rh.txt`, `rh.dyn`, `rh.hp`, `rh.trl`, each with its second and third lanes such as `rh.txt2` and `rh.txt3` directly after it), `rh`, `rh2` to `rh4`, `lh`, `lh2` to `lh4`, the mark lanes of the lower staff in the same order, `ped`. So the `rh` and `lh` lines are adjacent.
- **Spacing:** one empty line between bars. The bar number and the lane name are padded to one width for the file. One space is on each side of a bar sign and between tokens.
- **Fewest slots:** each cell has the smallest slot count that can show its content: `| E5 | - F5 |`, not `| E5 - | - F5 |`.
- **Order on one token:** the grace group, then for each pitch `~`, `_`, the pitch, `!` or `?`, and its `^` marks in alphabetical order; then the `:` marks in alphabetical order; then `)` and `(`; then an after-grace group.
- **No mark that is equal to its default.**

`check` gives an error for each of these cases:

| Lane | Error |
|---|---|
| All grid lanes | The number of cells is not the number of the time signature (not for a line with only `R`, or a bar with `x`; with `cells=` the number is that value). An invalid slot count. `x` slots that differ between the lanes of a bar. |
| Voice lanes | `-` in the first slot of a bar. `-` after `_` or `x`. A `-` run that is not one note value. `~` on a pitch with no tie start (section 3.2). A `^t` with no later `~` of that pitch. A `^gl` or `^sl` with no chord after it that fits. A numbered slur with no partner. An open slur at the end of the file. |
| Span lanes | `-` with no span to continue. A `tr` in a `trl` lane where the staff has no note. |
| `@` lines | An unknown attribute. A header-only attribute with a bar number. |
| File | A bar with no `rh` line or no `lh` line (with `staves=1`: no `rh` line, or an `lh` lane). A bar label that is not correct for its position. An unknown lane. |

## 3.10 Symbols

Each symbol has one meaning in its position, so the parser needs no context from other lines.

| Symbol | Meaning |
|---|---|
| Bar sign | Cell separator. |
| `@` | Attribute line. |
| `=` | Attribute value. Tempo value in the `tempo` lane. The level of a dynamic text in the `dyn` lane. |
| `"..."` | Text. |
| `#`, `b` | Sharp and flat in a pitch (`F#4`, `Bb2`), in a key with no mode (`key=2#`, `key=1b`), and after an ornament name (`:tr#`). After an ornament name, `n` is a natural. In a bar label, `b` is the second part of a split bar (`8b`). |
| `+` | Joins the pitches of a chord (a pitch follows). After `^a`, `^r`, `^v`, `:v`, `vel.rh=` or `voicing.rh=`: a positive offset (a digit follows). Joins the numbers of an additive time signature. |
| `-` | Voice lane: the note or rest continues, or the group before it continues. Span lane: the span continues. A part of some names (`dc-fine`, `pedal-style`). After `^a`, `^r`, `^v`, `:v`, `vel.rh=` or `voicing.rh=`: a negative offset. |
| `~` | Prefix of a tied pitch. |
| `r`, `R` | Rest, whole-bar rest. |
| `_` | Nothing at this time. Before a pitch: the note is not drawn. Before a metronome mark in the `tempo` lane: the mark is not shown (`_e=110`). |
| `x` | Time that is not in the bar. |
| `^` | A mark on one pitch. |
| `:` | A mark on the full token, or a placement in a mark lane (`:above`). In a tuplet number: the ratio (`13:12[`). In an attribute value: an option of the value (`end:3`, `double:ferm`, `1:open`). |
| `!`, `?` | The accidental sign is forced, or in parentheses. |
| `(`, `)` | Slur start and end; a digit after it is the slur number, and `u` or `d` after a later `(` on a token is its direction. In a `time=` value: the beat pattern. In the `tempo` lane: a metronome mark in parentheses. |
| `{`, `}`, `,` | Grace group and its separator. Also the separator in `repeat=start,end` and `volta=1,2`. |
| `/` | In a grace group: the slash. In `time=`: the fraction. After an ornament name: the sign below it. |
| `[`, `]` | A tuplet group (`3[`) or a tremolo group (`t3[`). A letter between the number and `[` is `u` (bracket up), `d` (bracket down) or `h` (no number). |
| `\` | In a text: the next character is a quote sign (`\"`) or a line break (`\n`). |
| `<`, `>` | Hairpins. |
| `P`, `C` | Pedal press and change. |
| `*` | A lane line that repeats (`ped*`). |
| `.` | Separates the staff and the type in a lane name (`rh.dyn`), and the parts of an attribute name (`vel.p`). A dotted value in a metronome mark (`q.=44`). The decimal point of an offset. |

## 3.11 More examples

Each example is 4/4 if no `time=` line is given. An example shows only the lines of its case. The test fixtures in `tests/fixtures/spec/` add the missing `rh` or `lh` lines.

```
A tuplet in one beat, and a tuplet across two beats:
1  rh     | 3[C5 D5 E5] | F5 | 3[G5 - A5] | G5 |
2  rh     | 3[C5 - D5 | - E5 -] | F5 | G5 |

Grace notes: with a slash, a group of two, an after-grace:
3  rh     | {/D5}C5 | {E5,D5}C5 - B4 C5 | D5{C5,D5} | E5 |

Two voices in one hand; the second voice starts on beat 3 of bar 5:
4  rh     | E5 | - | D5 | - |
4  rh2    | G4 A4 | G4 F4 | E4 | F4 |
5  rh     | C5 | D5 | E5 | F5 |
5  rh2    | _ | _ | G4 | - |

A partial tie in a chord, a whole-bar rest:
6  rh     | C5+E5 | ~C5+F5 | ~C5+~F5 | r |
6  lh     | R |

A clef change in the middle of a bar:
7  lh      | C3 G3 | E4 G4 | C5 E5 | G5 |
7  lh.clef | _ | _ | treble | _ |

Cross-staff notes: the left hand goes up to the upper staff:
9  rh     | R |
9  lh     | C3 G3 | C4^rh E4^rh | G4^rh E4^rh | C4^rh G3 |

A repeat with two voltas:
10 @      repeat=start
10 rh     | C5 | D5 | E5 | F5 |
11 @      repeat=end volta=1
11 rh     | G5 | - | - | - |
12 @      barline=final volta=2
12 rh     | C5 | - | - | - |

An ottava line (the pitches are the sounding pitches):
13 rh.8va | 8va | - | - | - |
13 rh     | C7 D7 | E7 F7 | G7 | - |

Dynamic marks and a hairpin that starts on the second eighth:
14 rh.dyn | p | _ | _ | f |
14 rh.hp  | _ < | - | - | _ |
14 rh     | C5 D5 | E5 F5 | G5 A5 | B5 |

A 6/8 bar and a 7/8 bar:
15 @      time=6/8
15 rh     | C5 D5 E5 | F5 - E5 |
15 lh     | C3 | G3 |
16 @      time=7/8(2+2+3)
16 rh     | C5 D5 | E5 F5 | G5 A5 B5 |
16 lh     | C3 | G3 | C4 - G3 |

Slurs: two slurs, where the second starts on the end note of the first:
17 rh     | C5( D5 | E5 F5)( | G5 A5 | B5) r |

A text with an extension line, then a text with a metronome mark:
18 tempo  | "rit." | - | - | - |
19 tempo  | "a tempo"q=72 | _ | _ | _ |

A tremolo between two chords, each written as a half note:
20 rh     | t3[C5+E5 | G5+C6] | r | - |

A pedal rule, and one bar with a different pedal:
1  ped*   | P | - | - | - |
7  ped    | P | - | C | - |

A cadenza bar of 6 cells in 4/4, with small notes and a fermata on a rest:
21 @      cells=6
21 rh     | C5:ferm | D5:small E5:small F5:small G5:small | A5:small B5:small C6:small D6:small | E6 | - | r:ferm |

A slur that starts on a grace note; grace notes that are tied into their chord:
22 rh     | {/D5(}C5) | {C4,E4}~C4+~E4+G4 | r | - |

Four notes in three beats, and triplets with no printed number:
23 @      time=3/4
23 rh     | 4[F#4 A4 E4 E#4] | - | - |
24 rh     | 3h[C5 D5 E5] | 3h[F5 G5 A5] | B5 |

A tie with other notes between its start and its end:
25 rh     | C4^t E4 | G4 ~C4 | r | - |

A dynamic mark with text, a trill with a flat, a turn between two notes:
26 rh.dyn | p"dolce" | _ | _ | _ |
26 rh     | E5:trb | - | D5:dturn | C5 |
```

## 3.12 Not in version 1

The list of marks and tokens in section 3 is closed. For each other element the import stops and names it. Known cases:

- multi-bar rests;
- lyrics;
- more than four voices on a staff;
- more than two staves;
- a glissando that does not end on the next chord of its voice as `^gl` places it;
- half pedal and other pedal types (sostenuto and una corda are possible as text);
- finger substitution;
- notehead shapes and parentheses on a notehead;
- a notehead fill that is not the fill of its note value;
- key signatures that are not standard;
- metric modulation marks;
- a metronome mark with no number;
- an accidental sign in square brackets;
- a tuplet across a barline;
- a clef with an octave number other than 1 or -1, or on a C clef;
- a bracket for chords that are not arpeggios;
- a note shorter than a 256th;
- dots that are not shown;
- ornaments with an approach or departure note;
- a playback dynamic level with no mark;
- a dynamic text next to a dynamic mark in one element;
- a mark at a time that no slot or group shows;
- a text with a frame that is not a rectangle;
- a note or rest at a time that no slot or group shows;
- pedal lines that overlap;
- a tie to a later note in another voice;
- a tie into another voice with two possible start notes;
- a tuplet whose rounded lengths do not add up;
- a tuplet with the number 1 (a tuplet mark on notes with no time modification);
- two trill lines at one time on one staff, or a trill line end with no start.
