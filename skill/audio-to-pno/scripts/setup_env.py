"""Create the analysis environment outside the skill folder, and download the models."""
import shutil, subprocess, sys, urllib.request, venv
from pathlib import Path

CACHE = Path.home() / '.cache' / 'audio-to-pno'
ENV = CACHE / 'venv'
PYTHON = ENV / ('Scripts/python.exe' if sys.platform == 'win32' else 'bin/python')
CUDA = 'cu130'      # CUDA 13.0 builds also run on the newest cards (sm_120)
MODELS = {
    # piano_transcription_inference fetches this file with `wget`, which Windows does not have
    Path.home() / 'piano_transcription_inference_data' / 'note_F1=0.9677_pedal_F1=0.9186.pth':
        'https://zenodo.org/record/4034264/files/CRNN_note_F1%3D0.9677_pedal_F1%3D0.9186.pth?download=1',
    # beat_this puts it in the torch hub cache if a2p.py does not give it this file
    CACHE / 'beat_this-final0.ckpt': 'https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp/final0.ckpt',
    # the chord model of `roll` (BTC, MIT license), at a fixed commit
    CACHE / 'btc_model_large_voca.pt': 'https://raw.githubusercontent.com/jayg996/BTC-ISMIR19/2682317be668032e6e4b269ded36adaa2ad57df0/test/btc_model_large_voca.pt',
}

if not PYTHON.exists():
    venv.create(ENV, with_pip=True)
# the pip of Python 3.10 cannot build `sox`, which transkun installs
subprocess.run([str(PYTHON), '-m', 'pip', 'install', '-q', '--upgrade', 'pip'], check=True)
requirements = Path(__file__).resolve().parent.parent / 'requirements.txt'
if shutil.which('nvidia-smi'):
    # the CUDA build of the pinned torch: the models then run on the GPU (a2p.device); it satisfies the pin of requirements.txt
    torch = next(line.strip() for line in requirements.read_text().splitlines() if line.startswith('torch=='))
    subprocess.run([str(PYTHON), '-m', 'pip', 'install', '-q', f'{torch}+{CUDA}', '--index-url', f'https://download.pytorch.org/whl/{CUDA}'], check=True)
subprocess.run([str(PYTHON), '-m', 'pip', 'install', '-q', '-r', str(requirements)], check=True)
for target, url in MODELS.items():
    if not target.exists():
        target.parent.mkdir(parents=True, exist_ok=True)
        part = target.with_suffix('.part')
        urllib.request.urlretrieve(url, part)
        part.rename(target)
