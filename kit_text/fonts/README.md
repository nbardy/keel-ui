# Bundled font: Inter 4.1

`Inter-Regular.ttf` / `Inter-Bold.ttf` are Inter 4.1 static TTFs, unmodified, used as `kit_text` test fixtures (full
coverage, so the bidi and fallback tests have glyphs to compare against). They are not compiled into a build.

`subset/Inter-{Regular,Bold}.latin.ttf` are what `kit_text::font` bundles: Basic Latin, Latin-1, the Latin Extended-A
letters, General Punctuation, arrows and a few maths symbols. Unhinted, layout features `kern tnum pnum calt ccmp locl
mark mkmk` only. 812 KB -> 98 KB.

Inter is (c) The Inter Project Authors, licensed under the SIL Open Font License 1.1 (`OFL.txt`). Regenerate the subset
(uv, fonttools):

```sh
for w in Regular Bold; do
  uvx --from fonttools pyftsubset Inter-$w.ttf \
    --unicodes='U+0020-007E,U+00A0-00FF,U+0131,U+0152-0153,U+0160-0161,U+0178,U+017D-017E,U+0192,U+02C6,U+02DC,U+2000-206F,U+20AC,U+2122,U+2190-2199,U+21B0-21B1,U+21BB,U+2212,U+221E,U+2264-2265,U+2295,U+25C8,U+2713,U+03C0' \
    --layout-features='kern,tnum,pnum,ccmp,locl,mark,mkmk,calt' --no-hinting --drop-tables+=DSIG \
    --output-file=subset/Inter-$w.latin.ttf
done
```
