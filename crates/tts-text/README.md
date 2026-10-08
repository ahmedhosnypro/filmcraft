# filmcraft-tts-text

Text-to-speech text front end for FilmCraft (L2, phase P2a). Takes a narration
script and returns plain spoken words, structured as tokens: `Word`,
`Pause` (from `[pause 1s]` markers), `SentenceEnd` and `Punct`. The
grapheme-to-phoneme stage (P2b) consumes the words.

```rust
use filmcraft_tts_text::{normalize, Lang, Token};

let tokens = normalize("Call 555-0134 by 3:30 p.m. on Oct. 7th, it's $3.50 (20% off)", Lang::EnUs)?;
// → Call | five five five zero one three four | by | three thirty p m | on |
//   October seventh | , | it's | three dollars and fifty cents | ( | twenty percent off | )
```

English only (`Lang::EnUs`, `Lang::EnGb`), per the amended brief. Handles:
cardinals to 999 999 999 999, negatives, decimals, ordinals, money (`$`, `€`,
`£`), percent, years (1100–2099), clock times, numeric and abbreviated dates,
phone-like digit groups, common abbreviations (Mr., Mrs., Dr., St., Mt., vs.,
etc., e.g., i.e., a.m., p.m.), spelled acronyms, `&`/`+`/`@`/`#`/`/`, and pause
markers. Never crashes: >1 MiB input is `NormalizeError::TooLong`, oversized or
leading-zero numbers are read digit by digit, malformed pause markers become
plain text, and all slicing is boundary-safe with `get()`.

## Rules where behaviour is a judgement call

- **Cardinals have no "and"**: "1,234" → "one thousand two hundred thirty-four".
  British "one thousand two hundred **and** thirty-four" is not implemented.
- **Decimals read digit by digit** after "point" ("3.14" → "three point one four").
- **Years**: a standalone 4-digit number in 1100–2099: 1100–1999 →
  century + remainder ("nineteen ninety-nine", "nineteen oh five",
  "nineteen hundred"); 2000–2009 → "two thousand five"; 2010–2099 →
  "twenty twenty-six". 2100+ reads as an ordinary cardinal.
- **St.** is "saint" before a capitalised name, "street" otherwise; **Dr.** is
  always "doctor" (drive/dr. disambiguation is out of scope, as the brief allows).
- **Acronyms**: unpronounceable all-caps words of 2–5 letters are spelled letter
  by letter ("TDA" → T D A, "FBI" → F B I): a leading consonant cluster of 2+ or
  an interior run of 3+ consonants makes an acronym unpronounceable, as does
  having no vowels at all. Pronounceable ones (NASA, UNESCO and a small list)
  stay words. Single letters are Word tokens that G2P should read as letter names.
- **a.m./p.m.** become the letter words "a m"/"p m". Lowercase "pm" without dots
  is spelled; lowercase "am" is left alone (it's the English verb).
- **Fractions**: 1/2 → "one half"; other denominators ≤ 12 → "N ordinal(s)"
  ("three quarters"); anything else → "N over M". Dates need a 4-digit year —
  "10/7" alone reads as a fraction.
- **Digit groups**: hyphen-joined digits read digit by digit when any group has
  a leading zero or there are 7+ digits in total ("555-0134"); otherwise two
  small groups read "A to B" ("2-3" → "two to three").
- **Pause markers**: `[pause <n>s|ms]`, capped at 10 s; malformed markers
  ([pause], [pause x], unclosed) stay plain text — never an error.
- **Money**: "$3.50" → "three dollars and fifty cents" (€ → euros/cents, £ →
  pounds/pence); a lone `$`/`€`/`£` with no number stays a punctuation token.
- **Sentence ends**: runs of `.!?` are one `SentenceEnd`; abbreviation and
  number dots are consumed by their rules and never end a sentence.

## Limitations

- English only (amended brief). Spanish was cut from this crate's scope.
- No "and" in British cardinals; no British-ism beyond date order and £.
- Pronunciation is not attempted here — this crate stops at spoken words.
- Time hours 0–12 read as cardinals ("0:15" → "zero fifteen"), not "twelve".
- Ordinal suffixes are validated but "1st"/"1 th"-style spacing is not accepted.
- "may" is never treated as a month abbreviation (it's a word); other dotted
  month abbreviations (Jan.–Dec.) are.

## References

None (rules written from scratch). Clean-room: no TTS/normalizer source was read.
