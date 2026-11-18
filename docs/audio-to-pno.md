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
- It downloads the piano transcription model, the beat tracker model and the chord model. The lead-voice model downloads on the first `prepare`.
- About 1.3 GB, or 3.8 GB with CUDA. ffmpeg must be installed.

## The work folder

`-w <folder>` holds the files of one song. Each command reads and writes there.

| File | Written by | Content |
|---|---|---|
| `src.wav` | `prepare` | The recording, converted by ffmpeg |
| `drums.wav`, `bass.wav`, `other.wav`, `vocals.wav` | step `stems` | demucs stems |
| `backing.wav` | step `stems` | bass + other: the input of the note and chord models |
| `beats.json` | step `beats` | beat_this beats and downbeats |
| `lead.wav` | step `lead` | The lead voice without backing vocals (karaoke model) |
| `tk.json`, `pt.json`, `bp.json` | steps `tk`, `pt`, `bp` | Notes of Transkun, piano_transcription_inference and basic-pitch |
| `bass.json`, `bp_bass.json` | step `bass` | basic-pitch notes of the bass stem |
| `chords.npy` | step `chords` | BTC chord probabilities for each frame |
| `grid.json` | `grid --set` or `grid --constant` | The bar grid that all later commands read: beats in a bar, quarters in a beat, the beat times, and `bars` (`{"57": 2}`) for the bars of another length, if there are any |
| `tune.json` | `tune` | Draft melody without doubtful notes |
| `compare.txt`, `check.txt` | `compare`, `check` | All rows of their lists |
| `cqt.npy`, `cqt_lead.npy`, `f0_lead.npy` | first use | Caches; `prepare` deletes them |

## Commands

| Command | What it does |
|---|---|
| `prepare AUDIO` | Converts the audio, then runs the steps: `stems` and `beats` first, then `tk`, `pt`, `bp`, `bass`, `lead` and `chords` at the same time. Each step is a separate process with its own numba cache. |
| `step NAME` | Runs one step again, for example for a folder made before that step existed |
| `grid` | Fits a bar grid to the tracked beats and the note onsets, and compares it with a constant grid. It finds bars of another length where the tracked downbeats move to another beat (`phase_path`, `odd_bars`), and places each with the chord changes. `--set FIELD=VALUE` corrects and saves it (`bar=N:beats` gives the bars of another length). |
| `roll [BARS]` | Each bar: the chord, the bass note and the model notes, merged across the models. Letters show which models have a note. |
| `raw [BARS]` | The model notes with exact positions and velocities, not merged |
| `bass [BARS]` | The bass line of the bass stem, in the format of `tune` |
| `tune [BARS]` | The sung tune of `lead.wav`, with marks for scoops, late onsets, notes between two semitones and doubtful notes, plus parallel phrases |
| `peek BARS PITCHES` | The loudness of given pitches through time |
| `compare SCORE` | Exports the score with `pno` and compares it with the recording: harmony, notes, melody |
| `check SCORE` | Piano-idiom checks of a score (dead spots, leaps, spans, crossed hands, melody balance, chords) |

## How `a2p.py` is organized

The file has one section for each area, marked with `# ----` lines:

| Section | Main functions |
|---|---|
| (top) | Constants with the reason for each value, `Grid` (the bar grid of a song: all bar positions come from its `odd` bars) and `saved_grid`, `combined` and `viewed` (model notes) |
| prepare | `run_stems`, `run_beat_tracker`, `run_lead`, `run_transkun`, `run_pt`, `run_basic_pitch`, `run_bass`, `run_chords` (`btc_logits` is the BTC forward pass), `cmd_prepare` |
| sung tune | `vocal_f0` (pYIN on `lead.wav`), `semitone_path` (Viterbi), `sung_notes` (syllable cuts, scoop joins, loud starts, soft notes), `octave_errors`, `on_grid`, `parallel_bars`, `cmd_tune` |
| bass line | `bass_line`, `cmd_bass` |
| grid | `repair`, `cover`, `snap`, `phase_path`, `bar_phase`, `line_evidence`, `odd_bars`, `tracked_grid`, `cmd_grid` |
| evidence | `merged`, `bar_bass`, `chord`, `bar_chord`, `cmd_roll`, `cmd_raw` |
| compare | `score_notes`, `cmd_compare` |
| check | `bar_cells` (the cells of each bar, with meter changes and incomplete bars), `onsets` (reads `pno sound --velocity`), `cmd_check` |
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
| librosa pYIN | The pitch of the voice | ISC |

No model weights are in the repo. They download at setup or on first use.

## Accuracy and tests

The trust levels that the agent needs are in `SKILL.md`. The full test record (RWC Popular Music, Vocadito, rendered ensembles, one piano and voice song) is kept outside the repo, because some of its data has a non-commercial license. Do not add RWC data or copyrighted arrangements to the repo.
