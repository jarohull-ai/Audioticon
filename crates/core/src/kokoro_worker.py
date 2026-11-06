#!/usr/bin/env python3
import sys
import json
import os
import wave
import numpy as np

def main():
    if len(sys.argv) < 3:
        print(json.dumps({"status": "error", "message": "Usage: kokoro_worker.py <model_onnx_path> <voices_bin_path>"}), flush=True)
        sys.exit(1)

    model_path = sys.argv[1]
    voices_path = sys.argv[2]

    if not os.path.exists(model_path):
        print(json.dumps({"status": "error", "message": f"Kokoro ONNX model not found: {model_path}"}), flush=True)
        sys.exit(1)
    if not os.path.exists(voices_path):
        print(json.dumps({"status": "error", "message": f"Kokoro voices.bin not found: {voices_path}"}), flush=True)
        sys.exit(1)

    try:
        from kokoro_onnx import Kokoro
        kokoro = Kokoro(model_path, voices_path)
    except Exception as e:
        print(json.dumps({
            "status": "error",
            "message": f"Failed to initialize kokoro_onnx ({e}). Install via: pip install kokoro-onnx soundfile"
        }), flush=True)
        sys.exit(1)

    print(json.dumps({"status": "ready", "sample_rate": 24000}), flush=True)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
            text = req["text"]
            out_path = req["out_path"]
            voice = req.get("voice", "af_bella")
            speed = float(req.get("speed", 1.0))
            lang = req.get("lang", "en-us")

            samples, sample_rate = kokoro.create(text, voice=voice, speed=speed, lang=lang)
            pcm16 = np.clip(samples * 32767.0, -32768, 32767).astype(np.int16)

            with wave.open(out_path, "wb") as wav_file:
                wav_file.setnchannels(1)
                wav_file.setsampwidth(2)
                wav_file.setframerate(int(sample_rate))
                wav_file.writeframes(pcm16.tobytes())

            print(json.dumps({"status": "ok", "sample_rate": int(sample_rate)}), flush=True)
        except Exception as e:
            print(json.dumps({"status": "error", "message": str(e)}), flush=True)

if __name__ == "__main__":
    main()
