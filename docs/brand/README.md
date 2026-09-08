# InterFire brand assets (`docs/brand`)

Public named copies and **resize-only** size declensions.

## Source of truth

Masters live under local `assets/` / `assets/slices/` (ChatGPT export filenames).
That directory is **local-only** (git exclude) and must stay on disk as the crop
master set. **Do not delete or mutate `assets/`.**

This folder holds:

1. Byte-identical renamed masters (`logo-banner.png`, …)
2. Width/size variants (`-readme`, `-desktop`, `-mobile`, `-256`, `-128`, `-64`, favicons)

Processing rule: **byte-identical copy** of local slices for masters, then
**LANCZOS scale** (up or down) for `-readme` / `-desktop` / `-mobile` /
`-256` / `-128` / `-64` / favicons so the artwork fills the target size.
Do not mutate `assets/`. Do not pad a small master into a larger empty canvas.

## Export timestamp → name map

| `assets/slices/` time suffix | Master name                   |
| ---------------------------- | ----------------------------- |
| `02_35_37`                   | `logo-lockup-wide`            |
| `02_35_38`                   | `logo-banner`                 |
| `02_35_39`                   | `seal-shield`                 |
| `02_35_40`                   | `mark-phoenix`                |
| `02_35_41`                   | `logo-horizontal`             |
| `02_35_42`                   | `mark-phoenix-head`           |
| `02_35_43`                   | `mark-phoenix-mono`           |
| `02_35_44`                   | `icon-app-phoenix`            |
| `02_35_45`                   | `icon-app-phoenix-gradient`   |
| `02_35_46`                   | `icon-app-phoenix-light`      |
| `02_35_50`                   | `seal-gh-dark`                |
| `02_35_51`                   | `seal-gh-light`               |

## Suggested defaults

- README header: `logo-banner-readme.png` + `icon-app-phoenix-256.png`
- Compact lockup (before Thanks): `logo-horizontal-readme.png`
- Footer seal (theme-aware): white disk on GitHub dark, dark disk on GitHub light
- Desktop header: `logo-banner-desktop.png`
- Mobile header: `logo-banner-mobile.png` or `icon-app-phoenix-128.png`
- Favicon: `favicon-32.png` (from `icon-app-phoenix`)

Tagline: **FIREWALL · SECURE · CONTROL** (Linux-first Rust application firewall).

## GitHub README seals

Circular seals (phoenix + INTERFIRE / FIREWALL ring). Pick the disk that
**contrasts** with the page: white disk on dark GitHub, dark disk on light
GitHub.

| File | Use |
| --- | --- |
| `seal-gh-light.png` | White / light disk - GitHub **dark** (`#gh-dark-mode-only`) |
| `seal-gh-dark.png` | Dark disk - GitHub **light** (`#gh-light-mode-only`) |
| `seal-gh-light-128.png` / `seal-gh-dark-128.png` | Compact footer seals |

Do not put the dark disk on GitHub dark mode: the round vanishes into the page.
