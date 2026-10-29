# Notices

Copyright 2026 Ivan Grunev. The project code and documentation are licensed under the
[PolyForm Noncommercial License 1.0.0](LICENSE.md), except the material listed below,
which keeps its own licence.

## In this repository

| Path | Source | Licence |
|---|---|---|
| `data/airports/*/apt.dat` | X-Plane Scenery Gateway | GPLv2 (Gateway terms); see each airport's `SOURCE.md` |

## Fetched separately, not in this repository

These are downloaded by the scripts in `tools/` into the git-ignored `models/` directory.
Each user obtains them under their own licence; check it before use.

| What | Script | Licence |
|---|---|---|
| Piper TTS binary (`rhasspy/piper` 2023.11.14-2) | `tools/fetch-piper.sh` | MIT |
| Piper voices | `tools/fetch-piper.sh` | per voice, see [tools/MODELS.md](tools/MODELS.md); **all are derived from the `lessac` voice, whose dataset is licensed for research use only** |
| Whisper ggml models (`ggerganov/whisper.cpp`) | `tools/fetch-whisper.sh` | MIT |

X-Plane navigation data (CIFP, fixes, navaids) is read from the user's own X-Plane
installation and is never copied into this repository.
