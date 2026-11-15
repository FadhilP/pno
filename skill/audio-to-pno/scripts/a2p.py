"""Evidence and checks for a piano transcription of a recording into pno. The workflow is in SKILL.md."""
import argparse, json, math, re, subprocess, sys, tempfile
from fractions import Fraction
from pathlib import Path

import numpy as np

SR, HOP, LOWEST_MIDI, BINS = 22050, 512, 24, 84
NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B']
STEP = {'C': 0, 'D': 2, 'E': 4, 'F': 5, 'G': 7, 'A': 9, 'B': 11}
STRONG = 85          # model velocity that marks a prominent note
MELODY_FLOOR = 69    # the melody check looks at A4 and higher


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


def by_bar(items):
    groups = {}
    for bar, text in items:
        groups.setdefault(bar, []).append(text)
    return [f'  bar {bar}: ' + ' '.join(groups[bar]) for bar in sorted(groups)]


class Grid:
    """The bar grid of one song, saved by `grid --set` and read by every later command."""

    def __init__(self, work):
        path = work / 'grid.json'
        if not path.exists():
            sys.exit(f'{path} is missing: run `grid`, then `grid --set`')
        g = json.loads(path.read_text())
        self.beat_s, self.t0, self.per_bar = g['beat_s'], g['t0'], g['beats_per_bar']
        self.quarters = g.get('quarters_per_beat', 1)
        self.slots = 6 if self.quarters == 1.5 else 4      # sixteenths in one beat

    def slot(self, seconds):
        return (seconds - self.t0) / self.beat_s * self.slots

    def bar_start(self, bar):
        return self.t0 + (bar - 1) * self.per_bar * self.beat_s

    @property
    def bar_slots(self):
        return self.per_bar * self.slots


def model_notes(work, source):
    path = work / f'{source}.json'
    if not path.exists():
        sys.exit(f'{path} is missing: run `prepare`' + ('' if source in ('pt', 'bp') else f', then `bp {source[3:]}`'))
    return sorted(json.loads(path.read_text())['notes'], key=lambda n: (n['on'], n['p']))


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

def run_basic_pitch(work, stem):
    from basic_pitch import ICASSP_2022_MODEL_PATH
    from basic_pitch.inference import predict
    _, _, events = predict(str(work / f'{stem}.wav'), ICASSP_2022_MODEL_PATH, minimum_note_length=80)
    notes = [dict(on=float(s), off=float(e), p=int(p), v=int(round(amp * 127))) for s, e, p, amp, _ in events]
    target = work / ('bp.json' if stem == 'src' else f'bp_{stem}.json')
    target.write_text(json.dumps(dict(notes=notes)))
    print(f'{target.name}: {len(notes)} notes')


def cmd_prepare(a):
    import librosa, soundfile as sf, torch
    from demucs.apply import apply_model
    from demucs.pretrained import get_model
    from piano_transcription_inference import PianoTranscription, sample_rate

    a.work.mkdir(parents=True, exist_ok=True)
    wav = a.work / 'src.wav'
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', a.audio,
                    '-ac', '2', '-ar', '44100', str(wav)], check=True)
    (a.work / 'cqt.npy').unlink(missing_ok=True)

    mix, sr = sf.read(wav, dtype='float32')
    model = get_model('htdemucs')
    model.eval()
    x = torch.from_numpy(mix.T)[None]
    ref = x.mean(1)
    with torch.no_grad():
        stems = apply_model(model, (x - ref.mean()) / ref.std(), device='cpu', split=True, overlap=0.25)[0]
    stems = stems * ref.std() + ref.mean()
    rms = lambda s: float(np.sqrt((s ** 2).mean()))
    print(f'duration {len(mix) / sr:.1f} s')
    for stem, signal in zip(model.sources, stems):
        signal = signal.numpy().T
        sf.write(a.work / f'{stem}.wav', signal, sr)
        print(f'stem {stem}: {rms(signal) / rms(mix):.0%} of the mix level')

    audio, _ = librosa.load(wav, sr=sample_rate, mono=True)
    result = PianoTranscription(device='cpu', checkpoint_path=None).transcribe(audio, str(a.work / 'pt.mid'))
    notes = [dict(on=float(e['onset_time']), off=float(e['offset_time']), p=int(e['midi_note']), v=int(e['velocity']))
             for e in result['est_note_events']]
    (a.work / 'pt.json').write_text(json.dumps(dict(notes=notes)))
    last_onset, again = {}, 0
    for n in sorted(notes, key=lambda n: n['on']):
        again += n['on'] - last_onset.get(n['p'], -9) < 0.25
        last_onset[n['p']] = n['on']
    print(f'pt.json: {len(notes)} notes; {again / max(1, len(notes)):.0%} repeat a pitch within 0.25 s')
    run_basic_pitch(a.work, 'src')


def cmd_bp(a):
    run_basic_pitch(a.work, a.stem)


# ---------------------------------------------------------------- grid

def cmd_grid(a):
    import librosa
    notes = model_notes(a.work, 'pt')
    on = np.array([n['on'] for n in notes])
    vel = np.array([n['v'] for n in notes], float)
    low = np.array([n['p'] < 55 for n in notes])

    def lock(period, weights=vel):          # how well the onsets sit on a grid of this period, and its phase
        z = np.sum(weights * np.exp(2j * np.pi * on / period))
        return abs(z) / weights.sum(), (np.angle(z) / (2 * np.pi) * period) % period

    if a.beat_s:
        centre = a.beat_s
    else:
        y, _ = librosa.load(a.work / 'src.wav', sr=SR, mono=True)
        centre = 60 / float(np.atleast_1d(librosa.beat.beat_track(y=y, sr=SR)[0])[0])
    beat = float(max(np.arange(centre * 0.94, centre * 1.06, 0.0002), key=lambda t: lock(t / 4)[0]))
    print(f'beat period {beat:.4f} s = {60 / beat:.1f} BPM (searched near {centre:.3f} s; --beat-s tests half or double)')
    print('onset lock by divisions of the beat:', '  '.join(f'{d}: {lock(beat / d)[0]:.2f}' for d in (1, 2, 3, 4, 6, 8)))

    sixteenth = beat / 4
    phase = lock(sixteenth)[1]
    first = on.min()
    starts = [phase + k * sixteenth for k in range(-4, 8)]
    starts = [s for s in starts if s >= first - sixteenth / 2][:4]      # the 4 possible places of a beat
    bass_weight = [sum(vel[low & (np.abs((on - s + beat / 2) % beat - beat / 2) < sixteenth / 2)]) for s in starts]
    beat0 = starts[int(np.argmax(bass_weight))]
    print('bass weight on each sixteenth of the beat:', ' '.join(f'{w:.0f}' for w in bass_weight), f'-> a beat is at {beat0:.4f} s')

    spectrum, times = source_cqt(a.work)
    count = int((times[-1] - beat0) / beat)
    beats = np.array([window(spectrum, times, beat0 + i * beat, beat0 + (i + 1) * beat) for i in range(count)])
    change = lambda rows: np.r_[0, np.linalg.norm(np.diff(rows / (rows.sum(1, keepdims=True) + 1e-9), axis=0), axis=1)]
    harmony, bass = change(np.array([chroma(b) for b in beats])), change(beats[:, :24])
    print('change of harmony and of bass at each beat of a candidate bar (a downbeat is high):')
    best = None
    for per_bar in (2, 3, 4, 6):
        rows = [[float(x[k::per_bar].mean()) for k in range(per_bar)] for x in (harmony, bass)]
        total = np.add(*rows)
        contrast = float(total.max() / np.median(total))
        print(f'  {per_bar} beats: harmony {" ".join(f"{x:.2f}" for x in rows[0])} | bass {" ".join(f"{x:.2f}" for x in rows[1])} | contrast {contrast:.2f}')
        if best is None or contrast > best[0] * 1.1:        # a longer bar must be clearly better
            best = (contrast, per_bar, int(total.argmax()))
    _, per_bar, downbeat = best
    t0 = beat0 + (downbeat - per_bar if downbeat else 0) * beat        # bar 1 holds the first note; no pickup bar

    drift = []
    for start in np.arange(0, on.max(), 10):
        part = (on >= start) & (on < start + 10)
        if part.sum() >= 8:
            z = np.sum(vel[part] * np.exp(2j * np.pi * (on[part] - phase) / sixteenth))
            drift.append(np.angle(z) / (2 * np.pi) * sixteenth * 1000)
    spread = max(drift) - min(drift) if drift else 0
    verdict = 'steady' if spread < 250 * sixteenth else 'NOT steady: a constant grid does not fit this recording'
    print(f'grid offset in each 10 s window: {" ".join(f"{d:+.0f}" for d in drift)} ms; spread {spread:.0f} ms of a {1000 * sixteenth:.0f} ms sixteenth -> {verdict}')
    profile = chroma(window(spectrum, times, 0, times[-1]))
    print('pitch classes, strongest first:', ' '.join(f'{NAMES[k]}:{profile[k] / profile.max():.2f}' for k in np.argsort(profile)[::-1]))

    grid = dict(beat_s=round(beat, 4), t0=round(float(t0), 4), beats_per_bar=per_bar, quarters_per_beat=1)
    print('suggestion:', json.dumps(grid))
    if a.set is not None:
        for pair in a.set:
            key, _, value = pair.partition('=')
            if key not in grid:
                sys.exit(f'unknown grid field {key}; fields: {", ".join(grid)}')
            grid[key] = float(value) if '.' in value else int(value)
        (a.work / 'grid.json').write_text(json.dumps(grid))
        print('saved:', json.dumps(grid))


# ---------------------------------------------------------------- evidence

def merged(notes, grid):
    """Notes as [start slot, end slot, pitch, velocity, onsets], where fast re-onsets of one pitch are one note."""
    out, last = [], {}
    for n in sorted(notes, key=lambda n: (n['p'], n['on'])):
        start, end = grid.slot(n['on']), grid.slot(n['off'])
        prev = last.get(n['p'])
        if prev and (start - prev[1] <= 0.3 * grid.slots or start - prev[5] <= 0.35 * grid.slots):
            prev[1], prev[3], prev[4], prev[5] = max(prev[1], end), max(prev[3], n['v']), prev[4] + 1, start
            continue
        last[n['p']] = [start, end, n['p'], n['v'], 1, start]
        out.append(last[n['p']])
    return sorted(out)


TRIADS = {NAMES[root] + suffix: unit(np.bincount([(root + i) % 12 for i in steps], minlength=12).astype(float))
          for root in range(12) for suffix, steps in (('', (0, 4, 7)), ('m', (0, 3, 7)))}


def cmd_roll(a):
    grid = Grid(a.work)
    spectrum, times = source_cqt(a.work)
    sources = {'pt': merged(model_notes(a.work, 'pt'), grid), 'bp': merged(model_notes(a.work, 'bp'), grid)}
    if a.extra:
        sources[a.extra] = merged(model_notes(a.work, a.extra), grid)

    def line(notes, bar, lowest, highest):
        first = (bar - 1) * grid.bar_slots
        return ' '.join(f'{round(s) - first}:{name(p)}/{max(1, round(e - s))}' + ('!' if v >= STRONG else '') + (f'x{k}' if k >= 3 else '')
                        for s, e, p, v, k, _ in notes if first <= round(s) < first + grid.bar_slots and lowest <= p < highest)

    print(f'slot:pitch/length in sixteenths ({grid.bar_slots} in a bar); ! strong; xN is N fast re-onsets (a sustained or tremolo note)')
    for bar in bar_list(a.bars):
        bins = window(spectrum, times, grid.bar_start(bar), grid.bar_start(bar + 1))
        fit = sorted(((float(t @ unit(chroma(bins))), chord) for chord, t in TRIADS.items()), reverse=True)
        print(f'== bar {bar}  {grid.bar_start(bar):.2f} s  triad guess {fit[0][1]} {fit[0][0]:.2f}, {fit[1][1]} {fit[1][0]:.2f}  strongest low pitch {name(int(bins[:29].argmax()) + LOWEST_MIDI)}')
        for source, notes in sources.items():
            print(f'  {source} high: {line(notes, bar, a.split, 128)}')
            print(f'  {source} low : {line(notes, bar, 0, a.split)}')


def cmd_raw(a):
    grid = Grid(a.work)
    for bar in bar_list(a.bars):
        first = (bar - 1) * grid.per_bar
        for source in ('pt', 'bp'):
            row = []
            for n in model_notes(a.work, source):
                beat = grid.slot(n['on']) / grid.slots - first
                if -0.06 <= beat < grid.per_bar - 0.06 and a.lo <= n['p'] <= a.hi and n['v'] >= a.min_velocity:
                    row.append(f"{beat + 1:.2f}:{name(n['p'])}/{(n['off'] - n['on']) / grid.beat_s:.1f}({n['v']})")
            print(f'bar {bar} {source}: beat:pitch/beats(velocity)  ' + ' '.join(row))


def cmd_peek(a):
    grid = Grid(a.work)
    spectrum, times = source_cqt(a.work)
    bars = bar_list(a.bars)
    start, end = grid.bar_start(bars[0]), grid.bar_start(bars[-1] + 1)
    frames = (times >= start) & (times < end)
    on_beat = np.diff(np.floor((times[frames] - start) / grid.beat_s), prepend=-1) > 0
    print('     ' + ''.join('|' if b else ' ' for b in on_beat) + '  (| is a beat)')
    for pitch in map(midi, a.pitches.split(',')):
        row = spectrum[pitch - LOWEST_MIDI, frames] / spectrum.max()
        print(name(pitch).ljust(5) + ''.join(' .:-=+*#%@'[min(9, int(15 * x ** 0.5))] for x in row))


# ---------------------------------------------------------------- compare

def export(score, target):
    target.unlink(missing_ok=True)      # `pno export` can report success when it could not replace the file
    subprocess.run(['pno', 'export', str(score), str(target)], check=True, capture_output=True)
    if not target.exists():
        sys.exit(f'pno export did not write {target}')


def score_notes(path, grid):
    import mido
    mid = mido.MidiFile(path)
    to_slot = grid.slots / mid.ticks_per_beat / grid.quarters
    tick, sounding, notes = 0, {}, []
    for message in mido.merge_tracks(mid.tracks):
        tick += message.time
        if message.type == 'note_on' and message.velocity > 0:
            sounding[message.note] = tick
        elif message.type in ('note_off', 'note_on') and message.note in sounding:
            notes.append((sounding.pop(message.note) * to_slot, tick * to_slot, message.note))
    return sorted(notes)


def cmd_compare(a):
    grid = Grid(a.work)
    export(a.score, a.work / 'score.mid')
    export(a.score, a.work / 'score.wav')
    score = score_notes(a.work / 'score.mid', grid)
    bars = math.ceil(max(end for _, end, _ in score) / grid.bar_slots - 1e-6)
    src, src_t = source_cqt(a.work)
    out, out_t = cqt(a.work / 'score.wav')
    both = lambda start, end: (window(src, src_t, grid.t0 + start * grid.beat_s, grid.t0 + end * grid.beat_s),
                               window(out, out_t, start * grid.beat_s, end * grid.beat_s))

    def beat_match(i):
        s, o = both(i, i + 1)
        return float(unit(chroma(s)) @ unit(chroma(o)))

    match = np.array([beat_match(i) for i in range(bars * grid.per_bar)]).reshape(bars, grid.per_bar).mean(1)
    print(f'HARMONY (beat chroma, octaves folded): mean {match.mean():.3f}; lowest bars: '
          + ' '.join(f'{i + 1}:{match[i]:.2f}' for i in np.argsort(match)[:8]))

    models = [[(grid.slot(n['on']), n['p'], n['v']) for n in model_notes(a.work, source)] for source in ('pt', 'bp')]
    near = lambda notes, slot, pitch, reach, any_octave=False: any(
        abs(s - slot) <= reach and (p == pitch or (any_octave and p % 12 == pitch % 12)) for s, p, *_ in notes)
    bar_of = lambda slot: int(round(slot) // grid.bar_slots) + 1
    place = lambda slot, pitch, known: f'{round(slot) % grid.bar_slots}:{name(pitch)}' + ('' if known else '!!')
    added = [(bar_of(s), place(s, p, any(near(m, s, p, 1.1, True) for m in models)))
             for s, _, p in score if not any(near(m, s, p, 1.1) for m in models)]
    print(f'NOTES: {len(added)} of {len(score)} score notes have no model note at that pitch and time (!! in no octave):')
    print('\n'.join(by_bar(added)))
    strong = [(s, p) for s, p, v in models[0] if v >= STRONG]
    written = [(start, pitch) for start, _, pitch in score]
    missed = [(bar_of(s), place(s, p, near(written, s, p, 1.6, True))) for s, p in strong if not near(written, s, p, 1.6)]
    print(f'{len(missed)} of {len(strong)} strong piano-model notes are not in the score at that pitch (!! in no octave):')
    print('\n'.join(by_bar(missed)))

    print(f'MELODY (half beats, {name(MELODY_FLOOR)} and higher, octaves not folded); TOP marks a weak top note:')
    half = grid.slots / 2
    for i in range(bars * grid.per_bar * 2):
        s, o = (v[MELODY_FLOOR - LOWEST_MIDI:] for v in both(i / 2, (i + 1) / 2))
        held = sorted({p for start, end, p in score if start < (i + 0.9) * half and end > (i + 0.1) * half and p >= MELODY_FLOOR})
        if not held or not s.max():
            continue
        weak = [p for p in held if s[p - MELODY_FLOOR] < 0.25 * s.max()]
        similarity = float(unit(s) @ unit(o))
        if weak or similarity < 0.6:
            loudest = [name(int(k) + MELODY_FLOOR) for k in np.argsort(s)[::-1][:3]]
            print(f'  bar {i // (2 * grid.per_bar) + 1} beat {1 + (i % (2 * grid.per_bar)) / 2:g}: score {" ".join(map(name, held))}'
                  f' | weak {" ".join(map(name, weak)) or "-"}{" TOP" if held[-1] in weak else ""}'
                  f' | source loudest {" ".join(loudest)} | match {similarity:.2f}')

    for bar in bar_list(a.bars) if a.bars else []:
        s, o = (chroma(v) for v in both((bar - 1) * grid.per_bar, bar * grid.per_bar))
        print(f'bar {bar} pitch classes  source: ' + ' '.join(f'{NAMES[k]}:{s[k] / s.max():.1f}' for k in np.argsort(s)[::-1][:7])
              + '  | score: ' + ' '.join(f'{NAMES[k]}:{o[k] / o.max():.1f}' for k in np.argsort(o)[::-1][:7]))


# ---------------------------------------------------------------- check

def meter(score_text):
    """Beats in a bar, and quarter notes in a beat, from the first time signature."""
    m = re.search(r'^@\s*time=(\S+)', score_text, re.M)
    if not m or m[1] == 'c':
        return 4, 1
    if m[1] == 'cut':
        return 2, 2
    upper, lower = map(int, re.match(r'(\d+)/(\d+)', m[1]).groups())
    if lower >= 8:
        return (upper // 3, 12 / lower) if upper % 3 == 0 else (upper // 2, 8 / lower)
    return upper, 4 / lower


def struck_velocities(score_path, quarters):
    """Pitch and velocity of the notes that start at each beat, counted from the start of the score."""
    import mido
    with tempfile.TemporaryDirectory() as folder:
        target = Path(folder) / 'score.mid'
        export(score_path, target)
        mid = mido.MidiFile(target)
    tick, struck = 0, {}
    for message in mido.merge_tracks(mid.tracks):
        tick += message.time
        if message.type == 'note_on' and message.velocity > 0:
            beat = (Fraction(tick, mid.ticks_per_beat) / Fraction(quarters)).limit_denominator(48)
            struck.setdefault(beat, []).append((message.note, message.velocity))
    return struck


def onsets(score_path):
    """One row per onset from `pno sound`: bar, beat from 0, then new and held pitches of each hand."""
    text = Path(score_path).read_text()
    with tempfile.TemporaryDirectory() as folder:
        plain = Path(folder) / 'score.pno'
        plain.write_text(''.join(line for line in text.splitlines(True) if '.8va' not in line))   # `pno sound` adds an octave under 8va
        report = subprocess.run(['pno', 'sound', str(plain)], capture_output=True, text=True, check=True).stdout
    rows = []
    for line in report.splitlines():
        m = re.match(r'(\d+) ([\d+/]+)\s+lh(.*?)\| rh([^\[]*)', line)
        if not m:
            continue
        hands = []
        for side in (m[3], m[4]):
            tokens = [t for t in side.split() if t != '-']
            hands += [[midi(t) for t in tokens if not t.endswith('~')], [midi(t.rstrip('~')) for t in tokens if t.endswith('~')]]
        rows.append((int(m[1]), sum(Fraction(x) for x in m[2].split('+')) - 1, *hands))
    return rows, *meter(text)


def cmd_check(a):
    rows, per_bar, quarters = onsets(a.score)
    song = json.loads(Path(a.song).read_text()) if a.song else {}
    free = set(bar_list(song['free_bars'])) if song.get('free_bars') else set()
    at = lambda bar, beat: f'{bar}:{float(beat) + 1:g}'
    clock = lambda bar, beat: (bar - 1) * per_bar + beat
    findings = {}

    if 'melody' in song:
        tune = {}
        for key, pitch in song['melody'].items():
            bar, beat = key.split(':')
            tune[(int(bar), Fraction(beat) - 1)] = midi(pitch)
        heard = {(bar, beat): ln + lh + rn + rh for bar, beat, ln, lh, rn, rh in rows}
        findings['melody notes that are not in the score'] = [
            f'{at(*key)} {name(pitch)}' for key, pitch in sorted(tune.items()) if pitch not in heard.get(key, [])]
        findings['melody notes below another note (an octave doubling above is not counted)'] = [
            f'{at(*key)} {name(pitch)} below {name(max(heard[key]))}' for key, pitch in sorted(tune.items())
            if pitch in heard.get(key, []) and max(heard[key]) % 12 != pitch % 12]
        struck = struck_velocities(a.score, quarters)
        quiet = []
        for key, pitch in sorted(tune.items()):
            together = struck.get(clock(*key), [])
            own = [v for p, v in together if p == pitch]
            others = [v for p, v in together if p % 12 != pitch % 12]
            if own and others and max(own) <= max(others):
                quiet.append(f'{at(*key)} {name(pitch)} {max(own)} against {max(others)}')
        findings['melody notes not louder than the other notes struck with them (velocity)'] = quiet
        velocities = [v for notes in struck.values() for _, v in notes]
        print(f'velocities in the score: {min(velocities)} to {max(velocities)}')

    times = sorted({clock(bar, beat) for bar, beat, ln, _, rn, _ in rows if ln or rn})
    findings['dead spots: 2 beats or more with no new note'] = [
        f'bar {int(a0 // per_bar) + 1} beat {float(a0 % per_bar) + 1:g}: {float(b0 - a0):g} beats'
        for a0, b0 in zip(times, times[1:]) if b0 - a0 >= 2 and int(a0 // per_bar) + 1 not in free]

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
        print(f'[{len(items):2d}] {label}: {" | ".join(items) or "-"}')


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
    command(cmd_prepare, 'convert the audio, split stems, run both note models', (['audio'], {}))
    command(cmd_bp, 'run the sustained-note model on one stem', (['stem'], dict(help='vocals, bass, other or drums')))
    command(cmd_grid, 'measure tempo, beat, bar length and drift; --set saves grid.json',
            (['--beat-s'], dict(type=float, help='search near this beat period')),
            (['--set'], dict(nargs='*', metavar='FIELD=VALUE', help='save the suggestion, with these fields replaced')))
    command(cmd_roll, 'model notes of each bar', bars,
            (['--split'], dict(type=midi, default=59, help='lowest pitch of the high rows (default B3)')),
            (['--extra'], dict(help='one more note file, for example bp_vocals')))
    command(cmd_raw, 'model notes of each bar, not merged, in beats', bars,
            (['--lo'], dict(type=midi, default=21)), (['--hi'], dict(type=midi, default=108)),
            (['--min-velocity'], dict(type=int, default=40)))
    command(cmd_peek, 'loudness of given pitches through time', bars, (['pitches'], dict(help='for example F#5,G5')))
    command(cmd_compare, 'compare a score with the recording', (['score'], dict(type=Path)),
            (['--bars'], dict(help='also print pitch-class profiles of these bars')))
    command(cmd_check, 'piano-idiom checks of a score', (['score'], dict(type=Path)),
            (['--song'], dict(help='JSON with melody, chords, parallel and free_bars')))
    arguments = parser.parse_args()
    arguments.run(arguments)


if __name__ == '__main__':
    main()
