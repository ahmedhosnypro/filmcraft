# filmcraft-tts

Text to speech for FilmCraft's Text to Speech panel (narrations, L2). Engine commands (`tts.*`) and
clip behaviour: `crates/engine/src/narration.rs`.

- `Voice` trait (script in, mono 24 kHz `f32` out), the voice catalogue (`voices`, `voice`), and
  the limits (64 KB script, 30-minute narration, pace 0.5–2×, pitch ±12 semitones).
- `script`: pause markers `[pause 1s]` / `[pause 500ms]` (case-insensitive, capped at 10 s;
  malformed markers are read as text).
- `formant`: the built-in voices **Basic Female** and **Basic Male**, an original source–filter
  formant synthesizer: spelling rules → phone targets → glottal pulse train through three two-pole
  resonators, plus filtered noise for fricatives and bursts. No download, works in the web build.

Everything is deterministic. No dependencies beyond `serde` and `thiserror`.

## Status and limitations (M7.9)

- The built-in voices are **robotic**, and their spelling rules are crude: English words with
  irregular spelling are mispronounced, and numbers are read digit by digit. They are the fallback
  and test voice. Natural voices (Kokoro-82M, Apache-2.0, downloaded on first use) come in M7.12,
  with the clean-room text normalizer (`crates/tts-text`, M7.10) and English pronunciation from
  CMUdict (M7.11).
- English (United States) only.
- Vocal pitch shifts the voice's base pitch (and its formants slightly); pace scales every duration.

## Tests

`cargo test -p filmcraft-tts`: determinism, valid samples (finite, peak ≤ 0.8, audible RMS), pause
markers produce exactly the requested number of zero samples, pace 2× / 0.5× gives 0.5× / 2× the
length (±5 % / ±10 %), measured pitch (autocorrelation) of the female voice is ≥ 1.5× the male, and
+6 semitones raises it by √2 (±0.12); empty, emoji-only, non-Latin, pause-only and over-long scripts
are refused before rendering; hostile scripts never panic.

## References

- G. E. Peterson and H. L. Barney, "Control Methods Used in a Study of the Vowels", JASA 24 (1952):
  the published average formant frequencies used for the vowels.
- D. H. Klatt, "Software for a cascade/parallel formant synthesizer", JASA 67 (1980): the standard
  two-pole digital resonator form. No code was read or copied; everything else is original.
