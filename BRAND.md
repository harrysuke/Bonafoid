# Bonafoid — Brand Guidelines

## Brand Assets

## Brand Assets

Assets sourced from workspace: `D:\laragon\www\playground\Nocode\favicon_io`

- `android-chrome-192x192.png` (61,479 bytes) — brand logo (square, high-res)
- `android-chrome-512x512.png` (361,520 bytes) — brand logo (square, max-res)
- `apple-touch-icon.png` (55,097 bytes) — submark / touch icon
- `favicon-16x16.png` (928 bytes) — browser tab favicon (small)
- `favicon-32x32.png` (2,838 bytes) — browser tab favicon (medium)
- `favicon.ico` (15,406 bytes) — multi-size .ico bundle (16/32/48)
- `site.webmanifest` (263 bytes) — PWA manifest reference
- Both sourced from `D:\laragon\www\playground\Nocode` workspace

---

## Color Palette

| Role | Token | Hex (Light) | Hex (Dark) | Usage |
|---|---|---|---|---|
| Background Primary | `--color-bg-primary` | `#f9fafb` (`gray-50`) | `#0f172a` (`slate-900`) | Page background |
| Background Secondary | `--color-bg-secondary` | `#ffffff` (`white`) | `#1e293b` (`slate-800`) | Cards, panels |
| Background Tertiary | `--color-bg-tertiary` | `#f3f4f6` (`gray-100`) | `#334155` (`slate-700`) | Sidebar, hover states |
| Primary 50 | `#f0f9ff` | `primary-50` | Light tint backgrounds |
| Primary 100 | `#e0f2fe` | `primary-100` | Subtle highlights |
| Primary 600 | `#0284c7` | `primary-600` | Brand color, primary buttons, links |
| Primary 700 | `#0369a1` | `primary-700` | Active/focus states |
| Gray 400 | `#9ca3af` | `gray-400` | Dividers, disabled text |
| Red 600 | `#dc2626` | `red-600` | Danger actions |
| Green 600 | `#16a34a` | `green-600` | Success states |

---

## Typography

### Font Family
- System sans-serif stack (Tailwind default): Inter / system-ui / -apple-system / sans-serif
- Feature settings: `font-feature-settings: "rlig" 1, "calt" 1;` for ligatures and contextual alternates

### Scale (Tailwind utilities)
| Size | Weight | Usage |
|---|---|---|
| `text-xs` (`12px`) | `font-medium` (`500`) | Labels, badges, captions |
| `text-sm` (`14px`) | `font-medium` (`500`) | Body, form labels |
| `text-base` (`16px`) | `font-normal` (`400`) | Default text |
| `text-lg` (`18px`) | `font-semibold` (`600`) | Section headings |
| `text-xl` (`20px`) | `font-bold` (`700`) | Page titles (`Layout` header), login heading |

### Color
- Default: `text-gray-900` / `dark:text-gray-100`
- Secondary: `text-gray-500` / `dark:text-gray-400`
- Brand links: `text-primary-600` / `dark:text-primary-400`
- Danger: `text-red-600` / `dark:text-red-400`

---

## Iconography

- **Library**: `lucide-react` (`lucide-react` package, v0.294.0)
- Usage: all navigation icons (`Home`, `FileText`, `GitBranch`, `Database`, `Table2`, `Palette`, `Puzzle`, `Monitor`, `Code`, `Settings`), button icons (`RefreshCw`, `Upload`, `Trash2`, `Shield`, `ChevronDown`, `ChevronLeft`, `ChevronRight`, `Menu`), and UI state (`Moon`, `Sun`, `LogOut`, `Save`, `FolderOpen`, `FilePlus`)
- Size: `w-5 h-5` (default nav/icon), `w-6 h-6` (brand/code), `w-4 h-4` (small actions), `w-8 h-8` (user avatar)
- Color: `text-primary-600` (brand), `text-gray-700` / `dark:text-gray-300` (default icon)

---

## UI Icons (Component System)

### Buttons (`packages/ide-core/src/components/ui/Button`)
- `.btn`: base — inline-flex, rounded-lg, font-medium, transition, disabled opacity
- `.btn-primary`: `bg-primary-600` / `hover:bg-primary-700` / `focus:ring-primary-500`
- `.btn-secondary`: `bg-gray-100` / `hover:bg-gray-200` (dark: `bg-gray-700`)
- `.btn-danger`: `bg-red-600` / `hover:bg-red-700`
- Size variants: `.btn-sm` (`text-sm`), `.btn-lg` (`text-lg`)

### Cards (`packages/ide-core/src/components/ui/Card`)
- `.card`: `bg-white` / `dark:bg-slate-800`, `rounded-xl`, shadow-sm, border
- Header (`.card-header`): title + optional actions
- Content (`.card-content`): body with `p-4` / `p-6`
- Footer (`.card-footer`): actions / status

### Inputs (`packages/ide-core/src/components/ui/Input`)
- `.input`: full width, rounded-lg, border `gray-300` / dark `slate-600`, focus ring `primary-500`
- `.input-error`: red border + red focus ring
- `.label`: `text-sm` `font-medium` `text-gray-700` / dark `text-gray-300`

### Badges (`packages/ide-core/src/components/ui/index` — `.badge` utility)
- `.badge`: `inline-flex`, `rounded-full`, `text-xs`, `font-medium`
- `.badge-primary`: light tint (`primary-100` / dark `primary-900`)
- `.badge-success`: green tint (`green-100` / dark `green-900`)
- `.badge-warning`: yellow tint (`yellow-100` / dark `yellow-900`)
- `.badge-danger`: red tint (`red-100` / dark `red-900`)
- `.badge-gray`: neutral (`gray-100` / dark `gray-700`)

---

## Submarks & Favicon

- **Brand logo**: `android-chrome-512x512.png` — sidebar brand mark, splash screens, app tiles
- **Submark / touch icon**: `apple-touch-icon.png` (180×180) — iOS home screen, PWA
- **Favicon**: `favicon.ico` (multi-size 16/32/48) — legacy browser tabs
- **Favicon PNGs**: `favicon-16x16.png`, `favicon-32x32.png` — modern browser tabs
- **PWA icons**: `android-chrome-192x192.png`, `android-chrome-512x512.png` — Android/PWA install
- **Manifest**: `site.webmanifest` — PWA name, icons, theme colour

Installed at:
- `packages/ide-core/src/assets/brand/` — shared by web and desktop hosts (imported by `Layout.tsx`)
- `apps/ide-web/public/` — served statically; referenced by `apps/ide-web/index.html`

---

## Accessibility & Motion

- Focus: `.focus-visible-ring` — `outline-none ring-2 ring-primary-500 ring-offset-2`
- Reduced motion: `prefers-reduced-motion` removes all animations and sets `transition-duration: 0.01ms`
- Contrast: body uses `text-gray-900` / `dark:text-gray-100` on `bg-gray-50` / `dark:bg-slate-900` (WCAG AA compliant); primary buttons use white text on `primary-600` (`#0284c7`)
- Drag/drop: `.drag-overlay` (`fixed inset-0 z-50 pointer-events-none`), `.drop-zone` (`border-dashed primary-500` background tint)

---

## Brand Name
- **Product**: Bonafoid (formerly No-Code IDE)
- **Directory / package**: `Nocode` / `nocode-ide` (unchanged for repository identity)
- **Public URL**: `http://localhost:8081`
