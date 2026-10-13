# Bardo brand

The mark is four editor tracks crossed by a gold playhead; the palette is "Ouro velho" (old gold with plum, moss green, burgundy and copper); the wordmark is "Bardo" in Geist SemiBold, tracking -0.035em, converted to outlines.

## Files

| File | Use |
| --- | --- |
| `bardo-mark.svg` | Mark for dark grounds (64-unit grid) |
| `bardo-mark-on-light.svg` | Mark for light grounds |
| `bardo-mark-mono.svg` | One color (`currentColor`), for themes that bring their own colors |
| `bardo-mark-small*.svg` | Simplified mark for 24 px and below: three thicker tracks, a wider gap around the playhead |
| `bardo-wordmark.svg` | "Bardo" in outlines (`currentColor`) |
| `bardo-lockup.svg`, `bardo-lockup-on-light.svg` | Mark and wordmark: cap height 44 % of the mark, 18/64 of the mark apart |
| `bardo-icon.svg`, `bardo-icon-small.svg` | App icon: the mark on a dark rounded tile (corner radius 14/64) |
| `png/bardo-icon-{16,20,24,32,48,64,128,256}.png` | The app icon rendered; 16 to 24 use the small mark |
| `bardo.ico` | Windows icon with 16, 20, 24, 32, 48 and 256 px; embedded in `bardo.exe` by `crates/ui/build.rs`, and the icon an installer should use |

The app draws the mark from per-color layers in `crates/ui/assets/mark/`, cut from these files (see `crates/ui/src/brand.rs`).

## Colors

| Role | Dark ground | Light ground |
| --- | --- | --- |
| Playhead (gold) | `#C8962E` | `#94680F` |
| Track 1 (plum) | `#5C4A8A` | `#463670` |
| Track 2 (moss green) | `#3F7A68` | `#2A5E4F` |
| Track 3 (burgundy) | `#A5455F` | `#7A2840` |
| Track 4 (copper) | `#C27A55` | `#A3552D` |
| Ground | `#14110D` | `#F4F0E8` |
| Wordmark | `#EFE9DF` | `#1D1812` |

The small mark drops track 3.

## Rules

- At 24 px and below use the small mark; above that, the full mark.
- Keep clear space of at least one track height (7/64 of the mark) on every side.
- Parts of different colors never touch, so the one-color mark works in any single color: in the app, the high contrast and terminal themes tint it with their accent.
- Geist is licensed under the SIL Open Font License; the outlined wordmark needs no font at runtime.
