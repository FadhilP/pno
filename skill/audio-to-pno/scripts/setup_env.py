"""Create the analysis environment outside the skill folder and print the path of its Python."""
import subprocess, sys, urllib.request, venv
from pathlib import Path

ENV = Path.home() / '.cache' / 'audio-to-pno' / 'venv'
PYTHON = ENV / ('Scripts/python.exe' if sys.platform == 'win32' else 'bin/python')
# piano_transcription_inference fetches this file with `wget`, which Windows does not have
CHECKPOINT = Path.home() / 'piano_transcription_inference_data' / 'note_F1=0.9677_pedal_F1=0.9186.pth'
CHECKPOINT_URL = 'https://zenodo.org/record/4034264/files/CRNN_note_F1%3D0.9677_pedal_F1%3D0.9186.pth?download=1'

if not PYTHON.exists():
    venv.create(ENV, with_pip=True)
requirements = Path(__file__).resolve().parent.parent / 'requirements.txt'
subprocess.run([str(PYTHON), '-m', 'pip', 'install', '-q', '-r', str(requirements)], check=True)
if not CHECKPOINT.exists():
    CHECKPOINT.parent.mkdir(exist_ok=True)
    part = CHECKPOINT.with_suffix('.part')
    urllib.request.urlretrieve(CHECKPOINT_URL, part)
    part.rename(CHECKPOINT)
print(PYTHON)
