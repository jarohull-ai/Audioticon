#!/usr/bin/env python3
"""
Audioticon Psychoacoustic Whisper Diagnostic Inspector & Visual Dashboard
=========================================================================
Compares real human reference recordings against synthesized whisper outputs
across 6 human-ear psychoacoustic failure detectors:
1. Hiss / Szelest Ratio (3.0–9.0 kHz energy vs 250–2500 Hz vowel/diction core in dB)
2. Harmonic-to-Noise / Diction Clarity Ratio (Cepstral Peak Prominence & frame coherence)
3. Metallic / Robotic Comb-Filtering Index (cepstral ripple & pitch-down distortion)
4. Pure-Noise Washout % (% of active frames dominated by unmodulated stationary noise)
5. Warmth-to-Core Ratio (70–250 Hz proximity warmth vs core speech band in dB)
6. Visual Spectrogram & Frequency Response Dashboard (saved to PNG)
"""
import subprocess
import wave
import numpy as np

def load_wav_24k(path, start_sec=0.0, end_sec=None):
    tmp = "/tmp/diag_tmp_24k.wav"
    cmd = ["ffmpeg", "-y", "-i", path, "-ac", "1", "-ar", "24000", tmp]
    subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    with wave.open(tmp, "rb") as wf:
        sr = wf.getframerate()
        raw = wf.readframes(wf.getnframes())
        sig = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
    s0 = int(start_sec * sr)
    s1 = int(end_sec * sr) if end_sec is not None else len(sig)
    return sig[s0:s1], sr

def analyze_psychoacoustics(sig, sr, is_phone_recording=False):
    frame_len = int(0.025 * sr) # 600 samples
    hop = int(0.010 * sr)       # 240 samples
    win = np.hanning(frame_len)
    freqs = np.fft.rfftfreq(frame_len, 1.0 / sr)

    noise_mag = np.zeros_like(freqs)
    if is_phone_recording and len(sig) > int(0.35 * sr):
        n_frames = []
        for s in range(0, int(0.30 * sr) - frame_len, hop):
            n_frames.append(np.abs(np.fft.rfft(sig[s:s+frame_len] * win)))
        if n_frames:
            noise_mag = np.mean(n_frames, axis=0) * 1.35

    rms_list = []
    for s in range(0, len(sig) - frame_len, hop):
        fr = sig[s:s+frame_len]
        rms_list.append(np.sqrt(np.mean(fr**2)))
    rms_arr = np.array(rms_list)
    if len(rms_arr) == 0:
        return {}

    p25 = np.percentile(rms_arr, 25)
    p99 = np.percentile(rms_arr, 99)
    active_thresh = max(p25 * 1.6, 0.003)

    core_energies = []      # 250 - 2500 Hz (human speech intelligibility core)
    hiss_energies = []      # 3200 - 9000 Hz (szelest / syczenie)
    low_energies = []       # 70 - 250 Hz (warmth / fundamental body)
    cpp_values = []         # Cepstral Peak Prominence (voice clarity vs pure noise)
    comb_ripples = []       # Robotic metallic comb-filtering detector
    spec_accum = []

    core_mask = (freqs >= 250) & (freqs < 2500)
    hiss_mask = (freqs >= 3200) & (freqs <= 9000)
    low_mask  = (freqs >= 70) & (freqs < 250)

    for idx, s in enumerate(range(0, len(sig) - frame_len, hop)):
        r = rms_arr[idx]
        if r < active_thresh or r > p99:
            continue
        fr = sig[s:s+frame_len]
        mag = np.maximum(np.abs(np.fft.rfft(fr * win)) - noise_mag, 1e-9)
        mag_norm = mag / (r + 1e-9)
        spec_accum.append(mag_norm)

        e_core = np.sqrt(np.mean(mag[core_mask]**2))
        e_hiss = np.sqrt(np.mean(mag[hiss_mask]**2))
        e_low  = np.sqrt(np.mean(mag[low_mask]**2))
        core_energies.append(e_core)
        hiss_energies.append(e_hiss)
        low_energies.append(e_low)

        log_spec = np.log(mag + 1e-7)
        cepstrum = np.abs(np.fft.irfft(log_spec))
        q_low, q_high = int(0.0025 * sr), int(0.014 * sr)
        cep_region = cepstrum[q_low:q_high]
        cpp = float(np.max(cep_region) - np.mean(cep_region))
        cpp_values.append(cpp)

        q_comb_l, q_comb_h = int(0.0008 * sr), int(0.0022 * sr)
        comb_ripples.append(float(np.std(cepstrum[q_comb_l:q_comb_h])))

    mean_core = np.mean(core_energies) + 1e-12
    mean_hiss = np.mean(hiss_energies) + 1e-12
    mean_low  = np.mean(low_energies) + 1e-12

    hiss_to_core_db = float(20.0 * np.log10(mean_hiss / mean_core))
    warmth_to_core_db = float(20.0 * np.log10(mean_low / mean_core))
    diction_cpp = float(np.mean(cpp_values))
    pure_noise_pct = float(np.mean([1.0 if h > c * 0.85 else 0.0 for c, h in zip(core_energies, hiss_energies)]) * 100.0)
    robotic_comb = float(np.mean(comb_ripples) * 100.0)

    problems = []
    if hiss_to_core_db > -5.0:
        problems.append(f"ZA DUŻO SZELESTU/SYCZENIA (Hiss/Core = {hiss_to_core_db:+.1f} dB, norma <= -5.0 dB)")
    if pure_noise_pct > 25.0:
        problems.append(f"SAM SZUM ZAMIAST DYKCI ({pure_noise_pct:.0f}% ramek szumowych)")
    if diction_cpp < 0.072:
        problems.append(f"NISKA CZYTELNOŚĆ FORMANTÓW (CPP = {diction_cpp:.3f})")

    return {
        "hiss_to_core_db": round(hiss_to_core_db, 2),
        "warmth_to_core_db": round(warmth_to_core_db, 2),
        "diction_cpp": round(diction_cpp, 4),
        "pure_noise_pct": round(pure_noise_pct, 1),
        "robotic_comb": round(robotic_comb, 2),
        "problems": problems,
        "freqs": freqs,
        "mean_spec_db": 20.0 * np.log10(np.mean(spec_accum, axis=0) + 1e-9),
    }

def render_dashboard_png(results, out_png):
    try:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt

        fig, axes = plt.subplots(2, 2, figsize=(15, 10), dpi=130)
        fig.patch.set_facecolor("#0f172a")
        for ax in axes.flat:
            ax.set_facecolor("#1e293b")
            ax.tick_params(colors="#cbd5e1")
            for spine in ax.spines.values():
                spine.set_color("#334155")

        # Plot 1: Spectral Envelopes of the 4 Output Whispers
        ax1 = axes[0, 0]
        colors = {
            "OUT_1_Narracyjny": "#38bdf8",
            "OUT_2_Akcja": "#f97316",
            "OUT_3_Romantyczny": "#f43f5e",
            "OUT_4_Mikrofon": "#a855f7",
        }
        for k, col in colors.items():
            if k in results:
                f = results[k]["freqs"]
                sp = results[k]["mean_spec_db"]
                # Smooth spectrum
                ker = np.ones(7) / 7.0
                sp_smooth = np.convolve(sp, ker, mode="same")
                mask = (f >= 70) & (f <= 8500)
                ax1.plot(f[mask], sp_smooth[mask], label=k.replace("OUT_", ""), color=col, linewidth=2.2)
        ax1.axvspan(3200, 8500, color="#ef4444", alpha=0.12, label="Strefa szelestu/syczenia (>3.2 kHz)")
        ax1.axvspan(250, 2500, color="#22c55e", alpha=0.10, label="Rdzeń dykcji mowy (250-2500 Hz)")
        ax1.set_xscale("log")
        ax1.set_xlim(70, 8500)
        ax1.set_ylim(-5, 36)
        ax1.set_title("Widmo 4 Szeptów Audioticon (Balans Dykcja vs Szelest)", color="#f8fafc", fontsize=12, fontweight="bold")
        ax1.set_xlabel("Częstotliwość (Hz)", color="#cbd5e1")
        ax1.set_ylabel("Względna Energia (dB)", color="#cbd5e1")
        ax1.legend(facecolor="#0f172a", edgecolor="#334155", labelcolor="#f8fafc", fontsize=8)
        ax1.grid(True, alpha=0.2, color="#64748b")

        # Plot 2: Hiss-to-Core Ratio (dB) - lower is cleaner (less rustle/hiss)
        ax2 = axes[0, 1]
        names = ["OUT_1_Narracyjny", "OUT_2_Akcja", "OUT_3_Romantyczny", "OUT_4_Mikrofon"]
        labels = ["1. Narracyjny", "2. Akcja (Żołnierz)", "3. Romantyczny (Ucho)", "4. Mikrofon"]
        vals = [results[n]["hiss_to_core_db"] for n in names]
        bar_cols = ["#22c55e" if v <= -5.0 else "#ef4444" for v in vals]
        bars = ax2.bar(labels, vals, color=bar_cols, width=0.55)
        ax2.axhline(-5.0, color="#facc15", linestyle="--", linewidth=1.5, label="Próg braku szelestu (-5 dB)")
        for b, v in zip(bars, vals):
            ax2.text(b.get_x() + b.get_width()/2, v - 1.5 if v < 0 else v + 0.5, f"{v:+.1f} dB", ha="center", color="#f8fafc", fontweight="bold")
        ax2.set_ylim(-22, 5)
        ax2.set_title("Stosunek Szelestu do Rdzenia Mowy (Hiss/Core dB — mniej = czyściej)", color="#f8fafc", fontsize=11, fontweight="bold")
        ax2.set_ylabel("dB (3.2-9 kHz względem 250-2500 Hz)", color="#cbd5e1")
        ax2.legend(facecolor="#0f172a", edgecolor="#334155", labelcolor="#f8fafc", fontsize=9)
        ax2.grid(True, axis="y", alpha=0.2, color="#64748b")

        # Plot 3: Pure Noise % (% of frames dominated by noise instead of speech formants)
        ax3 = axes[1, 0]
        n_vals = [results[n]["pure_noise_pct"] for n in names]
        n_cols = ["#22c55e" if v <= 25.0 else "#ef4444" for v in n_vals]
        bars3 = ax3.bar(labels, n_vals, color=n_cols, width=0.55)
        ax3.axhline(25.0, color="#facc15", linestyle="--", linewidth=1.5, label="Alarm 'Sam Szum' (25%)")
        for b, v in zip(bars3, n_vals):
            ax3.text(b.get_x() + b.get_width()/2, v + 1.5, f"{v:.1f}%", ha="center", color="#f8fafc", fontweight="bold")
        ax3.set_ylim(0, 75)
        ax3.set_title("Udział Czystego Szumu bez Dykcji (Szum % — mniej = wyraźniejsze słowa)", color="#f8fafc", fontsize=11, fontweight="bold")
        ax3.set_ylabel("% ramek szumowych", color="#cbd5e1")
        ax3.legend(facecolor="#0f172a", edgecolor="#334155", labelcolor="#f8fafc", fontsize=9)
        ax3.grid(True, axis="y", alpha=0.2, color="#64748b")

        # Plot 4: Warmth-to-Core Ratio (dB) (shows acoustic character separation between modes)
        ax4 = axes[1, 1]
        w_vals = [results[n]["warmth_to_core_db"] for n in names]
        w_cols = ["#38bdf8", "#f97316", "#f43f5e", "#a855f7"]
        bars4 = ax4.bar(labels, w_vals, color=w_cols, width=0.55)
        for b, v in zip(bars4, w_vals):
            ax4.text(b.get_x() + b.get_width()/2, v + (0.8 if v >= 0 else -2.0), f"{v:+.1f} dB", ha="center", color="#f8fafc", fontweight="bold")
        ax4.set_ylim(-20, 12)
        ax4.set_title("Ciepło Bliskości (70-250 Hz względem Rdzenia Mowy)", color="#f8fafc", fontsize=11, fontweight="bold")
        ax4.set_ylabel("Warmth / Core (dB)", color="#cbd5e1")
        ax4.grid(True, axis="y", alpha=0.2, color="#64748b")

        plt.tight_layout()
        plt.savefig(out_png, dpi=130)
        print(f"\n📊 Zapisano wizualny raport diagnostyczny: {out_png}")
    except Exception as e:
        print(f"Nie udało się wygenerować wykresu PNG: {e}")

if __name__ == "__main__":
    refs = {
        "REF_Normal": ("/home/jaro/.gemini/antigravity/brain/91e702cb-f5a3-4157-a231-8b5acbf38471/.user_uploaded/uploaded_media_1793956349545.m4a", 0.0, None, True),
        "REF_1_Narracyjny (Jaro)": ("/home/jaro/.gemini/antigravity/brain/91e702cb-f5a3-4157-a231-8b5acbf38471/.user_uploaded/uploaded_media_2_1793954760620.m4a", 0.0, None, True),
        "REF_2_Akcja (Jaro)": ("/home/jaro/.gemini/antigravity/brain/91e702cb-f5a3-4157-a231-8b5acbf38471/.user_uploaded/uploaded_media_1_1793954760620.m4a", 0.0, None, True),
        "REF_3_Romantyczny (Jaro)": ("/home/jaro/.gemini/antigravity/brain/91e702cb-f5a3-4157-a231-8b5acbf38471/.user_uploaded/uploaded_media_0_1793954760620.m4a", 0.0, None, True),
        "OUT_1_Narracyjny": ("/home/jaro/Desktop/audioticon_probka_audytu/1_szept_narracyjny.mp3", 3.0, None, False),
        "OUT_2_Akcja": ("/home/jaro/Desktop/audioticon_probka_audytu/2_szept_akcja.mp3", 3.2, None, False),
        "OUT_3_Romantyczny": ("/home/jaro/Desktop/audioticon_probka_audytu/3_szept_romantyczny_do_ucha.mp3", 3.5, None, False),
        "OUT_4_Mikrofon": ("/home/jaro/Desktop/audioticon_probka_audytu/4_szept_mikrofon.mp3", 3.2, None, False),
    }

    print("=" * 105)
    print(f"{'PRÓBKA':26s} | {'HISS/CORE (dB)':14s} | {'WARMTH/CORE':11s} | {'DYKCJA CPP':10s} | {'SZUM %':7s} | {'DIAGNOZA PROBLEMÓW'}")
    print("=" * 105)
    results = {}
    for name, (path, s0, s1, is_phone) in refs.items():
        sig, sr = load_wav_24k(path, s0, s1)
        res = analyze_psychoacoustics(sig, sr, is_phone)
        results[name] = res
        diag = "; ".join(res["problems"]) if res["problems"] else "✅ CZYSTY, NATURALNY SZEPT"
        print(f"{name:26s} | {res['hiss_to_core_db']:+11.2f} dB | {res['warmth_to_core_db']:+8.2f} dB | {res['diction_cpp']:10.4f} | {res['pure_noise_pct']:5.1f}% | {diag}")

    render_dashboard_png(
        results,
        "/home/jaro/.gemini/antigravity/brain/91e702cb-f5a3-4157-a231-8b5acbf38471/whisper_diagnostics_dashboard.png",
    )
