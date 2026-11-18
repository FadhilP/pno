# pno documentation

`pno` is a piano score format in plain text, and a command line tool for AI agents to read, check, edit and export scores in it. The repo also has two Claude Code skills: one to edit scores, and one to transcribe a recording into a score.

| Document | Content |
|---|---|
| [`../README.md`](../README.md) | For users: the format in brief, playback, install, commands, limits |
| [`../SPEC.md`](../SPEC.md) | The format reference (`pno spec` prints it) |
| [architecture.md](architecture.md) | The modules, the model, `Analysis`, and how each command runs |
| [playback-and-export.md](playback-and-export.md) | Export dispatch, the performance, MIDI, audio, video, MusicXML, MuseScore |
| [audio-to-pno.md](audio-to-pno.md) | The transcription skill, `a2p.py`, `pno a2p`, the models |
| [development.md](development.md) | Repository layout, build, external programs, skills, tests |
| [`../skill/pno/SKILL.md`](../skill/pno/SKILL.md) | The instructions of the `pno` skill for an agent |
| [`../skill/audio-to-pno/SKILL.md`](../skill/audio-to-pno/SKILL.md) | The instructions of the `audio-to-pno` skill for an agent |
