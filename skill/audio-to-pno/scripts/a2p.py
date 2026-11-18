"""Evidence and checks for a piano transcription of a recording into pno. The workflow is in SKILL.md."""
import argparse, json, math, os, re, subprocess, sys, tempfile
from collections import Counter
from fractions import Fraction
from pathlib import Path

import numpy as np

SR, HOP, LOWEST_MIDI, BINS = 22050, 512, 24, 84
NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B']
STEP = {'C': 0, 'D': 2, 'E': 4, 'F': 5, 'G': 7, 'A': 9, 'B': 11}
STRONG = 85          # piano-model velocity that marks a prominent note
MELODY_FLOOR = 69    # with no song file, the tune is the top score note from A4
MODELS = {'t': 'tk', 'p': 'pt', 'b': 'bp'}      # the letter of a model in the output, and its note file
AGREE_S = 0.08       # two models have the same note when their onsets are this near
BEAT_MODEL = Path.home() / '.cache' / 'audio-to-pno' / 'beat_this-final0.ckpt'
# the lead voice without backing vocals: in 24 pop songs it gave `tune` 3 points more than the vocals of demucs
LEAD_MODEL, LEAD_MODEL_DIR = 'bs_roformer_karaoke_frazer_becruily.ckpt', BEAT_MODEL.parent / 'separator'
# the chord model BTC reads the log CQT of 22.05 kHz audio (24 bins an octave from C1) in windows of 108 frames of 93 ms
CHORD_MODEL = BEAT_MODEL.parent / 'btc_model_large_voca.pt'
CHORD_SR, CHORD_HOP, CHORD_BINS, CHORD_WINDOW = 22050, 2048, 144, 108
# the 14 chord types of BTC, in its order, as the triad that `roll` prints: in 24 pop songs 1 of 3 of its sevenths was false
CHORD_KINDS = ['m', '', 'dim', 'aug', 'm', '', 'm', 'm', '', '', 'dim', 'dim', 'sus2', 'sus4']
UNSURE = 0.6        # 24 pop songs: below this probability the root of the chord was right half of the time, above it 9 times of 10
# beat repair, tuned on rendered clips: a gap of 0.5 or 1.5 beats (this near) in this share of the gaps means two beat levels;
# a tracked beat less than EXTRA_BEAT of a beat after the last one is not a beat when the next one follows in time
LEVEL_REACH, LEVEL_SHARE, EXTRA_BEAT = 0.15, 0.15, 0.6
# tuned on rendered ensembles: the piano models do not hear a lead instrument that is not a piano,
# so the mix view also shows a note that only bp has when it is this long
B_LONG_S = 0.25
VIEWS = {'piano': 'notes that Transkun has', 'mix': f'notes of two models, and bp notes of {B_LONG_S} s or longer', 'all': 'all notes'}
GRID_FIELDS = 'beats_per_bar=N shift=N level=half|third|double quarters_per_beat=1.5'
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


def name(p):
    return NAMES[p % 12] + str(p // 12 - 1)


def midi(token):
    m = re.fullmatch(r'([A-G])(#*|b*)(-?\d)', token)
    if not m:
        sys.exit(f'not a pitch: {token}')
    return STEP[m[1]] + m[2].count('#') - m[2].count('b') + 12 * (int(m[3]) + 1)


def bar_list(spec):
    bars = []
    for part in spec.split(','):
        first, _, last = part.partition('-')
        bars += range(int(first), int(last or first) + 1)
    return bars


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

    def __init__(self, work):
        path = work / 'grid.json'
        if not path.exists():
            sys.exit(f'{path} is missing: run `grid`, then `grid --set`')
        g = json.loads(path.read_text())
        self.per_bar, self.quarters = g['beats_per_bar'], g['quarters_per_beat']
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

    def bar_start(self, bar):
        return self.time((bar - 1) * self.per_bar)

    @property
    def bar_slots(self):
        return self.per_bar * self.slots


def model_notes(work, source):
    path = work / f'{source}.json'
    if not path.exists():
        sys.exit(f'{path} is missing: run `prepare`')
    return sorted(json.loads(path.read_text())['notes'], key=lambda n: (n['on'], n['p']))


def combined(work):
    """One note list from the three models. `by` has the letters of the models that have the note.
    The onset is from the best model that has it (t, p, b), the length from t, b, p.
    `v` is a velocity of a piano model (None when only bp has the note); `amp` is the bp amplitude."""
    tagged = sorted(((n['p'], n['on'], mark, n) for mark, source in MODELS.items() for n in model_notes(work, source)),
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
        sf.write(work / f'{stem}.wav', signal.numpy().T, sr)
    # the note models read this: a voice and drums give them false notes
    sf.write(work / 'backing.wav', sum(signal for stem, signal in zip(model.sources, stems) if stem in ('bass', 'other')).numpy().T, sr)


def run_transkun(work):
    import moduleconf, soundfile as sf, soxr, torch
    from importlib.resources import files
    folder = files('transkun') / 'pretrained'
    conf = moduleconf.parseFromFile(str(folder / '2.0.conf'))['Model']
    model = conf.module.TransKun(conf=conf.config)
    state = torch.load(str(folder / '2.0.pt'), map_location='cpu')
    model.load_state_dict(state.get('best_state_dict', state.get('state_dict')), strict=False)
    model.to(device()).eval()
    audio, rate = sf.read(work / 'backing.wav', dtype='float32')
    with torch.no_grad():
        events = model.transcribe(torch.from_numpy(soxr.resample(audio, rate, model.fs)).to(device()), discardSecondHalf=False)
    # a pedal event has a negative pitch
    save_notes(work, 'tk', [dict(on=float(e.start), off=float(e.end), p=int(e.pitch), v=int(e.velocity)) for e in events if e.pitch > 0])


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


def run_bass(work):
    """The bass line of the bass stem: the bp notes of bass.wav, one note at a time. No notes where the stem is silent."""
    import librosa
    run_basic_pitch(work, 'bass', 'bp_bass')
    level = lambda stem: 20 * np.log10(librosa.feature.rms(y=librosa.load(work / f'{stem}.wav', sr=F0_SR, mono=True)[0],
                                                           frame_length=F0_FRAME, hop_length=F0_HOP)[0] + 1e-9)
    stem, floor = level('bass'), np.percentile(level('src'), 99) + BASS_GATE_DB
    frame = lambda seconds: min(int(seconds * F0_SR / F0_HOP), len(stem) - 1)
    line = []
    for n in model_notes(work, 'bp_bass'):
        # a silent stem has the rest of the other instruments, and bp gives notes for it (a solo piano: about 100 in a minute)
        if stem[frame(n['on']):frame(n['off']) + 1].mean() <= floor:
            continue
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
AFTER_STEMS = dict(tk=run_transkun, pt=run_pt, bp=run_basic_pitch, bass=run_bass, lead=run_lead, chords=run_chords)


def cmd_step(a):
    (STEPS | AFTER_STEMS)[a.name](a.work)


def cmd_prepare(a):
    import soundfile as sf
    a.work.mkdir(parents=True, exist_ok=True)
    wav = a.work / 'src.wav'
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', a.audio,
                    '-ac', '2', '-ar', '44100', str(wav)], check=True)
    for cache in ('cqt.npy', 'cqt_lead.npy', 'f0_lead.npy'):
        (a.work / cache).unlink(missing_ok=True)

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
          + ' '.join(f'{stem} {rms(a.work / f"{stem}.wav") / mix:.0%}' for stem in ('drums', 'bass', 'other', 'vocals')))
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
    # the singer or the recording can be off A440: the circular mean of the distances to the nearest semitone
    off = pitch[voiced] - np.round(pitch[voiced])
    pitch = pitch - np.angle(np.exp(2j * np.pi * off).mean()) / (2 * np.pi)
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
        bars.setdefault(slot // grid.bar_slots + 1, []).append((slot % grid.bar_slots, length, pitch + 12 * octave, marks))
    return bars, early


def top_line(notes, floor):
    """The highest note of each slot among the (slot, pitch) notes from `floor` up."""
    top = {}
    for slot, pitch in notes:
        if pitch >= floor:
            top[slot] = max(pitch, top.get(slot, 0))
    return sorted(top.items())


def parallel_bars(line, other, bar_slots, octaves, share, least, instrument):
    """(bar, other bar, octaves up) where a bar of `line` and a bar of `other` have the same phrase; both are (slot, pitch) notes.
    Two bars pass when `share` of the notes of the bar are in the other bar, the octaves higher and at most PAR_REACH slots away,
    and PAR_SHARE of the notes of the other bar are in the bar. Of an instrument, the other bar is its top line from the lowest
    note of the bar: the chords below it are not the phrase. Bars count in a run of 2 or more at one distance and octave that
    shares PAR_NOTES notes. With an instrument, one bar between two passing bars is in the run: a bar where the voice is 2 slots
    from the instrument does not pass, and that bar needs the instrument most."""
    near = lambda notes: {(slot + d, pitch) for slot, pitch in notes for d in range(-PAR_REACH, PAR_REACH + 1)}
    bars = lambda notes: {bar: [n for n in notes if n[0] // bar_slots == bar] for bar in {slot // bar_slots for slot, _ in notes}}
    line_near, other_near, other_bars, shared = near(line), near(other), bars(other), {}
    for a, sung in bars(line).items():
        if len(sung) < least:
            continue
        low = min(pitch for _, pitch in sung)
        for b, played in other_bars.items():
            if a == b and not instrument:
                continue
            for octave in octaves:
                shift, up = (b - a) * bar_slots, 12 * octave
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
            out += [(a + i + 1, b + i + 1, octave) for i in range(run)]
    return sorted(out)


def sung_again(notes, grid):
    """Bar pairs where the voice sings one phrase again; each pair in both orders."""
    sung = [(slot, pitch) for slot, _, pitch, _ in notes if slot >= 0]
    return [(a, b) for a, b, _ in parallel_bars(sung, sung, grid.bar_slots, [0], PAR_SHARE, 3, False)]


def played_phrases(notes, work, grid):
    """(sung bar, bar of the instrument, octaves that the instrument is higher) where the shown notes of the models have the sung
    phrase, and the model letters of each shown note by (slot, pitch). The doubtful sung notes do not count."""
    sure = [(slot, pitch) for slot, _, pitch, marks in notes if slot >= 0 and '?' not in marks]
    played = {(round(n[0]), n[2]): n[6] for n in merged(viewed(combined(work), 'mix'), grid) if n[7] and round(n[0]) >= 0}
    return parallel_bars(sure, sorted(played), grid.bar_slots, range(-2, 3), INST_SHARE, 2, True), played


def cmd_tune(a):
    grid = Grid(a.work)
    width = sung_width(grid)
    notes = on_grid(octave_errors(sung_notes(*vocal_f0(a.work)), a.work), grid, width)
    if not notes:
        sys.exit('no sung notes in lead.wav')
    bars, early = line_bars(notes, grid, a.octave)
    again = sung_again(notes, grid)
    doubled, played = played_phrases(notes, a.work, grid)
    pitches = sorted(n[2] for n in notes)
    count = lambda mark: sum(mark in n[3] for n in notes)
    print(f'{len(notes) - early} sung notes in bars {bar_ranges(sorted(bars))}' + (f' and {early} before bar 1' if early else '') + f'; sung range {name(pitches[0])}-{name(pitches[-1])}, median {name(pitches[len(pitches) // 2])}'
          + (f', printed {a.octave:+d} octave' if a.octave else '') + f'; {count("^")} scoops (^), {count("~")} late in the slot (~), {count("?")} doubtful (?)')
    print(f'bar | slot:pitch/sixteenths, {grid.bar_slots} slots in a bar; ^ = starts with a scoop from below, written at the scoop; '
          f'~ = the onset is late in its slot; ' + ('the song is fast: the notes are on eighths (even slots); ' if width == 2 else '') + f'+ or - = sung {BETWEEN_C} cents or more above or below the pitch; ? = shorter than {WEAK_S} s, {SOFT_DB} dB softer than a note near it, an octave error, or an octave from the notes around it')
    if again:
        print('=N after a bar: the voice sings the phrase of the bar again in bar N')
    if doubled:
        print('`inst N` under a bar: an instrument plays the phrase of the bar in bar N; its notes as slot:pitch in the octave of the tune, with the model letters of `roll`')
    for bar in bar_list(a.bars) if a.bars else sorted(bars):
        same = [str(b) for x, b in again if x == bar]
        print(f'{bar} | ' + ' '.join(f'{slot}:{name(p)}/{length}{marks}' for slot, length, p, marks in bars.get(bar, []))
              + (' | =' + ','.join(same) if same else ''))
        sure = [p for _, _, p, marks in bars.get(bar, []) if '?' not in marks]
        instrument = [(b, octave) for x, b, octave in doubled if x == bar]
        if sure and instrument:
            b, octave = instrument[0]
            there = top_line([n for n in played if n[0] // grid.bar_slots == b - 1], min(sure) + 12 * (octave - a.octave))
            print(f'  inst {b} | ' + ' '.join(f'{slot % grid.bar_slots}:{name(p + 12 * (a.octave - octave))}{letters(played[slot, p])}' for slot, p in there))
    melody = {f'{bar}:{round(1 + slot / grid.slots, 3):g}': name(p) for bar in sorted(bars) for slot, _, p, marks in bars[bar] if '?' not in marks}
    (a.work / 'tune.json').write_text(json.dumps(dict(melody=melody), indent=1))
    print(f'draft melody without the doubtful notes: {(a.work / "tune.json").as_posix()} ({len(melody)} notes)')


# ---------------------------------------------------------------- bass line

def bass_line(work, grid):
    """The bass stem as [slot, sixteenths, pitch, marks] of on_grid. Empty when the stem is silent."""
    return on_grid([(n['on'], n['off'], n['p'], False, False, 0) for n in model_notes(work, 'bass')], grid)


def cmd_bass(a):
    grid = Grid(a.work)
    bars, _ = line_bars(bass_line(a.work, grid), grid, a.octave)
    if not bars:
        sys.exit('the bass stem is silent: read the bass from `roll`')
    print(f'bass stem: notes in bars {bar_ranges(sorted(bars))}; bar | slot:pitch/sixteenths, {grid.bar_slots} slots in a bar; '
          f'~ = the onset is late in its slot; ? = shorter than {WEAK_S} s, or an octave from the notes around it')
    for bar in bar_list(a.bars) if a.bars else sorted(bars):
        print(f'{bar} | ' + ' '.join(f'{slot}:{name(p)}/{length}{marks}' for slot, length, p, marks in bars.get(bar, [])))


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


def bar_phase(beats, downbeats, first_onset):
    """Beats in a bar, the index of the beat that starts bar 1 (the bar with the first note), and the share of downbeats that agree."""
    index = np.array([int(np.abs(beats - d).argmin()) for d in downbeats])
    per_bar = int(np.median(np.diff(index)))
    phase = int(np.bincount(index % per_bar).argmax())
    first_beat = int(np.searchsorted(beats, first_onset + 0.25 * np.median(np.diff(beats)), side='right')) - 1
    return per_bar, first_beat - (first_beat - phase) % per_bar, float(np.mean(index % per_bar == phase))


def tracked_grid(work, fields):
    """The beats from beat 1 of bar 1, after the grid fields. Returns a dict with the beats and what `grid` reports."""
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
    per_bar, first, agree = bar_phase(beats, np.array(tracked['downbeats']), on.min())
    quarters = 1
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
        else:
            sys.exit(f'unknown grid field {pair}; fields: {GRID_FIELDS}')
    while first < 0:
        beats, first = np.r_[2 * beats[0] - beats[1], beats], first + 1
    return dict(beats=snap(beats[first:], on, weight), per_bar=per_bar, quarters=quarters, agree=agree, added=added, on=on, weight=weight)


def cmd_grid(a):
    g = tracked_grid(a.work, a.set or [])
    beats, per_bar, on, weight = g['beats'], g['per_bar'], g['on'], g['weight']
    period = float(np.median(np.diff(beats)))
    index = np.arange(len(beats))
    beat_s, t0 = np.polyfit(index, beats, 1)
    constant = t0 + beat_s * index
    apart = np.abs(beats - constant)
    far_bars = sorted({int(i // per_bar) + 1 for i in index[apart > period / 8]})
    tempo = 60 / np.diff(beats)
    bar_of = lambda seconds: int(beat_at(beats, seconds) // per_bar) + 1
    print(f'tracked grid: beat {period:.3f} s = {60 / period:.0f} BPM ({np.percentile(tempo, 5):.0f} to {np.percentile(tempo, 95):.0f}), '
          f'{per_bar} beats in a bar ({g["agree"]:.0%} of the tracked downbeats agree), bar 1 at {beats[0]:.2f} s, {math.ceil(len(beats) / per_bar)} bars')
    if far_bars:
        print(f'constant grid: beat {beat_s:.4f} s from {t0:.3f} s; the tracked beats are more than half a sixteenth from it in '
              f'{len(far_bars)} bars ({bar_ranges(far_bars)}), at most {1000 * apart.max():.0f} ms: the tempo changes, use the tracked grid')
    else:
        print(f'constant grid: beat {beat_s:.4f} s from {t0:.3f} s; the tracked beats are less than half a sixteenth from it in all bars '
              f'(at most {1000 * apart.max():.0f} ms): steady, use --constant')
    if g['added'] is None:
        if not any(field.startswith('level=') for field in a.set or []):
            print('the tracker changed between two beat levels: the grid has the fast one (level=half gives the slow one)')
    else:
        added = [t for t in g['added'] if beats[0] <= t <= beats[-1]]
        if added:
            print(f'the tracker missed {len(added)} beats, added in bars {bar_ranges(sorted({bar_of(t) for t in added}))}: look at these bars with roll')
    position = beat_at(beats, on)
    lock = lambda division: abs(np.sum(weight * np.exp(2j * np.pi * position * division))) / weight.sum()
    print('onset lock by divisions of the beat: ' + ' '.join(f'{d}:{short(lock(d))}' for d in (1, 2, 3, 4, 6, 8)))
    if a.set is None and not a.constant:
        spectrum, times = source_cqt(a.work)
        profile = chroma(window(spectrum, times, 0, times[-1]))
        print('pitch classes, strongest first: ' + ' '.join(f'{NAMES[k]} {short(profile[k] / profile.max())}' for k in np.argsort(profile)[::-1][:8]))
        return
    chosen = constant if a.constant else beats
    (a.work / 'grid.json').write_text(json.dumps(dict(beats_per_bar=per_bar, quarters_per_beat=g['quarters'], beats=[round(float(b), 4) for b in chosen])))
    print(f'saved grid.json: the {"constant" if a.constant else "tracked"} grid, {per_bar} beats in a bar')


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
    first = (bar - 1) * grid.bar_slots
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


def chord(names, frames, start, end):
    """The most probable chord between two times, with ? where its probability is below UNSURE."""
    first, last = (round(max(seconds, 0) * CHORD_SR / CHORD_HOP) for seconds in (start, end))
    inside = frames[first:max(last, first + 1)]
    if not len(inside):
        return '-'
    mean = inside.mean(0)
    return names[mean.argmax()] + ('?' if mean.max() < UNSURE else '')


def bar_chord(names, frames, bar, grid):
    """The chord of the bar, or first>second where the two halves of a bar of an even meter have different chords."""
    edges = [grid.time((bar - 1 + k / 2) * grid.per_bar) for k in range(3)]
    halves = [chord(names, frames, a, b) for a, b in zip(edges, edges[1:])]
    if grid.per_bar % 2 or halves[0].rstrip('?') == halves[1].rstrip('?'):
        return chord(names, frames, edges[0], edges[2])
    return '>'.join(halves)


def cmd_roll(a):
    grid = Grid(a.work)
    bass = {slot // grid.bar_slots + 1: pitch for slot, _, pitch, _ in reversed(bass_line(a.work, grid))}
    notes = merged(viewed(combined(a.work), a.view), grid)
    hidden = Counter(int(round(n[0]) // grid.bar_slots) + 1 for n in notes if not n[7])
    chords = chord_frames(a.work)

    def in_bar(notes, bar):
        first = (bar - 1) * grid.bar_slots
        return [(round(s) - first, e - s, p, v, k, by) for s, e, p, v, k, _, by, show in notes if show and first <= round(s) < first + grid.bar_slots]

    def line(notes, lowest, highest):
        return ' '.join(f'{slot}:{name(p)}/{max(1, round(length))}' + ('!' if v >= STRONG else '') + (f'x{k}' if k >= 3 else '') + letters(by)
                        for slot, length, p, v, k, by in notes if lowest <= p < highest)

    print(f'bar chord bass | below {name(a.split)} | from {name(a.split)} | +hidden notes'
          + f'; view {a.view}: {VIEWS[a.view]}; slot:pitch/sixteenths, {grid.bar_slots} in a bar')
    for bar in bar_list(a.bars):
        low = bar_bass(notes, bass, bar, grid)
        shown = in_bar(notes, bar)
        print(f'{bar} {bar_chord(*chords, bar, grid)} {name(low) if low else "-"} | {line(shown, 0, a.split)} | {line(shown, a.split, 128)}'
              + (f' | +{hidden[bar]}' if hidden[bar] else ''))


def cmd_raw(a):
    grid = Grid(a.work)
    notes = viewed(combined(a.work), a.view)
    print(f'bar | beat:pitch/beats(velocity, or a and the bp amplitude) | +hidden notes; view {a.view}')
    for bar in bar_list(a.bars):
        first = (bar - 1) * grid.per_bar
        row, hidden = [], 0
        for n in notes:
            beat = grid.beat(n['on']) - first
            level = n['amp'] if n['v'] is None else n['v']
            if not (-0.06 <= beat < grid.per_bar - 0.06 and a.lo <= n['p'] <= a.hi and level >= a.min_velocity):
                continue
            if not n['show']:
                hidden += 1
                continue
            row.append(f"{beat + 1:.2f}:{name(n['p'])}/{grid.beat(n['off']) - grid.beat(n['on']):.1f}"
                       f"({'a' if n['v'] is None else ''}{level}){letters(n['by'])}")
        print(f'{bar} | ' + ' '.join(row) + (f' | +{hidden}' if hidden else ''))


def cmd_peek(a):
    grid = Grid(a.work)
    spectrum, times = source_cqt(a.work)
    print('one character is 1/8 beat; a space starts a beat, | starts a bar; from silent to loud: " .:-=+*#%@"')
    pitches = [midi(token) for token in a.pitches.split(',')]
    rows = {pitch: [] for pitch in pitches}
    for bar in bar_list(a.bars):
        for beat in range(grid.per_bar):
            start = (bar - 1) * grid.per_bar + beat
            columns = [window(spectrum, times, grid.time(start + k / 8), grid.time(start + (k + 1) / 8)) / spectrum.max() for k in range(8)]
            for pitch in pitches:
                rows[pitch].append(('|' if beat == 0 else ' ') + ''.join(' .:-=+*#%@'[min(9, int(15 * c[pitch - LOWEST_MIDI] ** 0.5))] for c in columns))
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


def cmd_compare(a):
    grid = Grid(a.work)
    export(a.score, a.work / 'score.mid')
    export(a.score, a.work / 'score.wav')
    # MuseScore plays the score at its first tempo: it does not play "rit.", and a later tempo mark is not handled here
    score, score_beat_s = score_notes(a.work / 'score.mid', grid)
    if not score:
        sys.exit(f'{a.score} has no notes')
    bars = math.ceil(max(end for _, end, _ in score) / grid.bar_slots - 1e-6)
    src, src_t = source_cqt(a.work)
    out, out_t = cqt(a.work / 'score.wav')
    both = lambda start, end: (window(src, src_t, grid.time(start), grid.time(end)),
                               window(out, out_t, start * score_beat_s, end * score_beat_s))
    full = []

    def beat_match(i):
        s, o = both(i, i + 1)
        return float(unit(chroma(s)) @ unit(chroma(o)))

    match = np.array([beat_match(i) for i in range(bars * grid.per_bar)]).reshape(bars, grid.per_bar).mean(1)
    print(f'HARMONY (beat chroma, octaves folded): mean {match.mean():.2f}; lowest bars: '
          + ' '.join(f'{i + 1}:{short(match[i])}' for i in np.argsort(match)[:8]))
    full.append('HARMONY of each bar: ' + ' '.join(f'{i + 1}:{short(m)}' for i, m in enumerate(match)))

    models = [(grid.slot(n['on']), n['p'], n['v']) for n in combined(a.work)]
    models += [(grid.slot(on), pitch, None) for on, _, pitch, *_ in sung_notes(*vocal_f0(a.work))]        # the note models do not read the voice
    near = lambda notes, slot, pitch, reach, any_octave=False: any(
        abs(s - slot) <= reach and (p == pitch or (any_octave and p % 12 == pitch % 12)) for s, p, *_ in notes)
    bar_of = lambda slot: int(round(slot) // grid.bar_slots) + 1
    place = lambda slot, pitch: f'{round(slot) % grid.bar_slots}:{name(pitch)}'

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
        tune = []
        for key, pitch in json.loads(Path(a.song).read_text())['melody'].items():
            bar, _, beat = key.partition(':')
            tune.append(((int(bar) - 1) * grid.per_bar + float(beat) - 1, midi(pitch)))
    else:
        tune = [(start / grid.slots, pitch) for start, pitch in top_line([(s, p) for s, _, p in score], MELODY_FLOOR)]
    weak = []
    for beat, pitch in tune:
        s = window(src, src_t, grid.time(beat), grid.time(beat + 0.5))
        k = pitch - LOWEST_MIDI
        low = max(0, k - 12)
        octave, close = s[low:k + 13], s[max(0, k - 2):k + 3]
        is_weak = s[k] < 0.25 * octave.max() or s[k] < 0.7 * close.max()
        louder = name(int(close.argmax()) + max(0, k - 2) + LOWEST_MIDI) if s[k] < 0.7 * close.max() else name(int(octave.argmax()) + low + LOWEST_MIDI)
        text = f'{int(beat // grid.per_bar) + 1}:{beat % grid.per_bar + 1:g} {name(pitch)}' + (f'<{louder}' if is_weak else '')
        full.append(f'MELODY {text} level {short(s[k] / (octave.max() + 1e-9))}')
        if is_weak:
            weak.append(text)
    print(f'MELODY ({"the tune of the song file" if a.song else f"top score notes from {name(MELODY_FLOOR)}"}): {len(weak)} of {len(tune)} tune notes are weak '
          'in the recording at their onset (bar:beat pitch<louder pitch near it)' + (':' if weak else ''))
    if weak:
        print('  ' + ' | '.join(weak[:2 * a.rows]) + (f' | ... {len(weak) - 2 * a.rows} more' if len(weak) > 2 * a.rows else ''))

    for bar in bar_list(a.bars) if a.bars else []:
        s, o = (chroma(v) for v in both((bar - 1) * grid.per_bar, bar * grid.per_bar))
        print(f'bar {bar} pitch classes, source: ' + ' '.join(f'{NAMES[k]} {short(s[k] / s.max())}' for k in np.argsort(s)[::-1][:6])
              + ' | score: ' + ' '.join(f'{NAMES[k]} {short(o[k] / o.max())}' for k in np.argsort(o)[::-1][:6]))
    write_detail(a.work, 'compare.txt', full)


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
        findings['melody notes that are not in the score'] = [
            f'{at(*key)} {name(pitch)}' for key, pitch in sorted(tune.items()) if pitch not in heard.get(key, [])]
        findings['melody notes below another note (an octave doubling above is not counted)'] = [
            f'{at(*key)} {name(pitch)} below {name(max(heard[key]))}' for key, pitch in sorted(tune.items())
            if pitch in heard.get(key, []) and max(heard[key]) % 12 != pitch % 12]
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

    times = sorted({clock(bar, beat): (bar, beat) for bar, beat, ln, _, rn, _ in rows if ln or rn}.items())
    findings['dead spots: 2 beats or more with no new note'] = [
        f'bar {bar} beat {float(beat) + 1:g}: {float(b0 - a0):g} beats'
        for (a0, (bar, beat)), (b0, _) in zip(times, times[1:]) if b0 - a0 >= 2 and bar not in free]

    left = [(bar, beat, ln) for bar, beat, ln, _, _, _ in rows if ln]
    findings['left-hand leaps larger than an octave inside a bar (the leap from the bass note is not counted)'] = [
        f'{at(bar, beat)} {name(before[-1])}->{name(ln[0])}'
        for (bar0, beat0, before), (bar, beat, ln) in zip(left, left[1:])
        if bar0 == bar and beat0 != 0 and min(abs(x - y) for x in ln for y in before) > 12]

    findings['one-hand spans larger than a tenth'] = [
        f'{at(bar, beat)} {hand} {name(min(notes))}-{name(max(notes))}'
        for bar, beat, ln, lh, rn, rh in rows for hand, notes in (('lh', ln + lh), ('rh', rn + rh))
        if notes and max(notes) - min(notes) > 16]

    right = [(clock(bar, beat), bar, beat, rn) for bar, beat, _, _, rn, _ in rows if rn]
    findings['right-hand jumps larger than an octave within one beat'] = [
        f'{at(bar, beat)} {name(max(before))}->{name(max(rn))}'
        for (t0, _, _, before), (t1, bar, beat, rn) in zip(right, right[1:])
        if t1 - t0 <= 1 and abs(min(rn) - min(before)) > 12 and abs(max(rn) - max(before)) > 12]

    findings['hands overlap or cross'] = [at(bar, beat) for bar, beat, ln, lh, rn, rh in rows
                                          if ln + lh and rn + rh and max(ln + lh) >= min(rn + rh)]

    figure = lambda bar: [(beat, sorted({p % 12 for p in ln})) for b, beat, ln in left if b == bar]
    if 'parallel' in song:
        findings['parallel bars with a different left-hand figure'] = [
            f'{x}/{y}' for x, y in song['parallel'] if figure(x) != figure(y)]

    rhythms = {}
    for bar in sorted({bar for bar, *_ in rows} - free):
        rhythms.setdefault(tuple(float(beat) + 1 for beat, _ in figure(bar)), []).append(bar)
    findings['different left-hand rhythms (free bars not counted)'] = [
        f'{len(bars)}x {" ".join(f"{b:g}" for b in rhythm) or "none"}' for rhythm, bars in sorted(rhythms.items(), key=lambda kv: -len(kv[1]))]

    if 'chords' in song:
        tones = {int(bar): {midi(t + '4') % 12 for t in chord.split()} for bar, chord in song['chords'].items()}
        inside = lambda bar, pitch: bar not in tones or pitch % 12 in tones[bar]
        findings['left-hand notes outside the chord of the bar'] = [
            f'{at(bar, beat)} {name(p)}' for bar, beat, ln in left for p in ln if not inside(bar, p)]
        tops = [(bar, beat, max(rn)) for _, bar, beat, rn in right]
        findings['top notes on a beat, outside the chord, that do not move by step to a chord note'] = [
            f'{at(bar, beat)} {name(top)}->{name(after)}'
            for (bar, beat, top), (bar1, _, after) in zip(tops, tops[1:])
            if beat.denominator == 1 and not inside(bar, top) and (abs(after - top) > 2 or not inside(bar1, after))]

    for label, items in findings.items():
        if items:
            print(f'[{len(items)}] {label}: ' + ' | '.join(items[:a.rows]) + (f' | ... {len(items) - a.rows} more' if len(items) > a.rows else ''))
    clean = [label for label, items in findings.items() if not items]
    if clean:
        print('[0] ' + '; '.join(clean))
    write_detail(a.work, 'check.txt', [f'[{len(items)}] {label}: ' + ' | '.join(items) for label, items in findings.items()])


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
    command(cmd_prepare, 'convert the audio, split stems and track beats, then run the note models on the stems', (['audio'], {}))
    command(cmd_step, 'one step of prepare', (['name'], dict(choices=list(STEPS | AFTER_STEMS))))
    command(cmd_grid, 'the tracked grid, the constant grid, and where they differ; --set or --constant saves grid.json',
            (['--constant'], dict(action='store_true', help='save the constant grid')),
            (['--set'], dict(nargs='*', metavar='FIELD=VALUE', help=f'save the grid; fields: {GRID_FIELDS}')))
    command(cmd_roll, 'merged model notes of each bar', bars,
            view,
            (['--split'], dict(type=midi, default=59, help='lowest pitch of the high notes (default B3)')))
    command(cmd_raw, 'model notes of each bar with exact beat positions and velocities, re-onsets not merged', bars,
            view,
            (['--lo'], dict(type=midi, default=21)), (['--hi'], dict(type=midi, default=108)),
            (['--min-velocity'], dict(type=int, default=40)))
    command(cmd_bass, 'the bass line of the bass stem, one line for each bar',
            (['bars'], dict(nargs='?', help='bars, for example 5-8,12 (default: each bar with bass notes)')),
            (['--octave'], dict(type=int, default=0, help='print the line this many octaves higher')))
    command(cmd_tune, 'the sung tune of the vocal stem, one line for each bar; writes a draft melody to tune.json',
            (['bars'], dict(nargs='?', help='bars, for example 5-8,12 (default: each sung bar)')),
            (['--octave'], dict(type=int, default=0, help='print and save the tune this many octaves higher')))
    command(cmd_peek, 'loudness of given pitches through time', bars, (['pitches'], dict(help='for example F#5,G5')))
    command(cmd_compare, 'compare a score with the recording', (['score'], dict(type=Path)), rows,
            (['--song'], dict(help='JSON with the melody: MELODY tests these notes')),
            (['--bars'], dict(help='also print pitch-class profiles of these bars')))
    command(cmd_check, 'piano-idiom checks of a score', (['score'], dict(type=Path)), rows,
            (['--song'], dict(help='JSON with melody, chords, parallel and free_bars')))
    arguments = parser.parse_args()
    arguments.run(arguments)


if __name__ == '__main__':
    main()
