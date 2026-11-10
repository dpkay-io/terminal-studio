# Bundled fonts

| File | Family | Version | Source |
| --- | --- | --- | --- |
| `Inter-Regular.ttf` | Inter | 4.1 (`Version 4.001`) | [rsms/inter v4.1](https://github.com/rsms/inter/releases/tag/v4.1) `extras/ttf` |
| `Inter-SemiBold.ttf` | Inter SemiBold | 4.1 (`Version 4.001`) | same archive |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono | 2.304 | [JetBrainsMono v2.304](https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304) `fonts/ttf` |
| `JetBrainsMono-Bold.ttf` | JetBrains Mono Bold | 2.304 | same archive |
| `lucide-subset.ttf` | Lucide Icons | 0.468.0 | [lucide-font 0.468.0](https://github.com/lucide-icons/lucide/releases/tag/0.468.0) |

Licenses: `LICENSE-Inter.txt` and `LICENSE-JetBrainsMono.txt` use the SIL Open
Font License 1.1. `LICENSE-Lucide.txt` uses the ISC License.

JetBrains Mono is **not** subset (Braille, arrows, geometric shapes, Greek, and Cyrillic stay intact). Inter is subset to Basic Latin, Latin-1 Supplement, and General Punctuation so the four files stay under about 1 MB.

```bash
pyftsubset Inter-Regular.ttf \
  --unicodes="U+0000-00FF,U+2000-206F" \
  --layout-features="*" \
  --glyph-names --symbol-cmap --legacy-cmap \
  --notdef-glyph --notdef-outline --recommended-glyphs \
  --name-IDs="*" --name-legacy --name-languages="*" \
  --output-file=Inter-Regular.ttf

pyftsubset Inter-SemiBold.ttf \
  --unicodes="U+0000-00FF,U+2000-206F" \
  --layout-features="*" \
  --glyph-names --symbol-cmap --legacy-cmap \
  --notdef-glyph --notdef-outline --recommended-glyphs \
  --name-IDs="*" --name-legacy --name-languages="*" \
  --output-file=Inter-SemiBold.ttf
```

The Inter commands require `fonttools` (`python3 -m pip install fonttools`) and
are run from `assets/fonts/` against fresh files from the release archive.

Lucide is regenerated from the icon names in `src/ui_kit/icons.rs`. The script
downloads the pinned release, resolves names through `info.json`, and writes
only the referenced glyphs:

```bash
python3 -m pip install fonttools
python3 scripts/subset-lucide.py
```
