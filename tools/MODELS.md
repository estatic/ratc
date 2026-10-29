# Models

`tools/fetch-piper.sh` downloads (idempotently) into the git-ignored `models/piper/`:

- Piper binary `piper_linux_x86_64.tar.gz`, release `rhasspy/piper` `2023.11.14-2` (bundles espeak-ng data and shared libs; extracted to `models/piper/piper/`).
- Three English `medium` voices from `huggingface.co/rhasspy/piper-voices`, chosen to be clearly distinguishable:

| VoiceId | Voice | Character |
|---|---|---|
| Ground | `en_US-ryan-medium` | US male |
| Tower  | `en_GB-alan-medium` | British male, deeper, brisker |
| Atis   | `en_US-amy-medium`  | US female |

Three more English `medium` voices for the controller positions added with the procedures
(`[delivery]`, `[approach]`, `[radar]` in `config/voices.toml`). All three names exist on the
voices repository as written in the plan. Licences are from each voice's `MODEL_CARD`:

| VoiceId | Voice | Dataset licence |
|---|---|---|
| Delivery | `en_US-lessac-medium` | see [Voice licences](#voice-licences) |
| Approach | `en_GB-northern_english_male-medium` | see [Voice licences](#voice-licences) |
| Radar (Warszawa Radar, Kraków Information, Katowice Tower) | `en_US-joe-medium` | see [Voice licences](#voice-licences) |

Approach and the adjacent units speak English only, so they have no `[pl.*]` entry. Polish
Delivery has: `[pl.delivery]` reuses `pl_PL-darkman-medium` (the Ground voice; only three
medium Polish voices exist).

Three Polish `medium` voices from the same repository, used when the exchange is in Polish
(`[pl.*]` in `config/voices.toml`; a missing one falls back to the English voice of that
position). Licences are from each voice's `MODEL_CARD`
(`huggingface.co/rhasspy/piper-voices/blob/main/pl/pl_PL/<name>/medium/MODEL_CARD`):

| VoiceId | Voice | Dataset licence |
|---|---|---|
| Ground | `pl_PL-darkman-medium`   | see [Voice licences](#voice-licences) |
| Tower  | `pl_PL-mc_speech-medium` | see [Voice licences](#voice-licences) |
| Atis   | `pl_PL-gosia-medium`     | see [Voice licences](#voice-licences) |

Each card states: one speaker, 22,050 Hz, "Finetuned from U.S. English lessac voice (medium
quality)".

The mapping and speech rate (`length_scale`) live in `config/voices.toml`.

## Voice licences

Voices are fetched by each user from `huggingface.co/rhasspy/piper-voices` and are never
committed. Checked against each voice's `MODEL_CARD` on 2026-10-28:

| Voice | Used for | Dataset licence | Base model |
|---|---|---|---|
| `en_US-lessac-medium` | Delivery | Blizzard 2013 Lessac research licence (research use only) | trained from scratch |
| `en_US-ryan-medium` | Ground | CC BY-NC-SA 4.0 | lessac |
| `en_GB-alan-medium` | Tower | Mycroft mimic3 `apope`: "Copyright 2022 Mycroft AI, All Rights Reserved" | lessac |
| `en_US-amy-medium` | Atis | Mycroft mimic3, licence not stated on the card | lessac |
| `en_GB-northern_english_male-medium` | Approach | CC BY-SA 4.0 (OpenSLR 83) | lessac |
| `en_US-joe-medium` | Radar | CC0 | lessac |
| `pl_PL-darkman-medium` | Polish Ground, Delivery | CC0 | lessac |
| `pl_PL-mc_speech-medium` | Polish Tower | CC0 | lessac |
| `pl_PL-gosia-medium` | Polish Atis | CC0 | lessac |

Every voice is the `lessac` model or a fine-tune of it, so none is cleared for commercial
use. They are fine for personal, non-commercial use of this project. A commercial build
needs voices trained from scratch on permissively licensed data, or another TTS engine.
