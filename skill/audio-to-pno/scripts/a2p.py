"""Evidence and checks for a piano transcription of a recording into pno. The workflow is in SKILL.md."""
import argparse, json, math, os, re, subprocess, sys, tempfile
from collections import Counter
from fractions import Fraction
from pathlib import Path

import numpy as np

SR, HOP, LOWEST_MIDI, BINS = 22050, 512, 24, 84
NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B']
STEP = {'C': 0, 'D': 2, 'E': 4, 'F': 5, 'G': 7, 'A': 9, 'B': 11}
PITCH = re.compile(r'([A-G])(#*|b*)(-?\d)')
RANGE = r'\d+(:\d+(\.\d+)?)?(-\d+(:\d+(\.\d+)?)?)?'
RANGES = re.compile(rf'{RANGE}(,{RANGE})*')
DRAFT_LINE = re.compile(r'\s*(\d+)\s+((?:rh|lh)\d*)(\s[^|]*)?')
BAR_LINE = re.compile(r'\s*(\d+)\s')
SLOT_NOTE = re.compile(r'(\d+):(.+?)/(\d+)\S*')      # slot:pitch/sixteenths as roll and tune print it, with their marks after it
STRONG = 85          # piano-model velocity that marks a prominent note
MELODY_FLOOR = 69    # with no song file, the tune is the top score note from A4
MODELS = {'t': 'tk', 'p': 'pt', 'b': 'bp'}      # the letter of a model in the output, and its note file
AGREE_S = 0.08       # two models have the same note when their onsets are this near
BEAT_MODEL = Path.home() / '.cache' / 'audio-to-pno' / 'beat_this-final0.ckpt'
# the lead voice without backing vocals: in 24 pop songs it gave `tune` 3 points more than the vocals of demucs
LEAD_MODEL, LEAD_MODEL_DIR = 'bs_roformer_karaoke_frazer_becruily.ckpt', BEAT_MODEL.parent / 'separator'
# guitar.wav and piano.wav, and the model that reads each: in 12 RWC songs this split gave piano notes F .81 (bp on the demucs
# other stem .39, Transkun on backing.wav .55, htdemucs_6s .40) and guitar notes F .39 (.34; htdemucs_6s .34)
PARTS_MODEL = 'BS-Roformer-SW.ckpt'
# bp notes of more stems, in 24 RWC songs: note F of the instrument family / share of the seconds without it where the stem plays.
# The other stem of SW: strings .18 / .57, brass .16 / .86, woodwinds .20 / .77, synth .28 / .58 (htdemucs other: .06-.16 / .96-.98).
# strings: gilliaan bowed strings v2 on src.wav (CC BY-NC-SA 4.0) .22 / .22, but it misses quiet parts; Mega 53 (no licence stated): brass .21 / .06 (5 songs),
# woodwinds .39 / .05 (3 songs), organ .10 / .18 (htdemucs other .05 / .98), synth .25 / .09. With the gate of audible_notes, F of
# strings .27, brass .55, woodwinds .66, organ .14, synth .30
LAYERS = ('strings', 'brass', 'woodwinds', 'organ', 'synth')
PART_NOTES = {'guitar': {'b': 'bp_guitar'}, 'piano': {'t': 'tk_piano'}} | {layer: {'b': f'bp_{layer}'} for layer in LAYERS}
STRINGS_MODEL = (LEAD_MODEL_DIR / 'gilliaan_bsroformer_bowedstrings_v2.ckpt', 'https://huggingface.co/gilliaan/Stem-Separation-Models/resolve/'
                 '62fe7d8e363e7285d1d24aaaab87351ebc566bae/BowedStrings/BandSplitRoformer/gilliaan_bsroformer_bowedstrings_v2.ckpt')
MEGA_URL = 'https://huggingface.co/noblebarkrr/BS-Roformer-MVSep-Mega-53-stems/resolve/0677941f9cdfd891a6bc3336229244e197035327/v1/bs_mega_53stem_'
# the Mega 53 files have one head each on the same backbone: one model with the four heads gives the same stems in a quarter of the time
MEGA_HEADS = {'brass': 'brass', 'woodwind': 'woodwinds', 'organ': 'organ', 'synth': 'synth'}
# the chord model BTC reads the log CQT of 22.05 kHz audio (24 bins an octave from C1) in windows of 108 frames of 93 ms
CHORD_MODEL = BEAT_MODEL.parent / 'btc_model_large_voca.pt'
CHORD_SR, CHORD_HOP, CHORD_BINS, CHORD_WINDOW = 22050, 2048, 144, 108
# the 14 chord types of BTC, in its order, as the triad that `roll` prints: in 24 pop songs 1 of 3 of its sevenths was false
CHORD_KINDS = ['m', '', 'dim', 'aug', 'm', '', 'm', 'm', '', '', 'dim', 'dim', 'sus2', 'sus4']
UNSURE = 0.6        # 24 pop songs: below this probability the root of the chord was right half of the time, above it 9 times of 10
SECOND_CHORD = 0.2  # roll --beats shows the second chord of a beat from this probability
# beat repair, tuned on rendered clips: a gap of 0.5 or 1.5 beats (this near) in this share of the gaps means two beat levels;
# a tracked beat less than EXTRA_BEAT of a beat after the last one is not a beat when the next one follows in time
LEVEL_REACH, LEVEL_SHARE, EXTRA_BEAT = 0.15, 0.15, 0.6
# tuned on rendered ensembles: the piano models do not hear a lead instrument that is not a piano,
# so the mix view also shows a note that only bp has when it is this long
B_LONG_S = 0.25
VIEWS = {'piano': 'notes that Transkun has', 'mix': f'notes of two models, and bp notes of {B_LONG_S} s or longer', 'all': 'all notes'}
GRID_FIELDS = 'beats_per_bar=N shift=N level=half|third|double quarters_per_beat=1.5 bar=N:beats'
# a bar of another length: the bars start on another beat when this many more tracked downbeats are on it (93 pop songs with regular
# bars: no false bar from 4 up; 238 edits of 30 recordings, each with one bar made shorter or longer: all found up to 8)
PHASE_MOVE = 6
# its place, tuned on 96 of the edits: bars to each side of the place that the tracker gives; a tracked downbeat counts as this much chord change
PLACE_REACH, PLACE_DOWNBEAT = 3, 0.25
# f0 of the vocal stem, one frame each 10 ms; 0.2 semitone bins are 4 times faster than 0.1 and give the same notes
F0_SR, F0_HOP, F0_FRAME, F0_MIN, F0_MAX, F0_STEP = 16000, 160, 1024, 65, 1050, 0.2
BRIGHT_FRAME, BRIGHT_HZ = 512, 1000     # the level above 1 kHz in 32 ms frames: a consonant lowers it more than the full level
# sung notes, tuned on Vocadito (40 solo singing clips with annotated notes); each value has a flat optimum
SUNG_GATE_DB = -22  # a frame is sung when lead.wav is above this level, relative to the 99th percentile of the mix (tuned on 24 pop songs)
BASS_GATE_DB = -40  # a frame has a bass note when bass.wav is above this level, relative to the 99th percentile of the mix
SIGMA = 0.5         # semitones: how far vibrato and drift go from the note
CHANGE = 16         # cost of a new note; with SIGMA it sets how long a pitch must hold to be a note
MIN_S = 0.1         # a shorter note joins its neighbour
DIP_DB = 10         # a dip this deep in the level above 1 kHz is a consonant: a new syllable starts
BETWEEN_C = 45      # a note whose steady middle is this many cents from its semitone is sung between two semitones
# windows of a melody note in seconds, not beats, so that the meter of the grid does not change the result (RWC: true notes flagged
# .084 -> .059 by song, .189 -> .163 by MELODY; wrong notes flagged as often)
SUNG_FROM_S, SUNG_TO_S = 0.05, 0.5    # song: the f0 from this long after the onset, until the next note or this long after the onset
MELODY_S = 0.4      # tune_level: the CQT from the onset, until the next note or this long after it
WEAK_S = 0.16       # Vocadito: 6 of 10 shorter notes are true, 9 of 10 longer notes
LEAP = 12           # semitones from the median of the reliable notes around it: often an octave error or a backing voice
SOFT_DB, SOFT_REACH_S = 12, 1.5    # this much softer than a note this near: a fading end, a breath or bleed (true in 1 of 3 on Vocadito)
OCTAVE_DB = 30      # the stem this much weaker at the pitch than an octave higher: pYIN took half the frequency (no Vocadito note)
SCOOP_S = {1: 0.45, 2: 0.33}       # a scoop from this many semitones below the note it leads into is shorter than this
GAP_S = 0.03        # notes this near have no gap between them
START_DB, START_REACH_S = 10, 0.25  # a note starts where it comes within START_DB of its top, at most this late
LATE = 0.25         # slots after its slot: the onset can belong to the next slot
EIGHTH_S = 0.111    # a sixteenth shorter than this (above 135 BPM): the voice sings eighths (of 88 pop songs, no faster one had 1 note in 10 between them)
BASS_MIN_S = 0.08   # the shortest bp note (run_basic_pitch); a bass note cut shorter by the next note is dropped
# parallel bars, tuned on 12 pop songs with known notes and one song of piano and voice
PAR_REACH = 1       # slots between a note and the same note in the parallel bar
PAR_SHARE = 0.6     # share of the notes of a bar that the parallel bar has
PAR_NOTES = 6       # notes that a run of parallel bars shares
INST_SHARE = 0.75   # the notes of an instrument are dense, so more of the sung bar must be in them
# stem levels, in dB relative to the 99th percentile of the mix in frames of LEVEL_S
LEVEL_S = 0.1
STEM_GATE_DB = -40  # a stem plays where its level in 1 s is above this (moored: the drum stem -60 dB without drums, -20 with them)
SPAN_GAP_S = 4      # a stem that stops for a shorter time still plays
FADE_DB, FADE_S = 6, 4      # the mix ends with a fade when it is this much below its median level for this long before its end
TUNING_FRAME = 16384        # 0.37 s at 44.1 kHz: 2.7 Hz bins
# onsets of a stem: peaks of the spectral flux of 23 ms frames each 5.4 ms at 44.1 kHz (noise bursts: 1 ms early, sd 2 ms)
ONSET_FRAME, ONSET_HOP, ONSET_GAP_S = 1024, 240, 0.09
KICK_HZ, SNARE_HZ = (30, 150), (180, 2500)
STRONG_HIT = 0.3    # of the 99.8th percentile of the flux: a strong drum hit
FIT_REACH_S = 0.035 # strong drum hits this near an eighth of the grid fit it (moored: 106 hits gave the beat of the hand grid within 0.1 ms)
DRIFT_BARS = 8
LEVELS_S = 4        # `levels` without a grid: seconds in a row
DEAD_S = 1.5        # check: a gap with no new note; not tuned (2 beats at 80 BPM)
LEAP_S = 0.25       # check: a leap of more than an octave in less time is fast; not tuned (an eighth at 120 BPM)
CHANGE_LEAD = 1.15     # `grid`: a beat of the bar with this times the chord change of beat 1 is named; 24 RWC songs with known bars: at most 1.07, shifted by 2 beats at least 1.26
GRID_STEMS = ('drums', 'bass', 'other4', 'lead')   # the start of the music and the drift of `grid` were tuned on the htdemucs other stem


def name(p):
    return NAMES[p % 12] + str(p // 12 - 1)


def midi(token):
    m = PITCH.fullmatch(token)
    if not m:
        sys.exit(f'not a pitch: {token}')
    return STEP[m[1]] + m[2].count('#') - m[2].count('b') + 12 * (int(m[3]) + 1)


def bar_list(spec):
    bars = []
    for part in spec.split(','):
        first, _, last = part.partition('-')
        bars += range(int(first), int(last or first) + 1)
    return bars


def in_ranges(spec, bar, beat):
    """True when bar:beat (beat 1 is the first beat) is in ranges such as 14-21,29:3.5-38, both ends included."""
    if not RANGES.fullmatch(spec):
        sys.exit(f'not bar ranges such as 14-21,29:3.5-38: {spec}')

    def position(text, end):
        bar, _, beat = text.partition(':')
        return int(bar), float(beat) if beat else math.inf if end else 1

    return any(position(first, False) <= (bar, beat) <= position(last or first, True)
               for first, _, last in (part.partition('-') for part in spec.split(',')))


def octaves_from_sung(song, bar, beat):
    """The octaves from the sung pitch to the melody of song.json at bar:beat, as `octave` recorded them."""
    return sum(n for spec, n in song.get('octaves', {}).items() if in_ranges(spec, bar, beat))


def bar_ranges(bars):
    """Sorted bar numbers as text: 3-9, 14, 20-22."""
    runs = []
    for bar in bars:
        if runs and bar == runs[-1][1] + 1:
            runs[-1][1] = bar
        else:
            runs.append([bar, bar])
    return ', '.join(str(a) if a == b else f'{a}-{b}' for a, b in runs)


def by_bar(items):
    groups = {}
    for bar, text in items:
        groups.setdefault(bar, []).append(text)
    return [f'  {bar}: ' + ' '.join(groups[bar]) for bar in sorted(groups)]


def short(x):
    return f'{x:.2f}'.lstrip('0')       # 0.71 -> .71


def first_rows(lines, rows):
    return lines[:rows] + ([f'  ... {len(lines) - rows} more'] if len(lines) > rows else [])


def write_detail(work, file, lines):
    work.mkdir(parents=True, exist_ok=True)
    (work / file).write_text('\n'.join(lines) + '\n')
    print(f'all rows: {(work / file).as_posix()}')


def beat_at(beats, seconds):
    i = np.clip(np.searchsorted(beats, seconds, side='right') - 1, 0, len(beats) - 2)
    return i + (seconds - beats[i]) / (beats[i + 1] - beats[i])


class Grid:
    """The bar grid of one song, saved by `grid --set` and read by every later command."""

    def __init__(self, g):
        self.per_bar, self.quarters = g['beats_per_bar'], g['quarters_per_beat']
        self.odd = {int(bar): count for bar, count in g.get('bars', {}).items()}    # the beats of each bar that does not have per_bar beats
        self.slots = 6 if self.quarters == 1.5 else 4      # sixteenths in one beat
        self.beats = np.array(g['beats'])                  # the time of each beat, from beat 1 of bar 1

    def beat(self, seconds):
        """Beats from the start of bar 1. Before the first and after the last beat, the nearest tempo continues."""
        return beat_at(self.beats, seconds)

    def time(self, beat):
        i = min(max(math.floor(beat), 0), len(self.beats) - 2)
        return float(self.beats[i] + (beat - i) * (self.beats[i + 1] - self.beats[i]))

    def slot(self, seconds):
        return self.beat(seconds) * self.slots

    def count(self, bar):
        return self.odd.get(bar, self.per_bar)

    def first(self, bar):
        """The beat that starts the bar, counted from the start of bar 1."""
        return (bar - 1) * self.per_bar + sum(count - self.per_bar for odd, count in self.odd.items() if odd < bar)

    def bar_at(self, position, unit=1):
        """The bar at a position in beats from the start of bar 1 (in slots with unit=self.slots), and the position in that bar."""
        regular, moved = self.per_bar * unit, 0
        for bar, count in sorted(self.odd.items()):
            start = (bar - 1) * regular + moved
            if position < start:
                break
            if position < start + count * unit:
                return bar, position - start
            moved += count * unit - regular
        bars = (position - moved) // regular
        return int(bars) + 1, position - moved - bars * regular

    def place(self, slot):
        return self.bar_at(slot, self.slots)

    def label(self, bar, unit=1):
        """The bar number; a bar that does not have per_bar beats also shows its beats (its slots with unit=self.slots)."""
        return f"{bar} ({self.odd[bar] * unit} {'beats' if unit == 1 else 'slots'})" if bar in self.odd else str(bar)

    def bar_start(self, bar):
        return self.time(self.first(bar))


def saved_grid(work):
    path = work / 'grid.json'
    if not path.exists():
        sys.exit(f'{path} is missing: run `grid`, then `grid --set`')
    return Grid(json.loads(path.read_text()))


def model_notes(work, source):
    path = work / f'{source}.json'
    if not path.exists():
        sys.exit(f'{path} is missing: run `prepare` (`step parts` for the notes of a stem)')
    return sorted(json.loads(path.read_text())['notes'], key=lambda n: (n['on'], n['p']))


def combined(work, models=MODELS):
    """One note list from the models (by default the three of MODELS). `by` has the letters of the models that have the note.
    The onset is from the best model that has it (t, p, b), the length from t, b, p.
    `v` is a velocity of a piano model (None when only bp has the note); `amp` is the bp amplitude."""
    tagged = sorted(((n['p'], n['on'], mark, n) for mark, source in models.items() for n in model_notes(work, source)),
                    key=lambda row: row[:2])
    groups = []
    for pitch, on, mark, note in tagged:
        group = groups[-1] if groups else None
        if group and group['p'] == pitch and on - group['on'] <= AGREE_S and mark not in group['by']:
            group['by'][mark] = note
        else:
            groups.append(dict(p=pitch, on=on, by={mark: note}))
    notes = []
    for group in groups:
        by = group['by']
        lead = by.get('t') or by.get('p') or by['b']
        length = by.get('t') or by.get('b') or by['p']
        piano = by.get('t') or by.get('p')
        notes.append(dict(on=lead['on'], off=lead['on'] + length['off'] - length['on'], p=group['p'],
                          v=piano['v'] if piano else None, amp=by['b']['v'] if 'b' in by else None,
                          by=''.join(mark for mark in MODELS if mark in by)))
    return sorted(notes, key=lambda n: (n['on'], n['p']))


def shown_notes(a):
    """The merged model notes in the view of `a`; with --stem, all notes of the model of that stem."""
    if a.stem:
        return viewed(combined(a.work, PART_NOTES[a.stem]), 'all')
    return viewed(combined(a.work), a.view)


def solo_share(notes):
    groups = Counter(n['by'] for n in notes)
    return groups['tpb'] / max(1, groups['tpb'] + groups['pb'])


def viewed(notes, view):
    """The notes with `show` set."""
    shown = {'piano': lambda n: 't' in n['by'],
             'mix': lambda n: len(n['by']) > 1 or (n['by'] == 'b' and n['off'] - n['on'] >= B_LONG_S),
             'all': lambda n: True}[view]
    return [dict(n, show=shown(n)) for n in notes]


def cqt(wav, cache=None):
    if cache and cache.exists():
        spectrum = np.load(cache)
    else:
        import librosa
        y, _ = librosa.load(wav, sr=SR, mono=True)
        spectrum = np.abs(librosa.cqt(y, sr=SR, hop_length=HOP, fmin=librosa.midi_to_hz(LOWEST_MIDI), n_bins=BINS))
        if cache:
            np.save(cache, spectrum)
    return spectrum, np.arange(spectrum.shape[1]) * HOP / SR


def source_cqt(work):
    return cqt(work / 'src.wav', work / 'cqt.npy')


def window(spectrum, times, start, end):
    frames = spectrum[:, (times >= start) & (times < end)]
    return frames.mean(1) if frames.shape[1] else np.zeros(spectrum.shape[0])


def chroma(bins):
    return np.array([bins[k::12].sum() for k in range(12)])


def unit(v):
    return v / (np.linalg.norm(v) + 1e-9)


def stem_wav(work, stem):
    path = work / f'{stem}.wav'
    if not path.exists():
        sys.exit(f'{path} is missing: run `step parts` (older work folders do not have it)')
    return path


def mono(work, stem):
    import soundfile as sf
    x, sr = sf.read(stem_wav(work, stem), dtype='float32')
    return x.reshape(len(x), -1).mean(1), sr


def power(work, stem):
    """The mean square of a stem in frames of LEVEL_S."""
    x, sr = mono(work, stem)
    n = int(sr * LEVEL_S)
    return (x[:len(x) // n * n].reshape(-1, n) ** 2).mean(1)


def levels(work, stems, edges):
    """{stem: level in dB between each two times of `edges`}, relative to the 99th percentile of the mix in frames of LEVEL_S."""
    top = np.percentile(power(work, 'src'), 99)
    out = {}
    for stem in stems:
        p = power(work, stem)
        cuts = np.clip(np.round(np.asarray(edges) / LEVEL_S).astype(int), 0, len(p))
        out[stem] = np.array([10 * np.log10(p[a:max(b, a + 1)].mean() / top + 1e-12) if a < len(p) else -120.0 for a, b in zip(cuts, cuts[1:])])
    return out


def played_stems(work, stems=('drums', 'bass', 'other', 'lead', *PART_NOTES)):
    return [stem for stem in stems if (work / f'{stem}.wav').exists()]


def spans(level):
    """(start s, end s) of the runs of 1 s frames above STEM_GATE_DB; a gap shorter than SPAN_GAP_S joins two runs."""
    runs = []
    for frame in np.flatnonzero(level > STEM_GATE_DB):
        if runs and frame - runs[-1][1] < SPAN_GAP_S:
            runs[-1][1] = frame + 1
        else:
            runs.append([frame, frame + 1])
    return [(int(a), int(b)) for a, b in runs]


def stem_onsets(work, stem):
    """The onsets of a stem where it plays: times, strength (1 at the 99.8th percentile of the spectral flux), and the rise
    in the kick band and in the snare band (each 1 at its 95th percentile)."""
    from scipy.signal import find_peaks, stft
    x, sr = mono(work, stem)
    freqs, times, spectrum = stft(x, sr, nperseg=ONSET_FRAME, noverlap=ONSET_FRAME - ONSET_HOP)
    magnitude = np.log1p(50 * np.abs(spectrum))
    rise = np.maximum(np.diff(magnitude, axis=1, prepend=magnitude[:, :1]), 0)
    flux = rise.sum(0)
    top = np.percentile(flux, 99.8)
    peaks, _ = find_peaks(flux, height=0.1 * top, distance=int(ONSET_GAP_S * sr / ONSET_HOP))
    plays = levels(work, [stem], np.arange(0, times[-1] + 2))[stem] > STEM_GATE_DB
    peaks = peaks[plays[times[peaks].astype(int)]]
    if not len(peaks):
        return np.zeros((4, 0))

    def band(low, high):
        rises = np.array([rise[(freqs >= low) & (freqs < high), p:p + 3].sum() for p in peaks])
        return rises / (np.percentile(rises, 95) + 1e-9)

    return np.array([times[peaks], flux[peaks] / top, band(*KICK_HZ), band(*SNARE_HZ)])


# ---------------------------------------------------------------- prepare

def save_notes(work, source, notes):
    (work / f'{source}.json').write_text(json.dumps(dict(notes=notes)))


def device():
    import torch
    return 'cuda' if torch.cuda.is_available() else 'cpu'


def run_lead(work):
    from audio_separator.separator import Separator
    # threshold 1.0 keeps the level of the model: `tune` gates the stem against the level of the mix; overlap 1: 4 gave the same notes
    separator = Separator(output_dir=str(work), model_file_dir=str(LEAD_MODEL_DIR), output_format='WAV', normalization_threshold=1.0,
                          use_soundfile=True, output_single_stem='Vocals',
                          mdxc_params=dict(segment_size=256, override_model_segment_size=False, batch_size=1, pitch_shift=0, overlap=1))
    separator.load_model(model_filename=LEAD_MODEL)
    separator.separate(str(work / 'src.wav'), custom_output_names={'Vocals': 'lead'})


def run_stems(work):
    import soundfile as sf, torch
    from demucs.apply import apply_model
    from demucs.pretrained import get_model
    mix, sr = sf.read(work / 'src.wav', dtype='float32')
    model = get_model('htdemucs')
    model.eval()
    x = torch.from_numpy(mix.T)[None]
    ref = x.mean(1)
    with torch.no_grad():
        stems = apply_model(model, (x - ref.mean()) / ref.std(), device=device(), split=True, overlap=0.25)[0]
    stems = stems * ref.std() + ref.mean()
    for stem, signal in zip(model.sources, stems):
        # other.wav is the other stem of SW (`parts`)
        sf.write(work / f'{"other4" if stem == "other" else stem}.wav', signal.numpy().T, sr)
    # the note models read this: a voice and drums give them false notes
    sf.write(work / 'backing.wav', sum(signal for stem, signal in zip(model.sources, stems) if stem in ('bass', 'other')).numpy().T, sr)


def run_transkun(work, source='backing', target='tk'):
    import moduleconf, soundfile as sf, soxr, torch
    from importlib.resources import files
    folder = files('transkun') / 'pretrained'
    conf = moduleconf.parseFromFile(str(folder / '2.0.conf'))['Model']
    model = conf.module.TransKun(conf=conf.config)
    state = torch.load(str(folder / '2.0.pt'), map_location='cpu')
    model.load_state_dict(state.get('best_state_dict', state.get('state_dict')), strict=False)
    model.to(device()).eval()
    audio, rate = sf.read(work / f'{source}.wav', dtype='float32')
    with torch.no_grad():
        events = model.transcribe(torch.from_numpy(soxr.resample(audio, rate, model.fs)).to(device()), discardSecondHalf=False)
    # a pedal event has a negative pitch
    save_notes(work, target, [dict(on=float(e.start), off=float(e.end), p=int(e.pitch), v=int(e.velocity)) for e in events if e.pitch > 0])


def run_pt(work):
    import librosa
    from piano_transcription_inference import PianoTranscription, sample_rate
    audio, _ = librosa.load(work / 'backing.wav', sr=sample_rate, mono=True)
    result = PianoTranscription(device=device(), checkpoint_path=None).transcribe(audio, str(work / 'pt.mid'))
    save_notes(work, 'pt', [dict(on=float(e['onset_time']), off=float(e['offset_time']), p=int(e['midi_note']), v=int(e['velocity']))
                            for e in result['est_note_events']])


def run_basic_pitch(work, source='backing', target='bp'):
    from basic_pitch import ICASSP_2022_MODEL_PATH
    from basic_pitch.inference import predict
    _, _, events = predict(str(work / f'{source}.wav'), ICASSP_2022_MODEL_PATH, minimum_note_length=80)
    # `v` of a bp note is an amplitude, not a key velocity
    save_notes(work, target, [dict(on=float(s), off=float(e), p=int(p), v=int(round(amp * 127))) for s, e, p, amp, _ in events])


def audible_notes(work, stem, notes, gate_db):
    """The notes where the stem is above gate_db, relative to the 99th percentile of the mix: a silent stem has the rest of the
    other instruments, and the note models give notes for it (bp in the bass stem of a solo piano: about 100 in a minute)."""
    import librosa
    level = lambda name: 20 * np.log10(librosa.feature.rms(y=librosa.load(work / f'{name}.wav', sr=F0_SR, mono=True)[0],
                                                           frame_length=F0_FRAME, hop_length=F0_HOP)[0] + 1e-9)
    stem_level, floor = level(stem), np.percentile(level('src'), 99) + gate_db
    frame = lambda seconds: min(int(seconds * F0_SR / F0_HOP), len(stem_level) - 1)
    return [n for n in notes if stem_level[frame(n['on']):frame(n['off']) + 1].mean() > floor]


def run_bass(work):
    """The bass line of the bass stem: the bp notes of bass.wav, one note at a time. No notes where the stem is silent."""
    run_basic_pitch(work, 'bass', 'bp_bass')
    line = []
    for n in audible_notes(work, 'bass', model_notes(work, 'bp_bass'), BASS_GATE_DB):
        last = line[-1] if line else None
        if not last or n['on'] >= last['off'] - 0.03:
            line.append(n)
        elif n['p'] == last['p']:
            last['off'] = max(last['off'], n['off'])        # bp starts a sounding note again
        elif n['v'] > last['v'] and n['on'] - last['on'] <= AGREE_S:
            line[-1] = n                                    # of two notes that start together, the louder one: the other is a harmonic
        elif n['v'] > last['v']:
            last['off'] = n['on']
            line.append(n)
    save_notes(work, 'bass', [n for n in line if n['off'] - n['on'] >= BASS_MIN_S])


def fetch(path, url):
    if not path.exists():
        import urllib.request
        part = path.with_suffix('.part')
        urllib.request.urlretrieve(url, part)
        part.replace(path)
    return path


def strings_config():
    import yaml
    path, url = STRINGS_MODEL
    return yaml.load(fetch(path.with_suffix('.yaml'), url.replace('.ckpt', '.yaml')).read_text(), Loader=yaml.FullLoader)


def mega_model():
    """One model file with the heads of MEGA_HEADS, and its config."""
    import torch, yaml
    config = yaml.load(fetch(LEAD_MODEL_DIR / 'bs_mega_53stem_brass_mvsep_config.yaml', MEGA_URL + 'brass_mvsep_config.yaml').read_text(),
                       Loader=yaml.FullLoader)
    config['model']['num_stems'] = len(MEGA_HEADS)
    config['training'] = dict(instruments=list(MEGA_HEADS), target_instrument=None)
    path = LEAD_MODEL_DIR / 'mega53_heads.ckpt'
    if not path.exists():
        state = {}
        for k, head in enumerate(MEGA_HEADS):
            single = fetch(LEAD_MODEL_DIR / f'bs_mega_53stem_{head}_mvsep.ckpt', f'{MEGA_URL}{head}_mvsep.ckpt')
            state |= {key.replace('mask_estimators.0.', f'mask_estimators.{k}.'): value
                      for key, value in torch.load(single, map_location='cpu', weights_only=True).items()}
        torch.save(state, path.with_suffix('.part'))
        path.with_suffix('.part').replace(path)
        for head in MEGA_HEADS:
            (LEAD_MODEL_DIR / f'bs_mega_53stem_{head}_mvsep.ckpt').unlink()
    return path, config


def separate(work, folder, source, names, model, config=None):
    """The stems `names` ({model stem: file name}) of source.wav into the work folder; the other stems stay in `folder`."""
    from audio_separator.separator import Separator
    separator = Separator(output_dir=folder, model_file_dir=str(LEAD_MODEL_DIR), output_format='WAV', normalization_threshold=1.0,
                          use_soundfile=True, sample_rate=config['audio']['sample_rate'] if config else 44100,
                          mdxc_params=dict(segment_size=256, override_model_segment_size=False, batch_size=1, pitch_shift=0, overlap=2))
    if config:
        # audio-separator 0.48 loads only the models of its list
        config['is_roformer'] = True
        separator.download_model_files = lambda name: (name, 'MDXC', name, str(model), 'config')
        separator.load_model_data_from_yaml = lambda _: config
    separator.load_model(model_filename=Path(model).name)
    separator.separate(str(work / f'{source}.wav'), custom_output_names=names)
    for name in names.values():
        Path(folder, f'{name}.wav').replace(work / f'{name}.wav')


def run_parts(work):
    if (work / 'other.wav').exists() and not (work / 'other4.wav').exists():
        (work / 'other.wav').replace(work / 'other4.wav')      # a work folder from before: other.wav is the htdemucs stem
    with tempfile.TemporaryDirectory(dir=work) as folder:
        separate(work, folder, 'src', {stem: stem for stem in ('guitar', 'piano', 'other')}, PARTS_MODEL)
        separate(work, folder, 'src', {'strings': 'strings'}, fetch(*STRINGS_MODEL), strings_config())
        separate(work, folder, 'src', MEGA_HEADS, *mega_model())
    run_transkun(work, 'piano', 'tk_piano')
    for part in ('guitar', *LAYERS):
        run_basic_pitch(work, part, f'bp_{part}')
    for part, models in PART_NOTES.items():
        for source in models.values():
            save_notes(work, source, audible_notes(work, part, model_notes(work, source), STEM_GATE_DB))


def run_beat_tracker(work):
    from beat_this.inference import File2Beats
    if not BEAT_MODEL.exists():
        sys.exit(f'{BEAT_MODEL} is missing: run `python {Path(__file__).with_name("setup_env.py")}`')
    beats, downbeats = File2Beats(checkpoint_path=str(BEAT_MODEL), device=device(), dbn=False)(str(work / 'src.wav'))
    (work / 'beats.json').write_text(json.dumps(dict(beats=[float(b) for b in beats], downbeats=[float(d) for d in downbeats])))


def btc_logits(w, x):
    """BTC (Park et al. 2019) from its weights: a transformer that reads each window to the past and to the future.
    x is [windows, CHORD_WINDOW, CHORD_BINS]; the result has the 170 chords of each frame."""
    import torch
    import torch.nn.functional as F
    norm = lambda h, key: w[key + '.gamma'] * (h - h.mean(-1, keepdim=True)) / (h.std(-1, keepdim=True) + 1e-6) + w[key + '.beta']
    heads = lambda h: h.view(*h.shape[:2], 4, -1).transpose(1, 2)
    turn = torch.arange(CHORD_WINDOW)[:, None] * torch.exp(torch.arange(64) * -(math.log(1e4) / 63))
    future = torch.triu(torch.full((CHORD_WINDOW, CHORD_WINDOW), -torch.inf), 1)
    h = x @ w['self_attn_layers.embedding_proj.weight'].T + torch.cat([turn.sin(), turn.cos()], 1)
    for layer in range(8):
        sides = []
        for side, mask in (('attn_block', future), ('backward_attn_block', future.T)):
            key = f'self_attn_layers.self_attn_layers.{layer}.{side}.'
            q, k, v = (heads(norm(h, key + 'layer_norm_mha') @ w[f'{key}multi_head_attention.{part}_linear.weight'].T) for part in ('query', 'key', 'value'))
            seen = torch.softmax(q @ k.transpose(-1, -2) / q.shape[-1] ** 0.5 + mask, -1) @ v
            y = h + seen.transpose(1, 2).reshape(h.shape) @ w[key + 'multi_head_attention.output_linear.weight'].T
            f = norm(y, key + 'layer_norm_ffn').transpose(1, 2)
            for index in (0, 1):
                conv = f'{key}positionwise_convolution.layers.{index}.conv.'
                f = F.relu(F.conv1d(f, w[conv + 'weight'], w[conv + 'bias'], padding=1))
            sides.append(y + f.transpose(1, 2))
        key = f'self_attn_layers.self_attn_layers.{layer}.linear.'
        h = torch.cat(sides, -1) @ w[key + 'weight'].T + w[key + 'bias']
    return norm(h, 'self_attn_layers.layer_norm') @ w['output_layer.output_projection.weight'].T + w['output_layer.output_projection.bias']


def run_chords(work):
    """chords.npy: the probability of each of the 170 chords of BTC in each frame of backing.wav."""
    import librosa, torch
    if not CHORD_MODEL.exists():
        sys.exit(f'{CHORD_MODEL} is missing: run `python {Path(__file__).with_name("setup_env.py")}`')
    # the file has two numpy numbers beside the weights; nothing else can come out of the pickle
    with torch.serialization.safe_globals([(np._core.multiarray.scalar, 'numpy.core.multiarray.scalar'), np.dtype, np.dtypes.Float64DType]):
        saved = torch.load(CHORD_MODEL, map_location='cpu', weights_only=True)
    audio, _ = librosa.load(work / 'backing.wav', sr=CHORD_SR, mono=True)
    cqt = librosa.cqt(audio, sr=CHORD_SR, hop_length=CHORD_HOP, fmin=librosa.note_to_hz('C1'), n_bins=CHORD_BINS, bins_per_octave=24)
    x = (np.log(np.abs(cqt.T) + 1e-6) - saved['mean']) / saved['std']
    windows = torch.from_numpy(np.pad(x, ((0, -len(x) % CHORD_WINDOW), (0, 0)))).float().view(-1, CHORD_WINDOW, CHORD_BINS)
    with torch.no_grad():
        p = torch.softmax(btc_logits(saved['model'], windows), -1)
    np.save(work / 'chords.npy', p.view(-1, p.shape[-1])[:len(x)].numpy().astype(np.float16))


STEPS = dict(stems=run_stems, beats=run_beat_tracker)      # these read src.wav
# lead reads src.wav too: it runs here so that it shares the GPU with demucs less
AFTER_STEMS = dict(tk=run_transkun, pt=run_pt, bp=run_basic_pitch, bass=run_bass, lead=run_lead, chords=run_chords, parts=run_parts)


def cmd_step(a):
    (STEPS | AFTER_STEMS)[a.name](a.work)


def cmd_prepare(a):
    import soundfile as sf
    a.work.mkdir(parents=True, exist_ok=True)
    wav = a.work / 'src.wav'
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', a.audio,
                    '-ac', '2', '-ar', '44100', str(wav)], check=True)
    for cache in [*a.work.glob('cqt*.npy'), a.work / 'f0_lead.npy']:
        cache.unlink(missing_ok=True)

    log = a.work / 'prepare.log'
    # three steps are heavy; with no thread limit they slow each other and the steps take longer than one after the other
    threads = str(max(1, os.cpu_count() // 3))
    with open(log, 'w') as out:         # the models print progress; it goes to the log
        # steps that run at the same time and write one numba cache can damage it; librosa.pyin in `tune` then crashes
        start = lambda step: subprocess.Popen([sys.executable, __file__, '-w', str(a.work), 'step', step], stdout=out, stderr=out,
                                              env=dict(os.environ, OMP_NUM_THREADS=threads, MKL_NUM_THREADS=threads,
                                                       NUMBA_CACHE_DIR=str(BEAT_MODEL.parent / 'numba' / step)))
        failed = []
        for group in (STEPS, AFTER_STEMS):
            if 'stems' not in failed:
                running = {step: start(step) for step in group}
                failed += [step for step, process in running.items() if process.wait()]
    if failed:
        sys.exit(f'failed: {", ".join(failed)}; read {log.as_posix()}')

    rms = lambda path: float(np.sqrt((sf.read(path, dtype='float32')[0] ** 2).mean()))
    mix = rms(wav)
    print(f'{sf.info(wav).duration:.0f} s; stems, share of the mix level: '
          + ' '.join(f'{stem} {rms(a.work / f"{stem}.wav") / mix:.0%}' for stem in ('drums', 'bass', 'other', 'vocals', *PART_NOTES)))
    notes = combined(a.work)
    groups = Counter(n['by'] for n in notes)
    print('notes: ' + ' '.join(f'{source} {len(model_notes(a.work, source))}' for source in MODELS.values())
          + f'; merged {len(notes)}: ' + ' '.join(f'{by} {count}' for by, count in groups.most_common()))
    print(f'Transkun has {solo_share(notes):.0%} of the notes that pt and bp agree on (solo piano: 89% or more; other recordings: 94% or less)')
    last_onset, again = {}, 0
    bp = model_notes(a.work, 'bp')
    for n in bp:
        again += n['on'] - last_onset.get(n['p'], -9) < 0.25
        last_onset[n['p']] = n['on']
    print(f'{again / max(1, len(bp)):.0%} of the bp notes repeat a pitch within 0.25 s (solo piano: below 10%)')
    stem_report(a.work)


def stem_report(work):
    import soundfile as sf
    if not (work / 'other4.wav').exists():
        print(f'an older work folder: other is the htdemucs stem, and {", ".join(LAYERS)} are missing; `step parts` makes them')
    level = levels(work, played_stems(work) + ['src'], np.arange(0, sf.info(work / 'src.wav').duration + 1))
    mix = level.pop('src')
    print(f'stems play (above {STEM_GATE_DB} dB of the mix top in 1 s frames): '
          + '; '.join(f'{stem} ' + (', '.join(f'{start}-{end}' for start, end in spans(l)) + ' s' if spans(l) else 'silent') for stem, l in level.items()))
    playing = np.flatnonzero(mix > STEM_GATE_DB)
    loud = np.flatnonzero(mix >= np.median(mix[playing]) - FADE_DB)[-1] + 1 if len(playing) else 0
    if len(playing) and playing[-1] + 1 - loud >= FADE_S:
        print(f'the mix fades out from {loud} s to {playing[-1] + 1} s')
    print('tuning, cents from A440: ' + ', '.join(f'{stem} {tuning(work, stem):+.0f}' for stem in ('backing', 'lead') if (work / f'{stem}.wav').exists()))


def tuning(work, stem):
    """Cents from A440: the circular mean of the distances of the spectral peaks from 80 Hz to 1.5 kHz to their semitones,
    weighted by magnitude."""
    # not librosa.estimate_tuning: its numba localmax crashed in the venv on float32 input (a damaged numba cache)
    from scipy.signal import stft
    x, sr = mono(work, stem)
    freqs, _, spectrum = stft(x, sr, nperseg=TUNING_FRAME, noverlap=TUNING_FRAME - TUNING_FRAME // 4)
    rows = np.flatnonzero((freqs >= 80) & (freqs < 1500))
    m = np.abs(spectrum[rows[0] - 1:rows[-1] + 2])
    left, middle, right = (np.log(m[k:len(m) - 2 + k] + 1e-12) for k in range(3))
    peak = (middle > left) & (middle >= right) & (m[1:-1] > 0.2 * m.max(0))
    hz = (rows[:, None] + 0.5 * (left - right) / (left - 2 * middle + right - 1e-12)) * sr / TUNING_FRAME
    turn = np.sum(m[1:-1][peak] * np.exp(2j * np.pi * 12 * np.log2(hz[peak] / 440)))
    return 100 * float(np.angle(turn)) / (2 * np.pi)


# ---------------------------------------------------------------- sung tune

def vocal_f0(work):
    """Frame times, MIDI pitch (nan where not sung), level in dB and level above BRIGHT_HZ in dB of lead.wav."""
    import librosa
    if not (work / 'lead.wav').exists():
        sys.exit(f'{work / "lead.wav"} is missing: run `prepare`, or `step lead` for a folder made before lead.wav')
    y, _ = librosa.load(work / 'lead.wav', sr=F0_SR, mono=True)
    cache = work / 'f0_lead.npy'
    if cache.exists():
        f0 = np.load(cache)
    else:
        f0, voiced, _ = librosa.pyin(y, sr=F0_SR, frame_length=F0_FRAME, hop_length=F0_HOP, fmin=F0_MIN, fmax=F0_MAX, resolution=F0_STEP)
        f0 = np.where(voiced, f0, np.nan)
        np.save(cache, f0)
    level = lambda signal: 20 * np.log10(librosa.feature.rms(y=signal, frame_length=F0_FRAME, hop_length=F0_HOP)[0] + 1e-9)
    stem, mix = level(y), level(librosa.load(work / 'src.wav', sr=F0_SR, mono=True)[0])
    # the level of the mix, not of the stem: a stem with no voice has only bleed, and all of it is near its own top level
    sung = stem > np.percentile(mix, 99) + SUNG_GATE_DB
    power = np.abs(librosa.stft(y, n_fft=BRIGHT_FRAME, hop_length=F0_HOP)[BRIGHT_HZ * BRIGHT_FRAME // F0_SR:]) ** 2
    return np.arange(len(f0)) * F0_HOP / F0_SR, np.where(sung, 69 + 12 * np.log2(f0 / 440), np.nan), stem, 10 * np.log10(power.sum(axis=0) + 1e-12)


def semitone_path(pitch):
    """The cheapest sequence of semitones for one sung run: squared distance to the note, plus CHANGE for each new note."""
    states = np.arange(int(np.floor(pitch.min())) - 1, int(np.ceil(pitch.max())) + 2)
    emit = np.minimum((pitch[:, None] - states[None]) ** 2, 9.0) / (2 * SIGMA ** 2)
    cost, back = emit[0].copy(), np.zeros(emit.shape, int)
    for f in range(1, len(pitch)):
        best = int(cost.argmin())
        stay = cost <= cost[best] + CHANGE
        back[f] = np.where(stay, np.arange(len(states)), best)
        cost = np.where(stay, cost, cost[best] + CHANGE) + emit[f]
    path = [int(cost.argmin())]
    for f in range(len(pitch) - 1, 0, -1):
        path.append(back[f, path[-1]])
    return states[path[::-1]]


def at_dips(bright, start, end, least):
    """[start, end) cut at each dip of DIP_DB or more in the bright level, at least `least` frames from the ends."""
    from scipy.signal import find_peaks
    dips, _ = find_peaks(-bright[start:end], prominence=DIP_DB, distance=least)
    cuts = [start, *(start + int(d) for d in dips if least <= d <= end - start - least), end]
    return list(zip(cuts, cuts[1:]))


def syllable_at(step, start, end, times, bright):
    """True when the bright level within GAP_S of `step` is DIP_DB below its top in [start, step) and in [step, end)."""
    part = lambda a, b: bright[(times >= a) & (times < b)]
    return min(part(start, step).max(), part(step, end).max()) - part(step - GAP_S, step + GAP_S).min() >= DIP_DB


def join_scoops(notes, times, bright):
    """A short note that steps up into the next note with no gap is the start of that note: the singer scoops into it, and the
    beat is at the scoop. When a syllable starts at the step, the short note is a note of its own.
    Notes as (onset s, end s, MIDI pitch, starts with a scoop)."""
    out, i = [], 0
    while i < len(notes):
        on, off, p = notes[i]
        if i + 1 < len(notes):
            on2, off2, p2 = notes[i + 1]
            short = off - on < SCOOP_S.get(p2 - p, 0)
            if short and on2 - off <= GAP_S and not syllable_at(on2, on, off2, times, bright):
                out.append((on, off2, p2, True))
                i += 2
                continue
        out.append((on, off, p, False))
        i += 1
    return out


def loud_starts(notes, times, level):
    """A note after a gap, or after a dip in one pitch, starts where its level comes within START_DB of its top:
    pYIN hears the pitch before the vowel is loud."""
    out = []
    for on, off, p, scoop in notes:
        prev = out[-1] if out else None
        after_gap = prev is None or on - prev[1] > GAP_S
        after_dip = not after_gap and prev[2] == p
        if after_gap or after_dip:
            frames = np.flatnonzero((times >= on - 0.005) & (times < min(off, on + START_REACH_S)))
            if len(frames):
                loud = float(times[frames[level[frames] >= level[frames].max() - START_DB][0]])
                on = loud if off - loud >= MIN_S else on
        out.append((on, off, p, scoop))
    return out


def soft_notes(notes, times, level):
    """The notes with a flag: SOFT_DB or more below the loudest note that starts within SOFT_REACH_S."""
    peaks = [level[(times >= on) & (times < off)].max(initial=-120.0) for on, off, *_ in notes]
    out = []
    for note, peak in zip(notes, peaks):
        near = [other for (on, *_), other in zip(notes, peaks) if abs(on - note[0]) <= SOFT_REACH_S]
        out.append((*note, max(near) - peak >= SOFT_DB))
    return out


def octave_errors(notes, work):
    """The notes with the doubt flag also set where the vocal stem is OCTAVE_DB weaker at the pitch than an octave higher."""
    spectrum, times = cqt(work / 'lead.wav', work / 'cqt_lead.npy')
    out = []
    for on, off, pitch, scoop, doubt, detune in notes:
        frames = (times >= on) & (times < off)
        k = pitch - LOWEST_MIDI
        if frames.any() and 0 <= k < len(spectrum) - 12:
            below = np.median(20 * np.log10((spectrum[k, frames] + 1e-9) / (spectrum[k + 12, frames] + 1e-9)))
            doubt = doubt or below <= -OCTAVE_DB
        out.append((on, off, pitch, scoop, doubt, detune))
    return out


def sung_notes(times, pitch, level, bright):
    """Notes as (onset s, end s, MIDI pitch, starts with a scoop, doubtful: much softer than the notes near it, between())."""
    hop = times[1] - times[0]
    least = round(MIN_S / hop)
    voiced = ~np.isnan(pitch)
    if not voiced.any():
        return []
    pitch = in_tune(pitch)
    edges = np.flatnonzero(np.diff(np.concatenate([[0], voiced.astype(int), [0]])))
    notes = []
    for a, b in zip(edges[::2], edges[1::2]):
        if b - a < least:
            continue
        path = semitone_path(pitch[a:b])
        cuts = [0, *(np.flatnonzero(np.diff(path)) + 1), b - a]
        parts = [[a + s, a + e, int(path[s])] for s, e in zip(cuts, cuts[1:])]
        while len(parts) > 1:
            k = min(range(len(parts)), key=lambda i: parts[i][1] - parts[i][0])
            if parts[k][1] - parts[k][0] >= least:
                break
            j = min((i for i in (k - 1, k + 1) if 0 <= i < len(parts)), key=lambda i: abs(parts[i][2] - parts[k][2]))
            parts[j][0], parts[j][1] = min(parts[j][0], parts[k][0]), max(parts[j][1], parts[k][1])
            del parts[k]
        joined = [parts[0]]
        for part in parts[1:]:
            if part[2] == joined[-1][2]:
                joined[-1][1] = part[1]
            else:
                joined.append(part)
        notes += [(times[s], times[e - 1] + hop, p) for start, end, p in joined for s, e in at_dips(bright, start, end, least)]
    notes = soft_notes(loud_starts(join_scoops(notes, times, bright), times, level), times, level)
    return [(*note, between(note, times, pitch)) for note in notes]


def in_tune(pitch):
    """The pitch moved by the circular mean of its distances to the nearest semitone: the singer or the recording can be off A440."""
    voiced = pitch[~np.isnan(pitch)]
    return pitch - np.angle(np.exp(2j * np.pi * (voiced - np.round(voiced))).mean()) / (2 * np.pi)


def sung_name(pitch):
    """The semitone of a sung pitch, with + or - when it is 30 cents or more above or below it; . when not sung."""
    if np.isnan(pitch):
        return '.'
    near = int(round(pitch))
    return name(near) + ('+' if pitch - near >= 0.3 else '-' if pitch - near <= -0.3 else '')


def contour(bar, grid, times, pitch, dips):
    """The sung pitch of each slot of the bar in 4 steps, equal steps joined; ' marks a step with a syllable start (a dip)."""
    cells = []
    for slot in range(grid.count(bar) * grid.slots):
        steps = []
        for k in range(4):
            start, end = (grid.time(grid.first(bar) + (slot + x / 4) / grid.slots) for x in (k, k + 1))
            inside = pitch[(times >= start) & (times < end)]
            sung = inside[~np.isnan(inside)]
            text = sung_name(np.median(sung)) if len(sung) and len(sung) >= len(inside) / 2 else '.'
            steps.append(("'" if ((dips >= start) & (dips < end)).any() else '') + text)
        joined = [step for i, step in enumerate(steps) if i == 0 or step != steps[i - 1]]
        if joined != ['.']:
            cells.append(f'{slot}[{" ".join(joined)}]')
    return ' '.join(cells)


def fine_rows(bar, grid, times, pitch, level, bright):
    """Each slot of the bar in 3 steps: the sung pitch, and the level and the level above BRIGHT_HZ in dB below their 99th percentile."""
    top, bright_top = np.percentile(level, 99), np.percentile(bright, 99)
    rows = []
    for slot in range(grid.count(bar) * grid.slots):
        steps = []
        for k in range(3):
            start, end = (grid.time(grid.first(bar) + (slot + x / 3) / grid.slots) for x in (k, k + 1))
            inside = (times >= start) & (times < end)
            sung = pitch[inside][~np.isnan(pitch[inside])]
            text = sung_name(np.median(sung)) if len(sung) > inside.sum() / 2 else '.'
            steps.append(f'{text} {level[inside].max(initial=-120) - top:.0f}/{bright[inside].max(initial=-120) - bright_top:.0f}')
        rows.append(f'    {slot}: ' + '  '.join(f'{step:<13}' for step in steps).rstrip())
    return rows


def between(note, times, pitch):
    """+1 or -1 when the middle half of the note is BETWEEN_C or more above or below its semitone, else 0."""
    sung = pitch[(times >= note[0]) & (times < note[1])]
    sung = sung[~np.isnan(sung)]
    quarter = len(sung) // 4
    cents = 100 * (np.median(sung[quarter:len(sung) - quarter]) - note[2]) if len(sung) else 0.0
    return int(np.sign(cents)) if abs(cents) >= BETWEEN_C else 0


def sung_width(grid):
    """Sixteenths in a slot of the sung tune: 2 in a fast song."""
    return 2 if float(np.median(np.diff(grid.beats))) / grid.slots < EIGHTH_S else 1


def on_grid(notes, grid, width=1):
    """Notes as [slot, sixteenths, pitch, marks], one for each slot of `width` sixteenths; marks are ^ (scoop), ~ (onset late in its slot),
    + or - (sung between the pitch and the semitone above or below) and ? (doubtful).
    Of two notes that round to one slot, the later one moves a slot on when it starts after the slot; else the longer one stays."""
    out = []
    for on, off, pitch, scoop, soft, detune in notes:
        start = grid.slot(on) / width
        note = [int(round(start)), int(round(grid.slot(off) / width)), pitch, off - on < WEAK_S or soft, off - on, scoop, start, detune]
        if out and out[-1][0] == note[0]:
            if start > note[0] and off - on >= WEAK_S / 2:
                note[0] += 1
            elif out[-1][4] >= off - on:
                continue
            else:
                out.pop()
        out.append(note)
    reliable = [i for i, note in enumerate(out) if not note[3]]
    marked = []
    for i, (slot, end, pitch, weak, _, scoop, start, detune) in enumerate(out):
        length = max(1, min(end, out[i + 1][0] if i + 1 < len(out) else end) - slot)
        before, after = [j for j in reliable if j < i][-3:], [j for j in reliable if j > i][:3]
        near = [out[j][2] for j in before + after]
        leap = len(near) > 1 and abs(pitch - float(np.median(near))) >= LEAP
        marks = ('^' if scoop else '') + ('~' if start - slot > LATE else '') + {1: '+', -1: '-', 0: ''}[detune] + ('?' if weak or leap else '')
        marked.append([slot * width, length * width, pitch, marks])
    return marked


def line_bars(notes, grid, octave):
    """on_grid notes as {bar: [(slot in the bar, sixteenths, pitch, marks)]}, and the count of notes before bar 1 (`roll` does not show them)."""
    bars, early = {}, 0
    for slot, length, pitch, marks in notes:
        if slot < 0:
            early += 1
            continue
        bar, place = grid.place(slot)
        bars.setdefault(bar, []).append((place, length, pitch + 12 * octave, marks))
    return bars, early


def top_line(notes, floor):
    """The highest note of each slot among the (slot, pitch) notes from `floor` up."""
    top = {}
    for slot, pitch in notes:
        if pitch >= floor:
            top[slot] = max(pitch, top.get(slot, 0))
    return sorted(top.items())


def parallel_bars(line, other, grid, octaves, share, least, instrument):
    """(bar, other bar, octaves up) where a bar of `line` and a bar of `other` have the same phrase; both are (slot, pitch) notes.
    Two bars pass when `share` of the notes of the bar are in the other bar, the octaves higher and at most PAR_REACH slots away,
    and PAR_SHARE of the notes of the other bar are in the bar. Of an instrument, the other bar is its top line from the lowest
    note of the bar: the chords below it are not the phrase. Bars count in a run of 2 or more at one distance and octave that
    shares PAR_NOTES notes. With an instrument, one bar between two passing bars is in the run: a bar where the voice is 2 slots
    from the instrument does not pass, and that bar needs the instrument most."""
    near = lambda notes: {(slot + d, pitch) for slot, pitch in notes for d in range(-PAR_REACH, PAR_REACH + 1)}

    def bars(notes):
        groups = {}
        for note in notes:
            groups.setdefault(grid.place(note[0])[0], []).append(note)
        return groups

    line_near, other_near, other_bars, shared = near(line), near(other), bars(other), {}
    for a, sung in bars(line).items():
        if len(sung) < least:
            continue
        low = min(pitch for _, pitch in sung)
        for b, played in other_bars.items():
            if a == b and not instrument:
                continue
            for octave in octaves:
                shift, up = (grid.first(b) - grid.first(a)) * grid.slots, 12 * octave
                played_line = top_line(played, low + up) if instrument else played
                here = sum((slot + shift, pitch + up) in other_near for slot, pitch in sung)
                there = sum((slot - shift, pitch - up) in line_near for slot, pitch in played_line)
                if here >= share * len(sung) and len(played_line) >= least and there >= PAR_SHARE * len(played_line):
                    shared[a, b, octave] = here
    passes = lambda a, b, octave: (a, b, octave) in shared
    in_run = lambda a, b, octave: passes(a, b, octave) or (instrument and passes(a - 1, b - 1, octave) and passes(a + 1, b + 1, octave))
    out = []
    for a, b, octave in shared:
        if in_run(a - 1, b - 1, octave):
            continue
        run = 1
        while in_run(a + run, b + run, octave):
            run += 1
        if run >= 2 and sum(shared.get((a + i, b + i, octave), 0) for i in range(run)) >= PAR_NOTES:
            out += [(a + i, b + i, octave) for i in range(run)]
    return sorted(out)


def sung_again(notes, grid):
    """Bar pairs where the voice sings one phrase again; each pair in both orders."""
    sung = [(slot, pitch) for slot, _, pitch, _ in notes if slot >= 0]
    return [(a, b) for a, b, _ in parallel_bars(sung, sung, grid, [0], PAR_SHARE, 3, False)]


def played_phrases(notes, work, grid):
    """(sung bar, bar of the instrument, octaves that the instrument is higher) where the shown notes of the models have the sung
    phrase, and the model letters of each shown note by (slot, pitch). The doubtful sung notes do not count."""
    sure = [(slot, pitch) for slot, _, pitch, marks in notes if slot >= 0 and '?' not in marks]
    played = {(round(n[0]), n[2]): n[6] for n in merged(viewed(combined(work), 'mix'), grid) if n[7] and round(n[0]) >= 0}
    return parallel_bars(sure, sorted(played), grid, range(-2, 3), INST_SHARE, 2, True), played


def cmd_tune(a):
    from scipy.signal import find_peaks
    grid = saved_grid(a.work)
    width = sung_width(grid)
    times, pitch, level, bright = f0 = vocal_f0(a.work)
    notes = on_grid(octave_errors(sung_notes(*f0), a.work), grid, width)
    if not notes:
        sys.exit('no sung notes in lead.wav')
    bars, early = line_bars(notes, grid, a.octave)
    again = sung_again(notes, grid)
    doubled, played = played_phrases(notes, a.work, grid)
    pitches = sorted(n[2] for n in notes)
    count = lambda mark: sum(mark in n[3] for n in notes)
    print(f'{len(notes) - early} sung notes in bars {bar_ranges(sorted(bars))}' + (f' and {early} before bar 1' if early else '') + f'; sung range {name(pitches[0])}-{name(pitches[-1])}, median {name(pitches[len(pitches) // 2])}'
          + (f', printed {a.octave:+d} octave' if a.octave else '') + f'; {count("^")} scoops (^), {count("~")} late in the slot (~), {count("?")} doubtful (?)')
    print(f'bar | slot:pitch/sixteenths, {grid.per_bar * grid.slots} slots in a bar; ^ = starts with a scoop from below, written at the scoop; '
          f'~ = the onset is late in its slot; ' + ('the song is fast: the notes are on eighths (even slots); ' if width == 2 else '') + f'+ or - = sung {BETWEEN_C} cents or more above or below the pitch; ? = shorter than {WEAK_S} s, {SOFT_DB} dB softer than a note near it, an octave error, or an octave from the notes around it')
    if again:
        print('=N after a bar: the voice sings the phrase of the bar again in bar N')
    if doubled:
        print('`inst N` under a bar: an instrument plays the phrase of the bar in bar N; its notes as slot:pitch in the octave of the tune, with the model letters of `roll`')
    tuned = in_tune(pitch) + 12 * a.octave
    dips = times[find_peaks(-bright, prominence=DIP_DB, distance=round(MIN_S / (times[1] - times[0])))[0]]
    if a.contour:
        print("`f0` under a bar: the sung pitch of each slot in 4 steps, slot[pitches]; + or - = 30 cents or more above or below; ' = a syllable starts")
    if a.fine:
        print(f'under a bar, each slot in 3 steps: pitch level/bright, in dB below the 99th percentile of lead.wav; bright is the level above {BRIGHT_HZ} Hz: a consonant lowers it')
    for bar in bar_list(a.bars) if a.bars else sorted(bars):
        same = [str(b) for x, b in again if x == bar]
        print(f'{grid.label(bar, grid.slots)} | ' + ' '.join(f'{slot}:{name(p)}/{length}{marks}' for slot, length, p, marks in bars.get(bar, []))
              + (' | =' + ','.join(same) if same else ''))
        sure = [p for _, _, p, marks in bars.get(bar, []) if '?' not in marks]
        instrument = [(b, octave) for x, b, octave in doubled if x == bar]
        if sure and instrument:
            b, octave = instrument[0]
            there = top_line([n for n in played if grid.place(n[0])[0] == b], min(sure) + 12 * (octave - a.octave))
            print(f'  inst {b} | ' + ' '.join(f'{grid.place(slot)[1]}:{name(p + 12 * (a.octave - octave))}{letters(played[slot, p])}' for slot, p in there))
        if a.contour:
            print(f'  f0 | {contour(bar, grid, times, tuned, dips)}')
        if a.fine:
            print('\n'.join(fine_rows(bar, grid, times, tuned, level, bright)))
    melody = {f'{bar}:{round(1 + slot / grid.slots, 3):g}': name(p) for bar in sorted(bars) for slot, _, p, marks in bars[bar] if '?' not in marks}
    (a.work / 'tune.json').write_text(json.dumps(dict(melody=melody), indent=1))
    print(f'draft melody without the doubtful notes: {(a.work / "tune.json").as_posix()} ({len(melody)} notes)')


# ---------------------------------------------------------------- bass line

def bass_line(work, grid):
    """The bass stem as [slot, sixteenths, pitch, marks] of on_grid. Empty when the stem is silent."""
    return on_grid([(n['on'], n['off'], n['p'], False, False, 0) for n in model_notes(work, 'bass')], grid)


def cmd_bass(a):
    grid = saved_grid(a.work)
    bars, _ = line_bars(bass_line(a.work, grid), grid, a.octave)
    if not bars:
        sys.exit('the bass stem is silent: read the bass from `roll`')
    print(f'bass stem: notes in bars {bar_ranges(sorted(bars))}; bar | slot:pitch/sixteenths, {grid.per_bar * grid.slots} slots in a bar; '
          f'~ = the onset is late in its slot; ? = shorter than {WEAK_S} s, or an octave from the notes around it')
    for bar in bar_list(a.bars) if a.bars else sorted(bars):
        print(f'{grid.label(bar, grid.slots)} | ' + ' '.join(f'{slot}:{name(p)}/{length}{marks}' for slot, length, p, marks in bars.get(bar, [])))


# ---------------------------------------------------------------- grid

def repair(beats):
    """Merge two beats that are too near, bring all beats to one metric level, drop a beat between two beats, and fill gaps.
    Returns the beats, and the times of the beats that were added (None when the tracker had two levels: then half of them are added)."""
    kept = [float(beats[0])]
    rough = float(np.median(np.diff(beats)))
    for b in map(float, beats[1:]):
        if b - kept[-1] < 0.25 * rough:               # the tracker gave one beat as two peaks
            kept[-1] = (kept[-1] + b) / 2
        else:
            kept.append(b)
    gaps = np.diff(kept)
    period = float(np.median(gaps))
    ratio = gaps / period
    two_levels = np.mean(np.minimum(np.abs(ratio - 0.5), np.abs(ratio - 1.5)) <= LEVEL_REACH) >= LEVEL_SHARE
    if two_levels:
        period /= 2                                   # use the fast one
    out, recent, added = [kept[0]], [period], []
    for i in range(1, len(kept)):
        local = float(np.median(recent[-4:]))
        ratio = (kept[i] - out[-1]) / local
        if ratio < EXTRA_BEAT and i + 1 < len(kept) and (kept[i + 1] - out[-1]) / local < 1.4:
            continue                                  # a beat between two beats of the grid
        count = round(ratio) if ratio >= 1.7 else 1   # rubato can make one beat 1.6 times as long
        step = (kept[i] - out[-1]) / count
        added += [out[-1] + step * k for k in range(1, count)]
        out += [out[-1] + step * k for k in range(1, count)] + [kept[i]]
        recent.append(step)
    return np.array(out), None if two_levels else added


def cover(beats, first, last):
    """Add beats at the local tempo until the grid starts before `first` and ends after `last`."""
    beats = list(beats)
    head, tail = float(np.median(np.diff(beats[:5]))), float(np.median(np.diff(beats[-5:])))
    while beats[0] > first + 0.25 * head:
        beats.insert(0, beats[0] - head)
    while beats[-1] < last:
        beats.append(beats[-1] + tail)
    return np.array(beats)


def snap(beats, onsets, weights, reach=0.04):
    """Move each beat to the weighted mean of the note onsets next to it (the tracker has steps of 20 ms)."""
    out = []
    for b in beats:
        close = np.abs(onsets - b) <= reach
        out.append(float(np.average(onsets[close], weights=weights[close])) if close.any() else float(b))
    return np.array(out)


def phase_path(phase, per_bar):
    """The beat of the bar that the bars start on at each tracked downbeat: the path with the fewest downbeats off it, where a move
    to another beat costs PHASE_MOVE downbeats. Of two equal paths, the one that moves later."""
    off = phase[:, None] != np.arange(per_bar)
    cost, back = off[0].astype(float), []
    for row in off[1:]:
        moved = cost.min() + PHASE_MOVE
        back.append(np.where(cost < moved, np.arange(per_bar), cost.argmin()))
        cost = np.minimum(cost, moved) + row
    path = [int(cost.argmin())]
    for step in reversed(back):
        path.append(int(step[path[-1]]))
    return np.array(path[::-1])


def bar_phase(beats, downbeats, first_onset):
    """Beats in a bar, the index of the beat that starts bar 1 (the bar with the first note), and the share of downbeats that agree."""
    index = np.array([int(np.abs(beats - d).argmin()) for d in downbeats])
    per_bar = int(np.median(np.diff(index)))
    path = phase_path(index % per_bar, per_bar)
    first_beat = int(np.searchsorted(beats, first_onset + 0.25 * np.median(np.diff(beats)), side='right')) - 1
    phase = int(path[min(np.searchsorted(index, first_beat), len(path) - 1)])
    return per_bar, first_beat - (first_beat - phase) % per_bar, float(np.mean(index % per_bar == path))


def line_evidence(work, beats, downbeats):
    """For each beat, how much it is a bar line: the share of the chord probabilities that changes at the beat, and PLACE_DOWNBEAT
    for a tracked downbeat (`downbeats` has their beat indexes)."""
    _, frames = chord_frames(work)
    frame = lambda seconds: min(round(max(seconds, 0) * CHORD_SR / CHORD_HOP), len(frames) - 1)
    chords = np.array([frames[frame(a):max(frame(b), frame(a) + 1)].mean(0) for a, b in zip(beats, beats[1:])])
    evidence = PLACE_DOWNBEAT * np.isin(np.arange(len(beats)), downbeats)
    evidence[1:-1] += 1 - np.minimum(chords[:-1], chords[1:]).sum(1)
    return evidence


def odd_bars(work, beats, downbeats, per_bar, first):
    """{bar: beats} of the bars that do not have per_bar beats. Where the tracked downbeats move to another beat of the bar and stay
    there, one bar is shorter or longer: of the bars up to PLACE_REACH bars from the move, the one that puts the most evidence on
    the bar lines around it. A bar has 2 to per_bar + 1 beats. `first` is the index of the beat that starts bar 1."""
    index = np.unique([int(np.abs(beats - d).argmin()) for d in downbeats])
    phase = (index - first) % per_bar
    path = phase_path(phase, per_bar)
    lines, state = index[phase == path], path[phase == path]
    if (state == state[0]).all():
        return {}
    evidence = line_evidence(work, beats, index)
    on_lines = lambda start, end: sum(evidence[line] for line in range(start, end + 1, per_bar) if 0 <= line < len(beats))
    out = [int(lines[0])]
    for a, b, same in zip(lines, lines[1:], state[:-1] == state[1:]):
        if same:
            out += range(out[-1] + per_bar, b + 1, per_bar)
            continue
        after = lambda last: b + per_bar * -((b - last - 2) // per_bar)        # the first bar line of the new part: 2 beats or more after `last`
        tracked = a + per_bar * ((b - 2 - a) // per_bar)                       # regular bars as far as they go before the move
        places = [tracked + per_bar * k for k in range(-PLACE_REACH, PLACE_REACH + 1)]
        last = max(places, key=lambda place: (on_lines(places[0], place) + on_lines(after(place), after(places[-1])), -abs(place - tracked)))
        out = [line for line in out if line < places[0]] or places[:1]
        out += [*range(out[-1] + per_bar, last + 1, per_bar), *range(after(last), max(b, after(last)) + 1, per_bar)]
    # the bars of the part with bar 1 start on beat `own` of the bars of the grid, when a field moved bar 1
    own = int(state[max(np.searchsorted(lines, first, side='right') - 1, 0)])
    out = np.array(out) - own + per_bar * (own > per_bar // 2)
    out = np.r_[first, np.arange(out[0] - per_bar, first, -per_bar)[::-1], out[out > first]]
    return {bar: int(count) for bar, count in enumerate(np.diff(out), 1) if count != per_bar}


def tracked_grid(work, fields):
    """The grid after the grid fields, as grid.json has it (beats from beat 1 of bar 1), and a dict with what `grid` reports."""
    notes = [n for n in combined(work) if len(n['by']) > 1]      # onsets that two or more models have
    on = np.array([n['on'] for n in notes])
    weight = np.array([n['v'] or n['amp'] or 1 for n in notes], float)
    tracked = json.loads((work / 'beats.json').read_text())
    if not notes:
        sys.exit('no note onset that two models have: there is nothing to fit a grid to')
    if len(tracked['downbeats']) < 2:
        sys.exit('the beat tracker found fewer than two bars: the recording is too short or has no steady beat')
    beats, added = repair(np.array(tracked['beats']))
    beats = cover(beats, on.min(), on.max())
    downbeats = np.array(tracked['downbeats'])
    per_bar, first, agree = bar_phase(beats, downbeats, on.min())
    quarters, declared = 1, {}
    for pair in fields:
        key, _, value = pair.partition('=')
        if key == 'beats_per_bar':
            per_bar = int(value)
        elif key == 'shift':                        # bar 1 starts this number of beats later (negative: earlier)
            first += int(value)
        elif pair in ('level=half', 'level=third'): # the true beat is two or three tracked beats
            count = 2 if value == 'half' else 3
            beats, first = beats[first % count::count], first // count
        elif pair == 'level=double':                # the true beat is half a tracked beat
            beats, first = np.sort(np.r_[beats, (beats[:-1] + beats[1:]) / 2]), first * 2
        elif key == 'quarters_per_beat':
            quarters = float(value)
        elif re.fullmatch(r'bar=[1-9]\d*(-\d+)?:[1-9]\d*', pair):   # bar=33:2 or bar=59-66:3: these bars have this number of beats
            bars, _, count = value.partition(':')
            declared |= dict.fromkeys(bar_list(bars), int(count))
        else:
            sys.exit(f'unknown grid field {pair}; fields: {GRID_FIELDS}')
    while first < 0:
        beats, first = np.r_[2 * beats[0] - beats[1], beats], first + 1
    found = odd_bars(work, beats, downbeats, per_bar, first)
    # bar fields give all the bars of another length: each one moves the later bars, also away from the place where a bar was found
    bars = {bar: count for bar, count in sorted((declared or found).items()) if count != per_bar}
    grid = dict(beats_per_bar=per_bar, quarters_per_beat=quarters, **({'bars': bars} if bars else {}), beats=snap(beats[first:], on, weight))
    return grid, dict(agree=agree, added=added, on=on, weight=weight, found=found)


def drum_fit(times, strength, beat_s, start, eighths):
    """The constant grid (beat s, time of beat 0) fitted to the strong drum hits near its eighth notes (`eighths` in a beat: 2, or 3
    in a compound beat), from the given grid, and the count of the hits that fit; None for fewer than 16."""
    strong = strength > STRONG_HIT
    for _ in range(8):
        eighth = np.round((times - start) / (beat_s / eighths))
        fits = strong & (np.abs(times - start - eighth * beat_s / eighths) < FIT_REACH_S)
        if fits.sum() < 16:
            return None
        beat_s, start = np.linalg.lstsq(np.c_[eighth[fits] / eighths, np.ones(fits.sum())], times[fits], rcond=None)[0]
    return float(beat_s), float(start), int(fits.sum())


def sixteenth_offsets(beats, times, slots):
    """Seconds from each time to the nearest sixteenth of the beats (`slots` in a beat)."""
    position = slots * beat_at(beats, times)
    return (position - np.round(position)) * float(np.median(np.diff(beats))) / slots


def drift(grid, beats, onsets):
    """The mean offset in ms of the onsets from the sixteenths of the beats (a circular mean, weighted by strength), in windows of
    DRIFT_BARS bars from bar 1; '--' for a window with fewer than 8 onsets."""
    times, strength = onsets[:2]
    position = beat_at(beats, times)
    window = np.array([(grid.bar_at(p)[0] - 1) // DRIFT_BARS for p in position], int)
    sixteenth_ms = 1000 * float(np.median(np.diff(beats))) / grid.slots
    out = []
    for w in range(max(window.max(initial=0), 0) + 1):
        inside = window == w
        turn = np.sum(strength[inside] * np.exp(2j * np.pi * grid.slots * position[inside]))
        out.append(f'{w * DRIFT_BARS + 1}:' + (f'{np.angle(turn) / (2 * np.pi) * sixteenth_ms:+.0f}' if inside.sum() >= 8 else '--'))
    return ' '.join(out)


def backbeat(grid, beats, drums):
    """Where the kick and the snare fall in the bars of the grid, and what the snare says of the bar start. The kick does not tell
    beat 1 from beat 3: in 22 RWC songs it was stronger on beat 3 in 6."""
    times, strength, kick_rise, snare_rise = drums
    strong = strength > STRONG_HIT
    kick = strong & (kick_rise > 0.5) & (kick_rise > snare_rise)
    snare = strong & (snare_rise > 0.5) & (snare_rise > kick_rise)
    places = [grid.bar_at(p) for p in beat_at(beats, times)]
    regular = np.array([bar >= 1 and bar not in grid.odd for bar, _ in places])
    eighth = np.array([round(2 * x) for _, x in places], int) % (2 * grid.per_bar)
    on_beat = eighth % 2 == 0
    count = lambda hits: np.bincount(eighth[hits & regular & on_beat] // 2, minlength=grid.per_bar)
    kicks, snares, between = count(kick), count(snare), int((snare & regular & ~on_beat).sum())
    text = ('backbeat, kick/snare hits on each beat of the bar: ' + ' '.join(f'{b + 1}:{k}/{s}' for b, (k, s) in enumerate(zip(kicks, snares)))
            + f', snare between the beats {between}')
    if snares.sum() + between < 16 or grid.per_bar % 2:
        return text
    if between > snares.sum():
        return text + ': the snare is between the beats: the beat can be two times too slow (level=double)'
    if snares.sum() < 16:
        return text
    if np.sort(snares)[-1] >= 3 * np.sort(snares)[-2]:
        return text + f': the snare is on beat {snares.argmax() + 1} only: the beat can be two times too fast (level=half), or the drums play half time'
    beats_of = lambda first: ' and '.join(map(str, range(first, grid.per_bar + 1, 2)))
    if snares[1::2].sum() >= 2 * snares[::2].sum():
        return text + f': the snare is on beats {beats_of(2)}: the bars start right, or {grid.per_bar // 2} beats off (the drums do not tell which)'
    if snares[::2].sum() >= 2 * snares[1::2].sum():
        return text + f': the snare is on beats {beats_of(1)}: the bars start one beat off: --set shift=1 or shift=-1'
    return text


def chord_change(work, grid):
    """How much the chords change at each beat of the bar (half the total variation of the chord probabilities of the beat before
    and the beat after it, a mean over the bars of per_bar beats), and what that says of the bar start; None without chords.npy."""
    if not (work / 'chords.npy').exists() or grid.per_bar < 2:
        return None
    _, frames = chord_frames(work)
    frame = lambda beat: round(max(grid.time(beat), 0) * CHORD_SR / CHORD_HOP)
    mean = lambda b0, b1: frames[frame(b0):max(frame(b1), frame(b0) + 1)].mean(0)
    reach, last = 1, grid.bar_at(len(grid.beats) - 1)[0]
    bars = [bar for bar in range(2, last) if bar not in grid.odd and frame(grid.first(bar + 1) + reach) <= len(frames)]
    if len(bars) < 8:
        return None
    change = np.array([np.mean([0.5 * np.abs(mean(b - reach, b) - mean(b, b + reach)).sum() for b in (grid.first(bar) + k for bar in bars)])
                       for k in range(grid.per_bar)])
    text = 'chord change at each beat of the bar: ' + ' '.join(f'{k + 1}:{short(c)}' for k, c in enumerate(change))
    top = int(change.argmax())
    if top and change[top] >= CHANGE_LEAD * change[0]:
        return text + f': the chords change most on beat {top + 1}: the bars can start {top} beat{"s" * (top > 1)} off: add shift={top} or shift={top - grid.per_bar} to the fields'
    return text + (': the chords change most on beat 1' if not top else '')


def cmd_grid(a):
    g, report = tracked_grid(a.work, a.set or [])
    grid = Grid(g)
    beats, per_bar, on, weight = grid.beats, grid.per_bar, report['on'], report['weight']
    period = float(np.median(np.diff(beats)))
    index = np.arange(len(beats))
    beat_s, t0 = np.polyfit(index, beats, 1)
    constant = t0 + beat_s * index
    apart = np.abs(beats - constant)
    far_bars = sorted({grid.bar_at(i)[0] for i in index[apart > period / 8]})
    tempo = 60 / np.diff(beats)
    bar_of = lambda seconds: grid.bar_at(grid.beat(seconds))[0]
    print(f'tracked grid: beat {period:.3f} s = {60 / period:.0f} BPM ({np.percentile(tempo, 5):.0f} to {np.percentile(tempo, 95):.0f}), '
          f'{per_bar} beats in a bar ({report["agree"]:.0%} of the tracked downbeats agree), bar 1 at {beats[0]:.2f} s, {grid.bar_at(len(beats) - 1)[0]} bars')
    found = ', '.join(f'{bar} ({count} beats)' for bar, count in report['found'].items())
    if any(field.startswith('bar=') for field in a.set or []):
        print('bars of another length: ' + (', '.join(map(grid.label, grid.odd)) or 'none')
              + f'; the bar fields replace the bars that the tracked downbeats give: {found}' * bool(found))
    elif found:
        print(f'bars of another length: {found}. The tracked downbeats give them; the true bar can be a bar or two earlier or later: '
              f'look at the bars around it with roll. bar= fields replace them all (bar=1:{per_bar}: no such bar)')
    if far_bars:
        print(f'constant grid: beat {beat_s:.4f} s from {t0:.3f} s; the tracked beats are more than half a sixteenth from it in '
              f'{len(far_bars)} bars ({bar_ranges(far_bars)}), at most {1000 * apart.max():.0f} ms: the tempo changes, use the tracked grid')
    else:
        print(f'constant grid: beat {beat_s:.4f} s from {t0:.3f} s; the tracked beats are less than half a sixteenth from it in all bars '
              f'(at most {1000 * apart.max():.0f} ms): steady, use --constant')
    if report['added'] is None:
        if not any(field.startswith('level=') for field in a.set or []):
            print('the tracker changed between two beat levels: the grid has the fast one (level=half gives the slow one)')
    else:
        added = [t for t in report['added'] if beats[0] <= t <= beats[-1]]
        if added:
            print(f'the tracker missed {len(added)} beats, added in bars {bar_ranges(sorted({bar_of(t) for t in added}))}: look at these bars with roll')
    position = beat_at(beats, on)
    lock = lambda division: abs(np.sum(weight * np.exp(2j * np.pi * position * division))) / weight.sum()
    print('onset lock by divisions of the beat: ' + ' '.join(f'{d}:{short(lock(d))}' for d in (1, 2, 3, 4, 6, 8, 12)))
    drums, other = stem_onsets(a.work, 'drums'), stem_onsets(a.work, 'other4')
    fit = drum_fit(drums[0], drums[1], beat_s, t0, grid.slots // 2)
    grids, saved_as = {'tracked grid': beats, 'constant grid': constant}, 'constant grid'
    if fit:
        fit_beat, fit_start, count = fit
        # the phase of the note onsets, as the tracked grid has it: in moored they were 12 ms before the drum hits
        fit_start += np.angle(np.sum(weight * np.exp(2j * np.pi * grid.slots * beat_at(fit_start + fit_beat * index, on)))) / (2 * np.pi) * fit_beat / grid.slots
        grids['drum fit'] = fit_start + fit_beat * index
        strong = drums[0][drums[1] > STRONG_HIT]
        miss = {label: [1000 * float(np.median(np.abs(sixteenth_offsets(b, times, grid.slots)))) for times in (strong, on)] for label, b in grids.items()}
        # the drums can fit another subdivision than the notes (swing, compound meter): then the notes are farther from the fit
        if miss['drum fit'][0] < miss['constant grid'][0] and miss['drum fit'][1] <= miss['constant grid'][1]:
            constant, saved_as = grids['drum fit'], 'drum fit'
        print(f'drum fit: beat {fit_beat:.4f} s from {fit_start:.3f} s (the beat of {count} drum hits, the phase of the note onsets); median offset from the sixteenths of the {len(strong)} strong '
              f'drum hits / the {len(on)} note onsets: ' + ', '.join(f'{label} {d:.0f}/{n:.0f} ms' for label, (d, n) in miss.items()) + f': --constant saves the {saved_as}')
    if len(drums[0]):
        print(backbeat(grid, beats, drums))
    harmony = chord_change(a.work, grid)
    if harmony:
        print(harmony)
    before = levels(a.work, played_stems(a.work, GRID_STEMS), np.arange(0, beats[0], LEVEL_S))
    sounding = [(np.flatnonzero(level > STEM_GATE_DB)[0] * LEVEL_S, stem) for stem, level in before.items() if (level > STEM_GATE_DB).any()]
    start, stem = min(sounding, default=(beats[0], ''))
    early = math.ceil((beats[0] - start - 0.25 * period) / (per_bar * period))
    if early > 0:
        print(f'{stem} starts at {start:.1f} s, {early} bar{"s" * (early > 1)} before bar 1: --set shift={-early * per_bar} starts bar 1 before it')
    print(f'drift, mean onset offset from the sixteenths in ms by {DRIFT_BARS} bars from bar 1 (+ = after the grid):')
    for label in grids:
        print(f'  {label}: ' + ' | '.join(f'{stem} {drift(grid, grids[label], onsets)}' for stem, onsets in (('drums', drums), ('other4', other)) if len(onsets[0])))
    if a.set is None and not a.constant:
        spectrum, times = source_cqt(a.work)
        profile = chroma(window(spectrum, times, 0, times[-1]))
        print('pitch classes, strongest first: ' + ' '.join(f'{NAMES[k]} {short(profile[k] / profile.max())}' for k in np.argsort(profile)[::-1][:8]))
        return
    chosen = constant if a.constant else beats
    (a.work / 'grid.json').write_text(json.dumps(g | dict(beats=[round(float(b), 4) for b in chosen])))
    print(f'saved grid.json: the {"constant" if a.constant else "tracked"} grid' + (' (the drum fit)' if a.constant and saved_as == 'drum fit' else '')
          + f', {per_bar} beats in a bar')


# ---------------------------------------------------------------- evidence

def merged(notes, grid):
    """Notes as [start slot, end slot, pitch, velocity, onsets, last onset slot, model letters, show], where fast re-onsets of one pitch
    are one note. An onset that two or more models have is a new note: only a sustained or tremolo note gives re-onsets in one model."""
    out, last = [], {}
    for n in sorted(notes, key=lambda n: (n['p'], n['on'])):
        start, end = grid.slot(n['on']), grid.slot(n['off'])
        prev = last.get(n['p'])
        if prev and len(n['by']) == 1 and (start - prev[1] <= 0.3 * grid.slots or start - prev[5] <= 0.35 * grid.slots):
            prev[1], prev[3], prev[4], prev[5] = max(prev[1], end), max(prev[3], n['v'] or 0), prev[4] + 1, start
            prev[7] = prev[7] or n['show']
            continue
        last[n['p']] = [start, end, n['p'], n['v'] or 0, 1, start, n['by'], n['show']]
        out.append(last[n['p']])
    return sorted(out, key=lambda note: (note[0], note[2]))


def letters(by):
    return '' if len(by) == len(MODELS) else by


def bar_bass(notes, bass, bar, grid):
    """The first note of the bass stem that starts in the bar; where the stem has no note, the lowest shown note that starts in beat 1
    (None when there is none)."""
    first = grid.first(bar) * grid.slots
    early = [pitch for start, _, pitch, *_, show in notes if show and first <= round(start) < first + grid.slots]
    return bass.get(bar) or (min(early) if early else None)


def chord_frames(work):
    """The chord names of `roll`, and their probability in each frame of chords.npy: each root with each triad
    (the sevenths and sixths of BTC join their triad), then no chord."""
    path = work / 'chords.npy'
    if not path.exists():
        sys.exit(f'{path} is missing: run `step chords`')
    p = np.load(path).astype(float)
    kinds = list(dict.fromkeys(CHORD_KINDS))
    types = p[:, :12 * len(CHORD_KINDS)].reshape(len(p), 12, -1)
    triads = np.stack([types[:, :, [k == kind for k in CHORD_KINDS]].sum(-1) for kind in kinds], -1).reshape(len(p), -1)
    return [NAMES[root] + kind for root in range(12) for kind in kinds] + ['-'], np.hstack([triads, 1 - triads.sum(1, keepdims=True)])


def chord_choices(names, frames, start, end):
    """The two most probable chords between two times, with their probabilities."""
    first, last = (round(max(seconds, 0) * CHORD_SR / CHORD_HOP) for seconds in (start, end))
    inside = frames[first:max(last, first + 1)]
    if not len(inside):
        return [('-', 1.0), ('-', 0.0)]
    mean = inside.mean(0)
    return [(names[k], float(mean[k])) for k in np.argsort(mean)[::-1][:2]]


def chord(names, frames, start, end):
    """The most probable chord between two times, with ? where its probability is below UNSURE."""
    (best, p), _ = chord_choices(names, frames, start, end)
    return best + ('?' if p < UNSURE else '')


def beat_chords(names, frames, bar, grid):
    """The chord of each beat of the bar with its probability, and the second chord where its probability is SECOND_CHORD or more."""
    cells = []
    for beat in range(grid.count(bar)):
        start = grid.first(bar) + beat
        (best, p), (other, q) = chord_choices(names, frames, grid.time(start), grid.time(start + 1))
        cells.append(f'{beat + 1} {best} {short(p)}' + (f' ({other} {short(q)})' if q >= SECOND_CHORD else ''))
    return ' | '.join(cells)


def bar_chord(names, frames, bar, grid):
    """The chord of the bar, or first>second where the two halves of a bar of an even meter have different chords."""
    edges = [grid.time(grid.first(bar) + k * grid.count(bar) / 2) for k in range(3)]
    halves = [chord(names, frames, a, b) for a, b in zip(edges, edges[1:])]
    if grid.count(bar) % 2 or halves[0].rstrip('?') == halves[1].rstrip('?'):
        return chord(names, frames, edges[0], edges[2])
    return '>'.join(halves)


def cmd_roll(a):
    grid = saved_grid(a.work)
    bass = {grid.place(slot)[0]: pitch for slot, _, pitch, _ in reversed(bass_line(a.work, grid))}
    notes = merged(shown_notes(a), grid)
    hidden = Counter(grid.place(round(n[0]))[0] for n in notes if not n[7])
    chords = chord_frames(a.work)

    def in_bar(notes, bar):
        first, end = grid.first(bar) * grid.slots, grid.first(bar + 1) * grid.slots
        return [(round(s) - first, e - s, p, v, k, by) for s, e, p, v, k, _, by, show in notes if show and first <= round(s) < end]

    def line(notes, lowest, highest):
        return ' '.join(f'{slot}:{name(p)}/{max(1, round(length))}' + ('!' if v >= STRONG else '') + (f'x{k}' if k >= 3 else '') + letters(by)
                        for slot, length, p, v, k, by in notes if lowest <= p < highest)

    print(f'bar chord bass | below {name(a.split)} | from {name(a.split)} | +hidden notes'
          + (f'; all notes of {a.stem}.wav' if a.stem else f'; view {a.view}: {VIEWS[a.view]}') + f'; slot:pitch/sixteenths, {grid.per_bar * grid.slots} in a bar')
    for bar in bar_list(a.bars):
        low = bar_bass(notes, bass, bar, grid)
        shown = in_bar(notes, bar)
        print(f'{grid.label(bar, grid.slots)} {bar_chord(*chords, bar, grid)} {name(low) if low else "-"} | {line(shown, 0, a.split)} | {line(shown, a.split, 128)}'
              + (f' | +{hidden[bar]}' if hidden[bar] else ''))
        if a.beats:
            print(f'  beats | {beat_chords(*chords, bar, grid)}')


def cmd_levels(a):
    stem_report(a.work)
    stems = played_stems(a.work) + ['src']
    by_bar = (a.work / 'grid.json').exists()
    if by_bar:
        grid = saved_grid(a.work)
        last = grid.bar_at(len(grid.beats) - 1)[0]
        level = levels(a.work, stems, [grid.bar_start(bar) for bar in range(1, last + 2)])
        rows = [(grid.label(bar), bar - 1) for bar in (bar_list(a.bars) if a.bars else range(1, last + 1)) if 1 <= bar <= last]
    else:
        import soundfile as sf
        level = levels(a.work, stems, np.arange(0, sf.info(a.work / 'src.wav').duration + LEVELS_S, LEVELS_S))
        rows = [(f'{k * LEVELS_S} s', k) for k in range(len(level['src']))]
    shown = lambda db: '-' if db < -60 else f'{db:.0f}'
    print(f'{"bar" if by_bar else "time"} | level in dB below the mix top: {" ".join(stems[:-1])} | mix (src); - = below -60 dB')
    for label, k in rows:
        print(f'{label} | ' + ' '.join(shown(level[stem][k]) for stem in stems[:-1]) + f' | {shown(level["src"][k])}')


def cmd_raw(a):
    grid = saved_grid(a.work)
    notes = shown_notes(a)
    print(f'bar | beat:pitch/beats(velocity, or a and the bp amplitude) | +hidden notes; ' + (f'all notes of {a.stem}.wav' if a.stem else f'view {a.view}'))
    for bar in bar_list(a.bars):
        first = grid.first(bar)
        row, hidden = [], 0
        for n in notes:
            beat = grid.beat(n['on']) - first
            level = n['amp'] if n['v'] is None else n['v']
            if not (-0.06 <= beat < grid.count(bar) - 0.06 and a.lo <= n['p'] <= a.hi and level >= a.min_velocity):
                continue
            if not n['show']:
                hidden += 1
                continue
            row.append(f"{beat + 1:.2f}:{name(n['p'])}/{grid.beat(n['off']) - grid.beat(n['on']):.1f}"
                       f"({'a' if n['v'] is None else ''}{level}){letters(n['by'])}")
        print(f'{grid.label(bar)} | ' + ' '.join(row) + (f' | +{hidden}' if hidden else ''))


def cmd_peek(a):
    grid = saved_grid(a.work)
    spectrum, times = source_cqt(a.work) if a.stem == 'src' else cqt(stem_wav(a.work, a.stem), a.work / f'cqt_{a.stem}.npy')
    source = 'the mix' if a.stem == 'src' else f'{a.stem}.wav'
    beats = [(beat, [window(spectrum, times, grid.time(grid.first(bar) + beat + k / 8), grid.time(grid.first(bar) + beat + (k + 1) / 8))
                     for k in range(8)])
             for bar in bar_list(a.bars) for beat in range(grid.count(bar))]
    top = max((column.max() for _, columns in beats for column in columns), default=0)
    if not top:
        sys.exit(f'{source} is silent in bars {a.bars}')
    print('one character is 1/8 beat; a space starts a beat, | starts a bar; from silent to loud: " .:-=+*#%@", '
          f'relative to the loudest pitch in these bars, {20 * np.log10(top / spectrum.max()):.0f} dB from the loudest moment of {source}')
    pitches = [midi(token) for token in a.pitches.split(',')]
    rows = {pitch: [] for pitch in pitches}
    for beat, columns in beats:
        for pitch in pitches:
            rows[pitch].append(('|' if beat == 0 else ' ') + ''.join(' .:-=+*#%@'[min(9, int(15 * (c[pitch - LOWEST_MIDI] / top) ** 0.5))] for c in columns))
    for pitch in pitches:
        print(name(pitch).ljust(4) + ''.join(rows[pitch]))


# ---------------------------------------------------------------- compare

def export(score, target):
    subprocess.run(['pno', 'export', str(score), str(target)], check=True, capture_output=True)
    if not target.exists():
        sys.exit(f'pno export did not write {target}')


def score_notes(path, grid):
    """Notes of the score as (start slot, end slot, pitch), and the seconds of one beat at its first tempo."""
    import mido
    mid = mido.MidiFile(path)
    to_slot = grid.slots / mid.ticks_per_beat / grid.quarters
    tick, sounding, notes, tempo = 0, {}, [], None
    for message in mido.merge_tracks(mid.tracks):
        tick += message.time
        if message.type == 'set_tempo' and tempo is None:
            tempo = message.tempo
        elif message.type == 'note_on' and message.velocity > 0:
            sounding[message.note] = tick
        elif message.type in ('note_off', 'note_on') and message.note in sounding:
            notes.append((sounding.pop(message.note) * to_slot, tick * to_slot, message.note))
    return sorted(notes), (tempo or 500000) / 1e6 * grid.quarters


def tune_level(spectrum, times, onset, next_onset, pitch):
    """The level of a tune note in the recording at its onset against the loudest pitch from an octave below to an octave above,
    and the louder pitch near it when the note is weak (None when it is not)."""
    s = window(spectrum, times, onset, min(next_onset, onset + MELODY_S))
    k = pitch - LOWEST_MIDI
    low = max(0, k - 12)
    octave, close = s[low:k + 13], s[max(0, k - 2):k + 3]
    louder = None
    if s[k] < 0.7 * close.max():
        louder = int(close.argmax()) + max(0, k - 2) + LOWEST_MIDI
    elif s[k] < 0.25 * octave.max():
        louder = int(octave.argmax()) + low + LOWEST_MIDI
    return s[k] / (octave.max() + 1e-9), louder


def cmd_compare(a):
    grid = saved_grid(a.work)
    export(a.score, a.work / 'score.mid')
    export(a.score, a.work / 'score.wav')
    # MuseScore plays the score at its first tempo: it does not play "rit.", and a later tempo mark is not handled here
    score, score_beat_s = score_notes(a.work / 'score.mid', grid)
    if not score:
        sys.exit(f'{a.score} has no notes')
    bars = grid.place(max(end for _, end, _ in score) - 1e-6)[0]
    src, src_t = source_cqt(a.work)
    out, out_t = cqt(a.work / 'score.wav')
    both = lambda start, end: (window(src, src_t, grid.time(start), grid.time(end)),
                               window(out, out_t, start * score_beat_s, end * score_beat_s))
    full = []

    def beat_match(i):
        s, o = both(i, i + 1)
        return float(unit(chroma(s)) @ unit(chroma(o)))

    match = np.array([np.mean([beat_match(i) for i in range(grid.first(bar), grid.first(bar + 1))]) for bar in range(1, bars + 1)])
    print(f'HARMONY (beat chroma, octaves folded): mean {match.mean():.2f}; lowest bars: '
          + ' '.join(f'{i + 1}:{short(match[i])}' for i in np.argsort(match)[:8]))
    full.append('HARMONY of each bar: ' + ' '.join(f'{i + 1}:{short(m)}' for i, m in enumerate(match)))

    models = [(grid.slot(n['on']), n['p'], n['v']) for n in combined(a.work)]
    models += [(grid.slot(on), pitch, None) for on, _, pitch, *_ in sung_notes(*vocal_f0(a.work))]        # the note models do not read the voice
    near = lambda notes, slot, pitch, reach, any_octave=False: any(
        abs(s - slot) <= reach and (p == pitch or (any_octave and p % 12 == pitch % 12)) for s, p, *_ in notes)
    bar_of = lambda slot: grid.place(round(slot))[0]
    place = lambda slot, pitch: f'{grid.place(round(slot))[1]}:{name(pitch)}'

    def report(label, items, total):
        """items are (bar, text, the pitch is there in another octave). Print the ones in no octave; all go to the file."""
        alone = by_bar((bar, text) for bar, text, octave in items if not octave)
        print(f'{label}: {len(items)} of {total}; in no octave: {sum(not octave for _, _, octave in items)}')
        if alone:
            print('\n'.join(first_rows(alone, a.rows)))
        full.extend([f'{label} (!! in no octave):'] + by_bar((bar, text + ('' if octave else '!!')) for bar, text, octave in items))

    report('NOTES: score notes that no model has', [(bar_of(s), place(s, p), near(models, s, p, 1.1, True))
                                                    for s, _, p in score if not near(models, s, p, 1.1)], len(score))
    strong = [(s, p) for s, p, v in models if v is not None and v >= STRONG]
    written = [(start, pitch) for start, _, pitch in score]
    report('NOTES: strong piano-model notes that the score does not have', [(bar_of(s), place(s, p), near(written, s, p, 1.6, True))
                                                                           for s, p in strong if not near(written, s, p, 1.6)], len(strong))

    if a.song:
        song = json.loads(Path(a.song).read_text())
        tune, moved = [], 0
        for key, pitch in song['melody'].items():
            bar, _, beat = key.partition(':')
            octaves = octaves_from_sung(song, int(bar), float(beat))
            tune.append((grid.first(int(bar)) + float(beat) - 1, midi(pitch) - 12 * octaves))
            moved += octaves != 0
        if moved:
            print(f'MELODY tests {moved} notes at the sung pitch: `octaves` in the song file moves them from it')
    else:
        tune = [(start / grid.slots, pitch) for start, pitch in top_line([(s, p) for s, _, p in score], MELODY_FLOOR)]
    tune.sort()
    onsets = [grid.time(beat) for beat, _ in tune] + [math.inf]
    weak, octave = [], []
    for i, (beat, pitch) in enumerate(tune):
        level, louder = tune_level(src, src_t, onsets[i], onsets[i + 1], pitch)
        bar, in_bar = grid.bar_at(beat)
        text = f'{bar}:{in_bar + 1:g} {name(pitch)}' + (f'<{name(louder)}' if louder is not None else '')
        full.append(f'MELODY {text} level {short(level)}')
        if louder is not None and abs(louder - pitch) == 12:
            octave.append(bar)
        elif louder is not None:
            weak.append(text)
    print(f'MELODY ({"the tune of the song file" if a.song else f"top score notes from {name(MELODY_FLOOR)}"}): {len(weak) + len(octave)} of {len(tune)} tune notes are weak '
          'in the recording at their onset (bar:beat pitch<louder pitch near it)' + (':' if weak else ''))
    if octave:
        print(f'  {len(octave)} with the louder pitch one octave away, in bars {bar_ranges(sorted(set(octave)))}')
    if weak:
        print('  ' + ' | '.join(weak[:2 * a.rows]) + (f' | ... {len(weak) - 2 * a.rows} more' if len(weak) > 2 * a.rows else ''))

    for bar in bar_list(a.bars) if a.bars else []:
        s, o = (chroma(v) for v in both(grid.first(bar), grid.first(bar + 1)))
        print(f'bar {bar} pitch classes, source: ' + ' '.join(f'{NAMES[k]} {short(s[k] / s.max())}' for k in np.argsort(s)[::-1][:6])
              + ' | score: ' + ' '.join(f'{NAMES[k]} {short(o[k] / o.max())}' for k in np.argsort(o)[::-1][:6]))
    write_detail(a.work, 'compare.txt', full)


# ---------------------------------------------------------------- song

def cmd_song(a):
    grid = saved_grid(a.work)
    song = json.loads(Path(a.song).read_text())
    last = grid.bar_at(len(grid.beats) - 1)[0]
    on_slot = lambda beat: abs((beat - 1) * grid.slots - round((beat - 1) * grid.slots)) < 0.02
    tune, bad = [], []
    for key, token in song.get('melody', {}).items():
        bar, _, beat = key.partition(':')
        valid = re.fullmatch(r'[1-9]\d*', bar) and re.fullmatch(r'\d+(\.\d+)?', beat) and PITCH.fullmatch(str(token))
        if valid and int(bar) <= last and 1 <= float(beat) < grid.count(int(bar)) + 1 and on_slot(float(beat)):
            tune.append((grid.first(int(bar)) + float(beat) - 1, f'{key} {token}', int(bar), midi(token) - 12 * octaves_from_sung(song, int(bar), float(beat))))
        else:
            bad.append(f'{key} {token}')
    tune.sort()
    tones, bad_chords = {}, []
    for bar, text in song.get('chords', {}).items():
        notes = str(text).split()
        if re.fullmatch(r'[1-9]\d*', bar) and int(bar) <= last and notes and all(PITCH.fullmatch(t + '4') for t in notes):
            tones[int(bar)] = {midi(t + '4') % 12 for t in notes}
        else:
            bad_chords.append(f'{bar}: {text}')
    bad_sections, owner = [], {}
    for label, text in song.get('sections', {}).items():
        m = re.fullmatch(r'([1-9]\d*)(?:-([1-9]\d*))?', str(text))
        if not m or int(m[2] or m[1]) < int(m[1]) or int(m[2] or m[1]) > last:
            bad_sections.append(f'{label}: {text}')
            continue
        for bar in range(int(m[1]), int(m[2] or m[1]) + 1):
            owner.setdefault(bar, []).append(label)
    times, pitch = vocal_f0(a.work)[:2]
    pitch = in_tune(pitch)
    src, src_t = source_cqt(a.work)
    notes = combined(a.work)
    unsung, unplayed, weak = [], [], []
    onsets = [grid.time(beat) for beat, *_ in tune] + [math.inf]
    for i, (beat, text, bar, m) in enumerate(tune):
        on, after = onsets[i], onsets[i + 1]
        sung = pitch[(times >= on + SUNG_FROM_S) & (times < min(after, on + SUNG_TO_S))]
        sung = sung[~np.isnan(sung)]
        apart = np.abs((sung - m + 6) % 12 - 6)
        if len(sung) >= 3 and (apart.min() > 0.6 or np.mean(apart <= 0.7) < 0.25):
            unsung.append(f'{text}<{name(int(round(np.median(sung))))}')
        elif len(sung) < 3 and not any(abs(grid.slot(n['on']) - beat * grid.slots) <= 1 and n['p'] == m for n in notes):
            unplayed.append(text)
        louder = tune_level(src, src_t, on, after, m)[1]
        if louder is not None:
            weak.append(f'{text}<{name(louder)}')
    print(f'{len(tune)} melody notes in bars {bar_ranges(sorted({bar for *_, bar, _ in tune}))}; chords for {len(tones)} of {last} bars')
    findings = {
        'melody entries that are not valid (bar:beat on a sixteenth of a bar of the grid, and a pitch such as F#4)': bad,
        'chord entries that are not valid (a bar of the grid, and pitch names such as "E G B")': bad_chords,
        'section entries that are not valid (a name and one bar range of the grid, such as "chorus 2": "30-37")': bad_sections,
        'bars in two sections': [f'{bar} ({" / ".join(labels)})' for bar, labels in sorted(owner.items()) if len(labels) > 1],
        'bars with no chord (check does not test their notes)': bar_ranges([bar for bar in range(1, last + 1) if bar not in tones]).split(', ') if len(tones) < last else [],
        'melody notes outside the chord of their bar': [text for _, text, bar, m in tune if bar in tones and m % 12 not in tones[bar]],
        'melody notes where the voice sings another pitch (bar:beat pitch<sung pitch; octaves not counted)': unsung,
        'melody notes where the voice does not sing, that no note model has within one slot': unplayed,
        'melody notes weak in the recording at their onset (bar:beat pitch<louder pitch near it; the MELODY test of compare)': weak}
    print_findings(a, findings, 'song.txt')


def cmd_octave(a):
    song = json.loads(a.song.read_text(encoding='utf-8'))
    moved = 0
    for key, token in song.get('melody', {}).items():
        bar, _, beat = key.partition(':')
        if in_ranges(a.ranges, int(bar), float(beat)):
            song['melody'][key] = name(midi(token) + 12 * a.octaves)
            moved += 1
    record = song.setdefault('octaves', {})
    record[a.ranges] = record.get(a.ranges, 0) + a.octaves
    song['octaves'] = {spec: n for spec, n in record.items() if n}
    if not song['octaves']:
        del song['octaves']
    a.song.write_text(json.dumps(song, indent=1, ensure_ascii=False) + '\n', encoding='utf-8', newline='')
    print(f'moved {moved} melody notes in {a.ranges} by {a.octaves:+d} octave{"s" * (abs(a.octaves) != 1)}; `octaves` in {a.song.name} records it, so that compare and song test the sung pitch')


# ---------------------------------------------------------------- cells

def cmd_cells(a):
    grid = saved_grid(a.work)
    lines = a.draft.read_text(encoding='utf-8').splitlines()
    drafted = {(int(m[1]), m[2]) for m in map(DRAFT_LINE.fullmatch, lines) if m}
    last_bar = max((int(m[1]) for m in map(BAR_LINE.match, lines) if m), default=0)
    carried = {}

    def print_ties(before):
        """Print the bars before `before` that a tie sounds into and the draft has no line of that lane for."""
        while due := sorted(((bar, lane, token, length) for lane, (bar, token, length) in carried.items()
                             if bar < before and bar <= last_bar and (bar, lane) not in drafted), key=lambda tie: tie[0]):
            for bar, lane, token, length in due:
                del carried[lane]
                print(f'{bar} {lane} | ' + ' | '.join(slot_cells(grid, bar, lane, [(0, token, length)], carried)) + ' |')

    for line in lines:
        bar_line = BAR_LINE.match(line)
        if bar_line:
            print_ties(int(bar_line[1]) + 1)
        m = DRAFT_LINE.fullmatch(line)
        if not m:
            print(line)
            continue
        bar, lane = int(m[1]), m[2]
        notes = []
        for item in (m[3] or '').split():
            note = SLOT_NOTE.fullmatch(item)
            if not note or int(note[1]) >= grid.count(bar) * grid.slots:
                sys.exit(f'bar {bar} {lane}: not slot:pitch/sixteenths in the bar: {item}')
            notes.append((int(note[1]), note[2], int(note[3])))
        tie = carried.pop(lane, None)
        if tie and tie[0] == bar and all(slot for slot, _, _ in notes):
            notes.append((0, *tie[1:]))
        print(f'{bar} {lane} | ' + ' | '.join(slot_cells(grid, bar, lane, notes, carried)) + ' |')
    print_ties(last_bar + 1)


def slot_cells(grid, bar, lane, notes, carried):
    """The pno cells of a bar of a lane from (slot, token, sixteenths). Notes on one slot make a chord; a note ends where the next
    one starts, and one that sounds into the next bar goes into carried as a tie."""
    size = grid.count(bar) * grid.slots
    tokens = ['_'] * size
    starts = sorted({slot for slot, _, _ in notes})
    for start, after in zip(starts, starts[1:] + [size]):
        chord = [(token, length) for slot, token, length in notes if slot == start]
        end = start + max(length for _, length in chord)
        stop = min(end, after)
        tokens[start:stop] = ['+'.join(token for token, _ in chord)] + ['-'] * (stop - start - 1)
        pitches = [p[0] for token, _ in chord for p in PITCH.finditer(token)]
        if end > size and pitches:
            carried[lane] = (bar + 1, '+'.join('~' + p for p in pitches), end - size)
    if lane in ('rh', 'lh'):
        if not notes:
            return ['R']
        i = 0
        while i < size:
            if tokens[i] != '_':
                i += 1
                continue
            length = grid.slots
            while i % length or i + length > size or any(token != '_' for token in tokens[i:i + length]):
                length //= 2
            tokens[i:i + length] = ['r'] + ['-'] * (length - 1)
            i += length
    cells = []
    for first in range(0, size, grid.slots):
        cell = tokens[first:first + grid.slots]
        for count in (1, 3, 6) if grid.slots == 6 else (1, 2, 4):
            step = grid.slots // count
            if all(token == ('_' if cell[i - i % step] == '_' else '-') for i, token in enumerate(cell) if i % step):
                break
        cells.append(' '.join(cell[::step]))
    return cells


# ---------------------------------------------------------------- check

def beat_lengths(signature):
    """The quarter notes in each beat of a bar of this time signature (pno spec 3.6)."""
    if signature == 'c':
        return [1] * 4
    if signature == 'cut':
        return [2] * 2
    upper, lower = re.match(r'([\d+]+)/(\d+)', signature).groups()
    lower = int(lower)
    pattern = re.search(r'\(([\d+]+)\)', signature)
    if pattern or '+' in upper:                 # 7/8(2+2+3) or 2+2+3/8: a beat for each group
        return [Fraction(int(n) * 4, lower) for n in (pattern[1] if pattern else upper).split('+')]
    upper = int(upper)
    if lower >= 8 and upper % 3 == 0:
        return [Fraction(12, lower)] * (upper // 3)
    if lower >= 8:
        return [Fraction(8, lower)] * (upper // 2)
    return [Fraction(4, lower)] * upper


def x_share(tokens):
    """The share of a cell that leading `x` slots take: time that is not in the bar (pno spec 3.2)."""
    return Fraction(next((i for i, t in enumerate(tokens) if t != 'x'), len(tokens)), len(tokens))


def bar_cells(text):
    """Each bar in file order as (label, quarter notes of each cell, share of `x` in its first cell, share of `x` in its last cell).
    A time signature holds until the next one. A bar has the cells of its rh line: an incomplete bar or a cadenza has its own count."""
    signature, cells, bars = 'c', {}, []
    for line in text.splitlines():
        words = line.split()
        if '@' in words[:2]:
            settings = dict(w.split('=', 1) for w in words if '=' in w)
            signature = settings.get('time', signature)
            if 'cells' in settings:
                cells[words[0]] = int(settings['cells'])
        elif len(words) > 1 and words[1] == 'rh':
            full = beat_lengths(signature)
            row = [cell.split() for cell in line.split('|')[1:-1]]
            count = cells.get(words[0], len(full)) if row == [['R']] else len(row)
            lead, trail = x_share(row[0]), x_share(row[-1][::-1])
            shift = -count if lead else 0           # a bar that starts with `x` is the end of a bar: in 7/8(2+2+3) a pickup cell is the 3
            bars.append((words[0], [full[(i + shift) % len(full)] for i in range(count)], lead, trail))
    return bars


def onsets(score_path):
    """One row per onset from `pno sound`: bar, beat from 0, then new and held pitches of each hand; bar_cells(); and the pitch
    and velocity of each new note by (bar, beat).
    The second part of a split bar (`8b`) is in its bar, after the cells that the first part plays."""
    text = Path(score_path).read_text()
    with tempfile.TemporaryDirectory() as folder:
        plain = Path(folder) / 'score.pno'
        plain.write_text(''.join(line for line in text.splitlines(True) if '.8va' not in line))   # `pno sound` adds an octave under 8va
        report = subprocess.run(['pno', 'sound', '--velocity', str(plain)], capture_output=True, text=True, check=True).stdout
    bars = bar_cells(text)
    first = {label: len(lengths) - trail for label, lengths, _, trail in bars}
    later = {label: first[label[:-1]] - lead for label, _, lead, _ in bars if label.endswith('b')}
    rows, struck = [], {}
    for line in report.splitlines():
        m = re.match(r'(\d+)(b?) ([\d+/]+)\s+lh(.*?)\| rh([^\[]*)', line)
        if not m:
            continue
        bar, beat = int(m[1]), later.get(m[1] + m[2], 0) + sum(Fraction(x) for x in m[3].split('+')) - 1
        hands = []
        for side in (m[4], m[5]):
            tokens = [t for t in side.split() if t != '-']
            new = [(midi(pitch), int(velocity)) for pitch, velocity in (t.split('@') for t in tokens if not t.endswith('~'))]
            hands += [[pitch for pitch, _ in new], [midi(t.rstrip('~')) for t in tokens if t.endswith('~')]]
            struck.setdefault((bar, beat), []).extend(new)
        rows.append((bar, beat, *hands))
    return rows, bars, struck


def lh_pattern(onsets):
    """(beat, new pitches) of the left-hand onsets of one bar -> (sixteenths of the onsets, contour of the lowest note: 0 same, 1 step, 2 leap, minus down); None below 2 onsets."""
    if len(onsets) < 2:
        return None
    line = [min(new) for _, new in onsets]
    moves = [b - a for a, b in zip(line, line[1:])]
    return frozenset(round(beat * 4) for beat, _ in onsets), tuple((m > 0) - (m < 0) if abs(m) <= 2 else 2 * ((m > 0) - (m < 0)) for m in moves)


def is_variant(a, b):
    """Two bar patterns are one idea: 0.6 or more of the onsets on the same sixteenths, and 0.6 or more of the contour in order."""
    common = [[0] * (len(b[1]) + 1) for _ in range(len(a[1]) + 1)]
    for i, x in enumerate(a[1]):
        for j, y in enumerate(b[1]):
            common[i + 1][j + 1] = common[i][j] + 1 if x == y else max(common[i][j + 1], common[i + 1][j])
    return len(a[0] & b[0]) / len(a[0] | b[0]) >= 0.6 and common[-1][-1] / max(len(a[1]), len(b[1]), 1) >= 0.6


def section_lines(work, sections, rows, shape, velocities):
    """One line for each section of song.json: the notes in a beat of each hand, the left-hand bars that vary the bar before, the
    most common left-hand rhythm, the level of the mix against the mean velocity of the score, and for each stem with notes there,
    how many of its notes the score strikes (the pitch class within one slot) and the beats where it strikes fewer than half."""
    grid = saved_grid(work)
    struck = {}
    for bar, beat, ln, _, rn, _ in rows:
        struck.setdefault(round((grid.first(bar) + beat) * grid.slots), set()).update(p % 12 for p in ln + rn)
    last = grid.bar_at(len(grid.beats) - 1)[0]
    mix = levels(work, ['src'], [grid.bar_start(bar) for bar in range(1, last + 2)])['src']
    stems = {'bass': model_notes(work, 'bass')} if (work / 'bass.json').exists() else {}
    stems |= {stem: combined(work, models) for stem, models in PART_NOTES.items() if all((work / f'{m}.json').exists() for m in models.values())}
    lines = ['sections: notes in a beat (lh/rh) | left-hand bars that vary the bar before (exact copies) | the most common left-hand rhythm '
             '(sixteenths) | mix dB, mean velocity | struck notes of each stem (beats with fewer than half)']
    for label, text in sections.items():
        first, _, end = str(text).partition('-')
        bars = range(int(first), int(end or first) + 1)
        beats = sum(grid.count(bar) for bar in bars)
        lh = sum(len(ln) for bar, _, ln, *_ in rows if bar in bars)
        rh = sum(len(rn) for bar, _, _, _, rn, _ in rows if bar in bars)
        pairs = [(shape.get(bar), shape.get(bar - 1)) for bar in bars if bar - 1 in bars]
        pairs = [(x, y) for x, y in pairs if x and y]
        rhythms = Counter(shape[bar][0] for bar in bars if shape.get(bar))
        if rhythms:
            onsets, count = rhythms.most_common(1)[0]
            rhythm = f'{" ".join(map(str, sorted(onsets)))} in {count} of {sum(rhythms.values())} bars'
        else:
            rhythm = '-'
        heard = [mix[bar - 1] for bar in bars if bar <= last]
        played = [v for (bar, _), notes in velocities.items() if bar in bars for _, v in notes]
        loudness = f'{np.mean(heard):.0f} dB, ' + (f'{np.mean(played):.0f}' if played else '-') if heard else '-'
        kept = []
        for stem, notes in stems.items():
            slots = [round(grid.slot(n['on'])) for n in notes if grid.place(round(grid.slot(n['on'])))[0] in bars]
            pcs = [n['p'] % 12 for n in notes if grid.place(round(grid.slot(n['on'])))[0] in bars]
            if len(slots) >= len(bars):
                hits = [any(pc in struck.get(s + d, ()) for d in (-1, 0, 1)) for s, pc in zip(slots, pcs)]
                beat_of = [int(grid.place(s)[1] // grid.slots) + 1 for s in slots]
                missed = [b for b in sorted(set(beat_of)) if 2 * sum(h for h, x in zip(hits, beat_of) if x == b) < beat_of.count(b)]
                kept.append(f'{stem} {sum(hits)}/{len(slots)}' + (f' ({" ".join(map(str, missed))})' if missed else ''))
        lines.append(f'{label} {text}: {lh / beats:.1f}/{rh / beats:.1f} | {sum(is_variant(x, y) for x, y in pairs)} of {len(pairs)} '
                     f'({sum(x == y for x, y in pairs)}) | {rhythm} | {loudness} | ' + (', '.join(kept) or 'no stem notes'))
    return lines


def cmd_check(a):
    rows, bars, struck = onsets(a.score)
    starts, total = {}, 0
    for label, lengths, lead, trail in bars:
        starts[label] = total - lead
        total += len(lengths) - lead - trail
    song = json.loads(Path(a.song).read_text()) if a.song else {}
    free = set(bar_list(song['free_bars'])) if song.get('free_bars') else set()
    at = lambda bar, beat: f'{bar}:{float(beat) + 1:g}'
    clock = lambda bar, beat: starts[str(bar)] + beat
    findings = {}

    if 'melody' in song:
        tune = {}
        for key, pitch in song['melody'].items():
            bar, beat = key.split(':')
            tune[(int(bar), Fraction(beat).limit_denominator(48) - 1)] = midi(pitch)
        heard = {(bar, beat): ln + lh + rn + rh for bar, beat, ln, lh, rn, rh in rows}
        inner = lambda key: 'inner' in song and in_ranges(song['inner'], key[0], float(key[1]) + 1)
        findings['melody notes that are not in the score'] = [
            f'{at(*key)} {name(pitch)}' for key, pitch in sorted(tune.items()) if pitch not in heard.get(key, [])]
        findings['melody notes below another note (an octave doubling above is not counted)'] = [
            f'{at(*key)} {name(pitch)} below {name(max(heard[key]))}' for key, pitch in sorted(tune.items())
            if pitch in heard.get(key, []) and max(heard[key]) % 12 != pitch % 12 and not inner(key)]
        quiet = []
        for key, pitch in sorted(tune.items()):
            together = struck.get(key, [])
            own = [v for p, v in together if p == pitch]
            others = [v for p, v in together if p % 12 != pitch % 12]
            if own and others and max(own) <= max(others):
                quiet.append(f'{at(*key)} {name(pitch)} {max(own)} against {max(others)}')
        findings['melody notes not louder than the other notes struck with them (velocity)'] = quiet
        velocities = [v for notes in struck.values() for _, v in notes]
        print(f'velocities in the score: {min(velocities)} to {max(velocities)}')

    grid = saved_grid(a.work)
    seconds = lambda bar, beat: grid.time(clock(bar, beat))
    info = {}
    times = sorted({seconds(bar, beat): (bar, beat) for bar, beat, ln, _, rn, _ in rows if ln or rn}.items())
    info[f'dead spots: {DEAD_S:g} s or more with no new note'] = [
        f'bar {bar} beat {float(beat) + 1:g}: {s1 - s0:.1f} s'
        for (s0, (bar, beat)), (s1, _) in zip(times, times[1:]) if s1 - s0 >= DEAD_S and bar not in free]

    left = [(seconds(bar, beat), bar, beat, ln) for bar, beat, ln, _, _, _ in rows if ln]
    info[f'left-hand leaps larger than an octave in less than {LEAP_S:g} s inside a bar (the leap from the bass note is not counted)'] = [
        f'{at(bar, beat)} {name(before[-1])}->{name(ln[0])}'
        for (s0, bar0, beat0, before), (s1, bar, beat, ln) in zip(left, left[1:])
        if bar0 == bar and beat0 != 0 and s1 - s0 < LEAP_S and min(abs(x - y) for x in ln for y in before) > 12]

    right = [(seconds(bar, beat), bar, beat, rn) for bar, beat, _, _, rn, _ in rows if rn]
    info[f'right-hand jumps larger than an octave in less than {LEAP_S:g} s'] = [
        f'{at(bar, beat)} {name(max(before))}->{name(max(rn))}'
        for (s0, _, _, before), (s1, bar, beat, rn) in zip(right, right[1:])
        if s1 - s0 < LEAP_S and abs(min(rn) - min(before)) > 12 and abs(max(rn) - max(before)) > 12]

    findings['hands overlap or cross'] = [at(bar, beat) for bar, beat, ln, lh, rn, rh in rows
                                          if ln + lh and rn + rh and max(ln + lh) >= min(rn + rh)]

    shape = {bar: lh_pattern([(beat, ln) for _, b, beat, ln in left if b == bar]) for bar in {bar for bar, *_ in rows} if bar not in free}
    if 'sections' in song:
        for line in section_lines(a.work, song['sections'], rows, shape, struck):
            print(line)

    print_findings(a, findings, 'check.txt', info)


def print_findings(a, findings, file, info={}):
    """Findings with their counts, then information without counts: a count invites a fix to zero of what is intended."""
    for label, items in findings.items():
        if items:
            print(f'[{len(items)}] {label}: ' + ' | '.join(items[:a.rows]) + (f' | ... {len(items) - a.rows} more' if len(items) > a.rows else ''))
    clean = [label for label, items in findings.items() if not items]
    if clean:
        print('[0] ' + '; '.join(clean))
    for label, items in info.items():
        if items:
            print(f'- {label}: ' + ' | '.join(items[:a.rows]) + (' | ...' if len(items) > a.rows else ''))
    write_detail(a.work, file, [f'[{len(items)}] {label}: ' + ' | '.join(items) for label, items in findings.items()]
                 + [f'- {label}: ' + ' | '.join(items) for label, items in info.items()])


# ---------------------------------------------------------------- command line

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('-w', '--work', type=Path, default=Path('a2p-work'), help='folder for the files of one song')
    commands = parser.add_subparsers(dest='command', required=True)

    def command(function, text, *arguments):
        sub = commands.add_parser(function.__name__[4:], help=text)
        sub.set_defaults(run=function)
        for flags, options in arguments:
            sub.add_argument(*flags, **options)

    bars = (['bars'], dict(help='bars, for example 5-8,12'))
    view = (['--view'], dict(choices=list(VIEWS), default='mix', help='the notes to show (default mix); ' + '; '.join(f'{k}: {v}' for k, v in VIEWS.items())))
    rows = (['--rows'], dict(type=int, default=12, help='rows to print for each list (default 12); the file has all rows'))
    stem = (['--stem'], dict(choices=list(PART_NOTES), help='instead: all notes of one model on this stem (Transkun on piano.wav, basic-pitch on the others)'))
    command(cmd_prepare, 'convert the audio, split stems and track beats, then run the note models on the stems', (['audio'], {}))
    command(cmd_step, 'one step of prepare', (['name'], dict(choices=list(STEPS | AFTER_STEMS))))
    command(cmd_grid, 'the tracked grid, the constant grid, and where they differ; --set or --constant saves grid.json',
            (['--constant'], dict(action='store_true', help='save the constant grid')),
            (['--set'], dict(nargs='*', metavar='FIELD=VALUE', help=f'save the grid; fields: {GRID_FIELDS}')))
    command(cmd_roll, 'merged model notes of each bar', bars,
            view,
            stem,
            (['--split'], dict(type=midi, default=59, help='lowest pitch of the high notes (default B3)')),
            (['--beats'], dict(action='store_true', help='also print the chord of each beat, and the second chord where it is likely')))
    command(cmd_raw, 'model notes of each bar with exact beat positions and velocities, re-onsets not merged', bars,
            view, stem,
            (['--lo'], dict(type=midi, default=21)), (['--hi'], dict(type=midi, default=108)),
            (['--min-velocity'], dict(type=int, default=40)))
    command(cmd_bass, 'the bass line of the bass stem, one line for each bar',
            (['bars'], dict(nargs='?', help='bars, for example 5-8,12 (default: each bar with bass notes)')),
            (['--octave'], dict(type=int, default=0, help='print the line this many octaves higher')))
    command(cmd_tune, 'the sung tune of the vocal stem, one line for each bar; writes a draft melody to tune.json',
            (['bars'], dict(nargs='?', help='bars, for example 5-8,12 (default: each sung bar)')),
            (['--octave'], dict(type=int, default=0, help='print and save the tune this many octaves higher')),
            (['--contour'], dict(action='store_true', help='also print the sung pitch of each slot in 4 steps, with syllable starts')),
            (['--fine'], dict(action='store_true', help='also print each slot in 3 steps with pitch and level (for one or two bars)')))
    command(cmd_peek, 'loudness of given pitches through time', bars, (['pitches'], dict(help='for example F#5,G5')),
            (['--stem'], dict(choices=['src', 'drums', 'bass', 'other', 'vocals', 'lead', 'backing', *PART_NOTES], default='src', help='the audio to read (default src, the mix)')))
    command(cmd_compare, 'compare a score with the recording', (['score'], dict(type=Path)), rows,
            (['--song'], dict(help='JSON with the melody: MELODY tests these notes')),
            (['--bars'], dict(help='also print pitch-class profiles of these bars')))
    command(cmd_song, 'checks of song.json against the recording, before the score', (['song'], dict(type=Path)), rows)
    command(cmd_levels, 'when each stem plays, the tuning, and the level of each stem in each bar (each 4 s without a grid)',
            (['bars'], dict(nargs='?', help='bars, for example 5-8,12 (default: all)')))
    command(cmd_octave, 'move the melody of song.json in bar ranges by octaves, and record the move from the sung pitch',
            (['song'], dict(type=Path)), (['ranges'], dict(help='for example 14-21,29:3.5-38 (bar:beat; both ends included)')),
            (['octaves'], dict(type=int, help='for example 1 or -1')))
    command(cmd_cells, 'print a draft as pno: lines `BAR LANE slot:pitch/sixteenths ...` (rh, lh, rh2, ...) become bar cells, other lines stay',
            (['draft'], dict(type=Path)))
    command(cmd_check, 'piano-idiom checks of a score', (['score'], dict(type=Path)), rows,
            (['--song'], dict(help='JSON with melody, chords, parallel, free_bars, inner and sections')))
    arguments = parser.parse_args()
    arguments.run(arguments)


if __name__ == '__main__':
    main()
