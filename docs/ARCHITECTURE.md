# MicStudio Architecture & Technical Reference

This document details the audio routing architecture, PipeWire filter-chain integration, DSP algorithms, and operational invariants of **MicStudio**.

---

## 1. Core Architectural Contract: Single Persistent Virtual Node

In Linux gaming and communication stacks, switching audio devices (e.g. creating temporary virtual microphones for separate effects) causes client dropouts, stream renegotiation failures, or game client crashes (e.g., in Counter-Strike 2 or Discord).

MicStudio solves this with a **Single Persistent Node Contract**:
1. **Target Node**: `ghelper-audio` (Clean Broadcast & Studio Output).
2. **Persistence**: The node is registered once with PipeWire upon service startup.
3. **Application Routing**: All games, voice chats, and recording tools select `ghelper-audio` as their recording input.
4. **Internal FX Swapping**: Switching voice presets (from `Natural Clean` to `Deep Voice`, `Robot`, or `Darth Vader`) occurs strictly inside the DSP filter graph without changing the node identity or re-linking client capture streams.

---

## 2. DSP Signal Processing Stages

The audio pipeline consists of 7 modular processing stages:

```
[ ALC256 ADC Hardware Capture ]
              │
              ▼
   [ Stage 1: Rumble High-Pass Filter (80 Hz) ]
              │
              ▼
   [ Stage 2: DPDFNet Neural AI Denoiser ]
              │
              ▼
   [ Stage 3: LSP Parametric Equalizer ]
              │
              ▼
   [ Stage 4: LADSPA Creative FX Engine (Pitch/Sat/RingMod/LoFi) ]
              │
              ▼
   [ Stage 5: Speech Leveler Compressor (Quiet Voice Boost) ]
              │
              ▼
   [ Stage 6: Anti-Shout Brickwall Limiter (-2.0 dBFS Ceiling) ]
              │
              ▼
   [ Stage 7: ghelper-audio Output Stream ]
```

### Stage 1: Rumble High-Pass Filter
* **Cutoff**: 80 Hz, 12 dB/octave slope.
* **Purpose**: Removes mechanical laptop desk vibrations, chassis tapping, and low-frequency HVAC rumble before the signal reaches the neural denoiser.

### Stage 2: DPDFNet Neural Denoiser
* **Architecture**: Deep recurrent neural network denoiser.
* **Attenuation**: 12 dB default (capped at 40 dB).
* **Behavior**: Always active across both clean and creative presets. Strips ambient fan hum, keyboard clicks, and background noise while preserving formant harmonics.

### Stage 3: LSP Parametric EQ
* **Filter Type**: 16-band IIR biquad equalizer.
* **Tuning**: Tailored vocal presence curves configured per-preset (e.g., +3.0 dB presence peak at 2.8 kHz for `Studio Broadcast`, low-end shelf for `Warm Radio`).

### Stage 4: LADSPA Creative FX Engine
* **Pitch Shifting**: Hermite-interpolated fractional delay pitch-shifting with formant-preserving phase correction.
* **Tube Saturation**: Soft-clipping hyperbolic tangent overdrive for warm tube harmonics.
* **Ring Modulation**: Carrier oscillator modulation (sine or square chopper) for robotic/alien timbres.
* **Decimator**: Bit-depth reduction (e.g., 8-bit) and sample rate decimation for vintage lo-fi effects.
* **Delay / Echo**: Single or multi-tap ping-pong echo with dry passthrough isolation.

### Stage 5: Speech Leveler Compressor (Quiet Voice Boost)
* **Default Boost**: `+4.0 dB` on low-level vocal input.
* **Ratio**: 2.5:1 with smooth soft-knee curve.
* **Behavior**: Transparently amplifies quiet whispers, low vocal registers, and subtle callouts without elevating the baseline noise floor.

### Stage 6: Anti-Shout Limiter Guard
* **Ceiling**: `-2.0 dBFS`.
* **Lookahead**: 2.5 ms lookahead buffer.
* **Release**: 50 ms adaptive release.
* **Behavior**: Prevents sudden loud shouts or enthusiastic speech from clipping the audio stream or causing harsh DAC overload.

---

## 3. Hardware Clipping vs. Digital Limiting

MicStudio actively monitors sample peak dynamics to distinguish between analog pre-amp distortion and digital safety limiting:

1. **Hardware ADC Clipping Suspected**:
   - Detected when 3 or more consecutive audio samples hit full-scale integer boundary (`1.000` or `-1.000`).
   - Trigger message: `[Input clipping suspected — check microphone gain]`.
   - Action required: Lower hardware input gain in ALSA or `alsamixer`.

2. **Digital Limiting Active**:
   - Detected when audio peak reaches between `-2.0 dBFS` and `-0.01 dBFS` where the anti-shout limiter is actively attenuating the signal.
   - Trigger message: `[LIMITING]`.
   - Action: Working as intended to protect listener ears.

---

## 4. TUI Visualizations & Dual-Graph Architecture

The interactive terminal interface operates at up to 60 FPS using Ratatui:

* **Dual-Graph Mode (`m` / `M` / `s`)**:
  - **Top (58%)**: High-resolution Real-Time Spectrum Analyzer (RTA) splitting the 50 Hz – 16 kHz band across available terminal columns using UTF-8 block elements (` ▂▃▄▅▆▇█`). Peak hold decay markers track transient energy.
  - **Bottom (42%)**: Spectrogram Waterfall Heatmap displaying energy frequency distribution over time.

* **Modal DSP Details (`Space`)**:
  - Opens a floating dialog displaying current preset parameters, wet/dry mix, denoiser attenuation, and DSP stage routing.

* **Factory Baseline Reset System**:
  - Single preset reset (`r`): Restores the currently highlighted preset to factory settings.
  - Universal reset (`R` / `Shift+R` / `micstudio reset all`): Restores all 27 voice models and Natural Clean to their factory default configuration.

---

## 5. Performance & Resource Benchmarks

| Metric | Measured Baseline |
| :--- | :--- |
| **Idle CPU Utilization** | `< 0.3%` |
| **Active DSP CPU (DPDFNet + FX)** | `1.1% – 1.4%` (Intel i5-13450HX) |
| **Resident Memory (RSS)** | `~28 MB` |
| **Buffer Latency** | `10.6 ms` (512 samples @ 48 kHz) |
| **Preset Switching Latency** | `< 2 ms` (zero audio dropouts) |
