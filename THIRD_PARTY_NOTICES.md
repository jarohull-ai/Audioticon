# Third-Party Notices and Licenses

This project incorporates third-party software components, neural voice models, and external services. Below are the details regarding licenses, copyrights, and terms of use.

---

## 1. Piper Neural Text-to-Speech

- **Project:** [Piper](https://github.com/rhasspy/piper)
- **Author:** Michael Hansen (Rhasspy / Nabu Casa)
- **License:** [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0)
- **Description:** Fast, local neural text-to-speech engine running via ONNX Runtime.

---

## 2. Voice Models (Piper Voices)

Voice models are hosted and distributed via the [rhasspy/piper-voices](https://huggingface.co/rhasspy/piper-voices) repository (catalog version 1.0.0, containing 142 open neural voice models across 42 languages). Each voice model in the catalog carries its own dataset provenance and license:

### Polish Models (`pl_PL`):
- **`pl_PL-darkman-medium`**
  - **Dataset:** NabuCasa / voice-datasets
  - **License:** CC0 (Public Domain) — safe for commercial audiobooks and open distribution.
- **`pl_PL-gosia-medium`**
  - **Dataset:** NabuCasa / voice-datasets
  - **License:** CC0 (Public Domain) — safe for commercial audiobooks and open distribution.
- **`pl_PL-mc_speech-medium`**
  - **Dataset:** MC Speech Corpus
  - **License:** CC0 (Public Domain) — safe for commercial audiobooks and open distribution.
- **`pl_PL-mls_6892-low`**
  - **Dataset:** Multilingual LibriSpeech (MLS)
  - **License:** Creative Commons Attribution 4.0 International (CC-BY-4.0).

### English & Multilingual Models:
- **`en_US-amy-medium`** — CC0 (Public Domain).
- **`en_US-lessac-medium`** — Non-commercial educational/research license (Blizzard Challenge 2013).
- **`en_US-ryan-medium`** / **`en_US-ryan-high`** — CC0 (Public Domain).
- **`de_DE-thorsten-medium`** — CC0 (Public Domain).

Before distributing or selling commercial audiobook productions, verify the specific license tag displayed in the Audioticon model manager (`audioticon models list` or Web Studio `Głosy & Silnik`). Models marked with `CC0` or `MIT` allow unrestricted commercial distribution.

---

## 3. Microsoft Edge TTS Service

- **Python Wrapper:** [edge-tts](https://github.com/rany2/edge-tts) (LGPL-3.0 / MIT)
- **Service:** Microsoft Edge Speech Synthesis Service
- **Notice & Disclaimer:** 
  Edge-TTS connects to Microsoft Edge's public Read Aloud endpoints. This bridge is provided for development, experimentation, and personal workflows. For guaranteed SLAs, commercial scale, or legal enterprise distribution, users should either rely on 100% offline local Piper models or configure official cloud APIs (such as Azure Speech Services or ElevenLabs). Audioticon is not affiliated with or endorsed by Microsoft Corporation.

---

## 4. Rust Crates & External Libraries

Audioticon is built with Rust and utilizes open-source libraries from crates.io:

- **clap** — Apache-2.0 OR MIT
- **tokio** — MIT
- **serde**, **serde_json**, **toml** — Apache-2.0 OR MIT
- **hound** — Apache-2.0
- **sha2**, **hex** — Apache-2.0 OR MIT
- **regex** — Apache-2.0 OR MIT
- **dirs** — Apache-2.0 OR MIT
- **anyhow**, **thiserror** — Apache-2.0 OR MIT
