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

Voice models are hosted and distributed via the [rhasspy/piper-voices](https://huggingface.co/rhasspy/piper-voices) repository. Each model carries its own training data attribution:

### Polish: `pl_PL-darkman-medium`
- **Source:** Hugging Face `rhasspy/piper-voices/pl/pl_PL/darkman/medium`
- **Voice Artist / Contributor:** Darkman
- **License:** Open Access / Permissive Community Model. Intended for personal, research, and general audio production. Check dataset provenance before using in strict commercial broadcast syndication.

### English: `en_US-lessac-medium`
- **Source:** Hugging Face `rhasspy/piper-voices/en/en_US/lessac/medium`
- **Dataset:** Lessac Voice Dataset
- **License:** BSD / Open Access.

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
