# The audio-to-pno skill and `pno a2p`

The `audio-to-pno` skill helps an agent write a piano score of a recording. It does not convert audio to a score in one step. The models make many errors, so the tool gives evidence and checks, and the agent decides each bar. The workflow for the agent is in [`skill/audio-to-pno/SKILL.md`](../skill/audio-to-pno/SKILL.md). The rules for a piano arrangement are in [`skill/audio-to-pno/arranging.md`](../skill/audio-to-pno/arranging.md).

## Files

| File | Role |
|---|---|
| `skill/audio-to-pno/scripts/a2p.py` | The evidence tool: one Python file with all commands |
| `skill/audio-to-pno/scripts/setup_env.py` | Makes the Python environment and downloads the models |
| `skill/audio-to-pno/requirements.txt` | The pinned Python packages |
| `src/a2p.rs` | The `pno a2p` command: runs `a2p.py` in its environment |

## `pno a2p`

The three files above are built into the `pno` binary (`include_str!`). On each run, `pno a2p`:

1. Writes them to `~/.cache/audio-to-pno/pno/`, if they changed.
2. Runs `setup_env.py` if the venv `~/.cache/audio-to-pno/venv` is missing, or if `setup_env.py` or `requirements.txt` changed since the last setup. The stamp file `setup.txt` holds the text of both. The Python for the setup is `python` from the `PATH`, or `PNO_PYTHON`.
3. Runs `a2p.py` with the venv Python and the given arguments, and exits with its exit code.

The setup output goes to stderr, so the output of `a2p.py` stays clean. A new `pno` with changed scripts writes them on its first run.

### The environment

- `setup_env.py` installs the CUDA build of torch when `nvidia-smi` is on the `PATH`. Then the models of `prepare` run on the GPU.
- It downloads the piano transcription model, the beat tracker model and the chord model. The lead-voice model and the models of step `parts` (1.2 GB) download on the first `prepare`. `a2p.py` fetches the two models that are not in the list of audio-separator itself (`fetch`, from Hugging Face at a fixed commit) into `~/.cache/audio-to-pno/separator`, and joins the four Mega 53 files into one model file (`mega53_heads.ckpt`).
- About 1.3 GB, or 3.8 GB with CUDA. ffmpeg must be installed.

## The work folder

`-w <folder>` holds the files of one song. Each command reads and writes there.

| File | Written by | Content |
|---|---|---|
| `src.wav` | `prepare` | The recording, converted by ffmpeg |
| `drums.wav`, `bass.wav`, `other4.wav`, `vocals.wav` | step `stems` | demucs stems (`other4.wav` is the demucs `other`; `grid` reads it) |
| `backing.wav` | step `stems` | bass + demucs other: the input of the note and chord models |
| `beats.json` | step `beats` | beat_this beats and downbeats |
| `lead.wav` | step `lead` | The lead voice without backing vocals (karaoke model) |
| `tk.json`, `pt.json`, `bp.json` | steps `tk`, `pt`, `bp` | Notes of Transkun, piano_transcription_inference and basic-pitch |
| `bass.json`, `bp_bass.json` | step `bass` | basic-pitch notes of the bass stem |
| `chords.npy` | step `chords` | BTC chord probabilities for each frame |
| `guitar.wav`, `piano.wav`, `other.wav` | step `parts` | Guitar, piano and other stems of a 6-stem model (BS-RoFormer SW). In a folder from before, `step parts` first renames the demucs `other.wav` to `other4.wav` |
| `strings.wav` | step `parts` | The bowed strings stem of `src.wav` (gilliaan bowed strings v2) |
| `brass.wav`, `woodwinds.wav`, `organ.wav`, `synth.wav` | step `parts` | Four heads of Mega 53 in one pass on `src.wav` |
| `tk_piano.json`, `bp_<stem>.json` | step `parts` | Transkun notes of `piano.wav` and basic-pitch notes of `guitar.wav` and the five layer stems, where the stem is above -40 dB |
| `grid.json` | `grid --set` or `grid --constant` | The bar grid that all later commands read: beats in a bar, quarters in a beat, the beat times, and `bars` (`{"57": 2}`) for the bars of another length, if there are any |
| `tune.json` | `tune` | Draft melody without doubtful notes |
| `compare.txt`, `check.txt`, `song.txt` | `compare`, `check`, `song` | All rows of their lists |
| `cqt.npy`, `cqt_<stem>.npy`, `f0_lead.npy` | first use | Caches; `prepare` deletes them |

## Commands

| Command | What it does |
|---|---|
| `prepare AUDIO` | Converts the audio, then runs the steps: `stems` and `beats` first, then `tk`, `pt`, `bp`, `bass`, `lead`, `chords` and `parts` at the same time. Each step is a separate process with its own numba cache. At the end it prints when each stem plays, the fade-out and the tuning. |
| `step NAME` | Runs one step again, for example for a folder made before that step existed |
| `grid` | Fits a bar grid to the tracked beats and the note onsets, and compares it with a constant grid. It finds bars of another length where the tracked downbeats move to another beat (`phase_path`, `odd_bars`), and places each with the chord changes. `--set FIELD=VALUE` corrects and saves it (`bar=N:beats` gives the bars of another length). With a drum stem it also fits a constant grid to the drum hits (`--constant` saves it when it fits better), counts kick and snare hits on each beat of the bar, and prints the drift of each grid. |
| `roll [BARS]` | Each bar: the chord, the bass note and the model notes, merged across the models. Letters show which models have a note. `--beats` adds the chord of each beat. `--stem piano`, `guitar`, `strings`, `brass`, `woodwinds`, `organ` or `synth` shows all notes of one stem instead. |
| `raw [BARS]` | The model notes with exact positions and velocities, not merged; `--stem` as in `roll` |
| `bass [BARS]` | The bass line of the bass stem, in the format of `tune` |
| `tune [BARS]` | The sung tune of `lead.wav`, with marks for scoops, late onsets, notes between two semitones and doubtful notes, plus parallel phrases. `--contour` and `--fine` show the sung pitch inside each slot. |
| `peek BARS PITCHES` | The loudness of given pitches through time, in the mix or in one stem (`--stem`) |
| `levels [BARS]` | When each stem plays, the tuning, and the level of each stem and of the mix in each bar |
| `song SONG` | Checks `song.json` against the recording before a score exists: entries, chords, sung pitch, weak notes |
| `octave SONG RANGES N` | Moves the melody of `song.json` in the ranges by N octaves and records it in `octaves`, so that `compare` and `song` test the sung pitch |
| `cells DRAFT` | Prints a draft as pno: lines `BAR LANE slot:pitch/sixteenths ...` become bar cells of the grid, other lines stay |
| `compare SCORE` | Exports the score with `pno` and compares it with the recording: harmony, notes, melody |
| `check SCORE` | Piano-idiom checks of a score: findings with counts (melody balance, crossed hands) and information without counts (dead spots, leaps and jumps in seconds, the section lines) |

## How `a2p.py` is organized

The file has one section for each area, marked with `# ----` lines:

| Section | Main functions |
|---|---|
| (top) | Constants with the reason for each value, `Grid` (the bar grid of a song: all bar positions come from its `odd` bars) and `saved_grid`, `combined` and `viewed` (model notes), `in_ranges` and `octaves_from_sung` (ranges of `song.json`) |
| (top, after the constants) | `levels` (stem levels in dB), `stem_onsets` (spectral-flux onsets with kick and snare bands) |
| prepare | `run_stems`, `run_beat_tracker`, `run_lead`, `run_transkun`, `run_pt`, `run_basic_pitch`, `run_bass`, `run_parts` (`separate`, `fetch`, `strings_config`, `mega_model`), `audible_notes`, `run_chords` (`btc_logits` is the BTC forward pass), `cmd_prepare`, `stem_report`, `tuning` |
| sung tune | `vocal_f0` (pYIN on `lead.wav`), `semitone_path` (Viterbi), `sung_notes` (syllable cuts, scoop joins, loud starts, soft notes), `octave_errors`, `on_grid`, `parallel_bars`, `contour`, `fine_rows`, `cmd_tune` |
| bass line | `bass_line`, `cmd_bass` |
| grid | `repair`, `cover`, `snap`, `phase_path`, `bar_phase`, `line_evidence`, `odd_bars`, `tracked_grid`, `drum_fit`, `drift`, `backbeat`, `chord_change`, `cmd_grid` |
| evidence | `merged`, `bar_bass`, `chord_choices`, `chord`, `beat_chords`, `bar_chord`, `cmd_roll`, `cmd_levels`, `cmd_raw`, `cmd_peek` |
| compare | `score_notes`, `tune_level`, `cmd_compare` |
| song | `cmd_song`, `cmd_octave` |
| cells | `cmd_cells`, `slot_cells` |
| check | `bar_cells` (the cells of each bar, with meter changes and incomplete bars), `onsets` (reads `pno sound --velocity`), `lh_pattern`, `is_variant` (the left-hand bars), `section_lines`, `cmd_check`, `print_findings` |
| command line | `main`: one subcommand for each `cmd_` function |

The constants are tuned values. Each has a comment with the reason or the test that chose it. Change one only with a new measurement.

## Models

| Model | Use | License |
|---|---|---|
| demucs htdemucs | Stems | MIT code |
| beat_this | Beats and downbeats | MIT |
| Transkun | Piano notes | MIT |
| piano_transcription_inference | Piano notes | MIT |
| basic-pitch | Notes of any instrument, and the bass line | Apache 2.0 |
| BS-RoFormer karaoke (frazer-becruily), through audio-separator | The lead voice | No stated license |
| BTC | Chords | MIT |
| BS-RoFormer SW (jarredou), through audio-separator | Guitar, piano and other stems | No stated license |
| BS-RoFormer bowed strings v2 (model by gilliaan, huggingface.co/gilliaan/Stem-Separation-Models), through audio-separator | Strings stem | CC BY-NC-SA 4.0: non-commercial use only, with attribution to gilliaan |
| BS-RoFormer Mega 53 (ZFTurbo, MVSep; one head a file in huggingface.co/noblebarkrr/BS-Roformer-MVSep-Mega-53-stems), through audio-separator | Brass, woodwinds, organ and synth stems | No stated license (the MSST code is MIT) |
| librosa pYIN | The pitch of the voice | ISC |

No model weights are in the repo. They download at setup or on first use.

## Accuracy and tests

The trust levels that the agent needs are in `SKILL.md`.

The guitar and piano stems were chosen on 12 RWC pop songs, against the MIDI track of each instrument (note F1, onsets within 0.1 s). Piano: Transkun on `piano.wav` .81, basic-pitch on it .66, Transkun on `backing.wav` .55, basic-pitch on the demucs `other` stem .39, htdemucs_6s .40. Guitar: basic-pitch on `guitar.wav` .39, on `other` .34, htdemucs_6s .34. Where the instrument did not play, the stems were usually below -60 dB. The step adds 27 s to `prepare` of a 4-minute song on an RTX 5070 (86 s to 113 s); alone it takes 35 s to split, 2.4 GB of GPU memory and 3 GB of RAM.

The layer stems were chosen on 24 RWC songs against the MIDI notes of each instrument family (basic-pitch notes without the -40 dB gate, note F1; activity = share of the seconds above -40 dB where the family plays / does not play). The `other` stem of SW was better than the demucs `other` for each family: strings F .18 (demucs .08), brass .16 (.08), woodwinds .20 (.06), synth .28 (.16); not playing .57 to .86 (.96 to .98). `strings` (gilliaan bowed strings v2 on `src.wav`): F .22, activity .84 / .22 (11 songs). It misses quiet string parts, and it failed on 2 of the 11 songs (F .03 or less: on one the stem was silent, on the other it kept other instruments). The xlance second stage on the SW `other` gave F .21, activity .98 / .57, because it keeps most of `other`. Mega 53: brass .21, .95 / .06 (5 songs); woodwinds .39, .86 / .05 (3 songs); organ .10, .94 / .18 (10 songs); synth .25, .86 / .09 (15 songs). With the gate of `step parts` (the notes that `--stem` shows), F is strings .27 (xlance .23), brass .55, woodwinds .66, organ .14, synth .30. Brass rests on 5 songs and woodwinds on 3. Mega 53 strings (.08) and the UVR wind model (no flute) were worse. The four Mega 53 heads share one backbone: one pass gives the same stems as four single passes (equal samples) in 34 s instead of 4 x 31 s. Rescored after the integration, all values were the same within .003. On `prepare` of a 4-minute song the layers add 100 s (110 s to 211 s); the peak GPU memory (8.4 GB above the desktop, from the steps that run at the same time) and the lowest free RAM (7 GB of 32 GB) did not change. The full test record (RWC Popular Music, Vocadito, rendered ensembles, one piano and voice song) is kept outside the repo, because some of its data has a non-commercial license. Do not add RWC data or copyrighted arrangements to the repo.
