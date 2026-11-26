# Blaze UI Unslop — Design Spec (2026-11-26)

Applies the `unslop` method (Damir Sokolovsky, @heydamir, CC BY 4.0 — keep this credit) to UI and the
`no-ai-slop` rules (Peter Yang) to interface copy, across every BlazeOS app except FireHub, Blaze House
(`blaze-house`, `blazeos-control`) and the OOBE (`blaze-setup`).

Reference material (read before editing):
- `unslop` skill: `<refs>/unslop/skills/unslop/SKILL.md`, `references/signs.md`, `references/tokens.md`
- `no-ai-slop` skill: `<refs>/no-ai-slop/skills/no-ai-slop/SKILL.md`
  (`<refs>` = local visual reference scratch directory)

## Direction (approved by the user)
- **Product:** Blaze SolarEvolution's own system tools — security center, desktop settings, first-run guide,
  launcher, game hub, recovery — for a Turkish power user and gamer on Fedora 45.
- **Character:** steady, exact, a little warm.
- **World:** the ember in the Blaze logo (red and yellow) on a night desktop. Surfaces stay graphite grey;
  the ember lives only in the accent.
- **Moments:** Akrep — one overall security status block; Settings — wallpaper/monitor preview;
  Welcome — the keycap shortcut sheet; GameZone — the game art itself.
- **Signature:** real keycaps for shortcuts, and a small status dot beside each security area or service state.

## Tokens (GTK4 CSS)
Neutral ramp — graphite, dark:
| Role | Value |
|---|---|
| Deepest (sidebar, rail) | `#0f0f10` |
| Page / window | `#141415` |
| Card | `#19191b` |
| Raised (active row, input, hover) | `#232325` |
| Hairline | `rgba(237,237,237,0.08)` (stronger: `0.12`) |
| Text primary | `#ededed` |
| Text secondary | `#a1a1a6` |
| Text muted / icons | `#85858b` |

Accent — Blaze ember: `#c83b22` (fill, white text OK only at >=14px/500 — prefer as indicator),
ember for text/indicator on dark: `#e0573c`. Used only for: current selection indicator, the one key status,
focus ring tint, checked state of checkbox/switch, progress fill. Never on large surfaces.

Primary button: off-white `#ededed` fill, `#141415` text. Secondary: `#232325` + hairline, primary text.
Destructive: `#e5484d` text on `#232325` + `rgba(229,72,77,0.35)` hairline; confirm dialogs may use solid `#c62f34` + white.

Semantic (toned): positive `#4cc38a`, warning `#d9a03f`, negative `#e5484d`. Show state as a 8px dot + plain text label,
not as filled colored pills. `Unavailable` = hollow muted dot + muted text. `Unknown` = muted dot with dashed ring
+ muted text. Never purple.

Spacing: 4px grid only (4, 8, 12, 16, 20, 24, 32, 40, 48). Nav rows 36 tall, gap 2–4. Card padding 16–20. Page padding 24.
Section gap 24–32. Same things share one size.

Radius: keycap/tag 4–6, row/input/button 8, card 12, window/panel 16. Nested radius = outer − padding (min 4).
No `999px` except status dots, switches and avatar circles.

Type: system UI font (do not force a new family). Sizes: 12 meta, 13 secondary, 14 body/nav, 16 card title,
20 page title, 24 hero title (welcome/overview only). Weights 400 and 500; 600 only for the page title.
No 700–900. Letter-spacing 0 below 16px. No uppercase tracked labels. Numbers tabular
(`font-feature-settings: "tnum"`).

Icons: GTK symbolic icon names only (`*-symbolic`), 16px, color inherits the text role (secondary/muted).
No multicolor PNG emblems in nav rows or card titles. No emoji anywhere in UI.

Depth: surface steps + hairlines. No gradients (no `linear-gradient` on surfaces, buttons, progress bars),
no glows, no colored shadows. Floating popovers/menus may keep one soft neutral shadow `0 8px 24px rgba(0,0,0,0.35)`.

Motion: `transition` lists only `background-color, border-color, color, opacity` (and `transform` for press),
`150ms cubic-bezier(0.22, 1, 0.36, 1)`. No `transition: all`. No hover scale. Press may scale to `0.98`.
No looping/pulsing animations on non-live elements.

Focus: visible keyboard focus everywhere: `outline: 2px solid rgba(224,87,60,0.65); outline-offset: 2px;` on
`:focus-visible`. Never globally disable outlines.

## Copy rules (Turkish UI)
- Sentence case for titles, labels, buttons (`Güvenlik duvarı`, not `Güvenlik Duvarı`). Proper nouns keep caps
  (Akrep, SolarUI, GameZone, Omnibar, FireHub, Sentinel, SELinux, Flatpak, ClamAV, Niri, Noctalia, We10X).
- No greeting headlines (`… Hoş Geldiniz`). Title names the context.
- No exclamation marks. No trailing period on titles, labels, buttons, captions.
- No hype/fake-profound words: Luminous, Elastik/Yay fiziği, Liquid, Aqua, Fluid, Hibrit ekosistem, Zırhlı,
  Living Flow, ColorOS, GPU hızlandırmalı (unless a real setting), devrim, sihir, kusursuz, yeni nesil.
- Internal/implementation terms leave titles and buttons: `(Polkit)`, `RO`, `D-Bus`, `CIDR`, `rich rule`,
  `transaction`, `idle-exit`, `5 durum modeli`, `Değişmez garanti`. If the user needs to know, put a plain
  muted caption under the control (e.g. `Yönetici parolası istenir`, or a lock icon `changes-prevent-symbolic`).
- Titles short (<= 4 words where possible). One-line description max under a section title, plain and specific.
- Buttons: verb first, short (`Uygula`, `Önizle`, `Tara`, `Karantinaya al`, `Kur`). One primary per view.
- Em dash only when clearly better; never decorative `—` in labels.
- Do not invent features, numbers or claims. Keep every feature, page and order.

## Do not
- Do not change behavior, D-Bus calls, security logic, config keys, file paths or keyboard shortcuts.
- Do not remove pages, options or actions. Do not rename products.
- Do not add new decoration (glass, grain, neon, bento) to replace removed decoration.
- Before changing any user-visible string, `rg` the string in `tests/` and update tests in lockstep.
