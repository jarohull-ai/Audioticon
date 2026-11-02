#!/usr/bin/env python3
import sys
import json
import wave
from piper.voice import PiperVoice
from piper.config import SynthesisConfig

def main():
    if len(sys.argv) < 2:
        print(json.dumps({"status": "error", "message": "Usage: piper_worker.py <model_path>"}), flush=True)
        sys.exit(1)

    model_path = sys.argv[1]
    try:
        voice = PiperVoice.load(model_path)
    except Exception as e:
        print(json.dumps({"status": "error", "message": f"Failed to load model: {e}"}), flush=True)
        sys.exit(1)

    # Signal ready
    print(json.dumps({"status": "ready", "sample_rate": voice.config.sample_rate}), flush=True)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
            text = req["text"]
            out_path = req["out_path"]
            length_scale = float(req.get("length_scale", 1.0))
            volume = float(req.get("volume", 1.0))

            syn_cfg = SynthesisConfig(length_scale=length_scale, volume=volume)

            # Synthesize
            with wave.open(out_path, "wb") as wav_file:
                wav_file.setnchannels(1)
                wav_file.setsampwidth(2)
                wav_file.setframerate(voice.config.sample_rate)
                for chunk in voice.synthesize(text, syn_config=syn_cfg):
                    wav_file.writeframes(chunk.audio_int16_bytes)

            print(json.dumps({"status": "ok"}), flush=True)
        except Exception as e:
            print(json.dumps({"status": "error", "message": str(e)}), flush=True)

if __name__ == "__main__":
    main()
