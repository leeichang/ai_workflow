---
name: Precision Industrial ERP
colors:
  surface: '#f7f9fb'
  surface-dim: '#d8dadc'
  surface-bright: '#f7f9fb'
  surface-container-lowest: '#ffffff'
  surface-container-low: '#f2f4f6'
  surface-container: '#eceef0'
  surface-container-high: '#e6e8ea'
  surface-container-highest: '#e0e3e5'
  on-surface: '#191c1e'
  on-surface-variant: '#434655'
  inverse-surface: '#2d3133'
  inverse-on-surface: '#eff1f3'
  outline: '#737686'
  outline-variant: '#c3c6d7'
  surface-tint: '#0053db'
  primary: '#004ac6'
  on-primary: '#ffffff'
  primary-container: '#2563eb'
  on-primary-container: '#eeefff'
  inverse-primary: '#b4c5ff'
  secondary: '#565e74'
  on-secondary: '#ffffff'
  secondary-container: '#dae2fd'
  on-secondary-container: '#5c647a'
  tertiary: '#46566c'
  on-tertiary: '#ffffff'
  tertiary-container: '#5e6e85'
  on-tertiary-container: '#e9f0ff'
  error: '#ba1a1a'
  on-error: '#ffffff'
  error-container: '#ffdad6'
  on-error-container: '#93000a'
  primary-fixed: '#dbe1ff'
  primary-fixed-dim: '#b4c5ff'
  on-primary-fixed: '#00174b'
  on-primary-fixed-variant: '#003ea8'
  secondary-fixed: '#dae2fd'
  secondary-fixed-dim: '#bec6e0'
  on-secondary-fixed: '#131b2e'
  on-secondary-fixed-variant: '#3f465c'
  tertiary-fixed: '#d3e4fe'
  tertiary-fixed-dim: '#b7c8e1'
  on-tertiary-fixed: '#0b1c30'
  on-tertiary-fixed-variant: '#38485d'
  background: '#f7f9fb'
  on-background: '#191c1e'
  surface-variant: '#e0e3e5'
typography:
  headline-xl:
    fontFamily: Noto Sans
    fontSize: 24px
    fontWeight: '600'
    lineHeight: 32px
    letterSpacing: -0.01em
  headline-lg:
    fontFamily: Noto Sans
    fontSize: 18px
    fontWeight: '600'
    lineHeight: 24px
    letterSpacing: -0.005em
  title-md:
    fontFamily: Noto Sans
    fontSize: 15px
    fontWeight: '600'
    lineHeight: 20px
  body-default:
    fontFamily: Noto Sans
    fontSize: 14px
    fontWeight: '400'
    lineHeight: 20px
  body-dense:
    fontFamily: Noto Sans
    fontSize: 13px
    fontWeight: '400'
    lineHeight: 18px
  label-header:
    fontFamily: Noto Sans
    fontSize: 12px
    fontWeight: '600'
    lineHeight: 16px
    letterSpacing: 0.02em
  label-caption:
    fontFamily: Noto Sans
    fontSize: 11px
    fontWeight: '400'
    lineHeight: 14px
  data-mono:
    fontFamily: JetBrains Mono
    fontSize: 13px
    fontWeight: '500'
    lineHeight: 18px
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  gutter: 1rem
  margin: 1rem
  space-xs: 0.25rem
  space-sm: 0.5rem
  space-md: 0.75rem
  space-lg: 1rem
  space-xl: 1.5rem
---

## Brand & Style

This design system targets Taiwanese manufacturing management, specifically operations, sales, purchasing, inventory, and accounting personnel transitioning from paper manifests and complex legacy spreadsheets. The interface prioritizes high-density legibility, immediate scanability, and functional reassurance over decorative trends.

The design movement is **Utilitarian Modern Enterprise**—fusing the crisp typographic structure of modern productivity tools with the dense spatial pragmatism of classic enterprise resource planning (ERP) platforms. The emotional tone is dependable, methodical, and low-friction, eliminating cognitive fatigue across 8-hour desktop operational workflows. 

Every UI element serves a direct data display or data entry function. Ornamentation is stripped away; structure, alignment, and semantic color communicate hierarchy and system status.

## Colors

The design system uses a strictly light-mode foundation built on industrial whites and cool slate neutrals. 

- **Primary (`#2563EB`)**: Anchor blue representing stability and operational precision. Used for primary calls-to-action, active navigational states, and focus boundaries. Hover state shifts to `#1D4ED8`; interactive selection backgrounds use `#EFF6FF`.
- **Secondary (`#0F172A`)**: Deep slate for primary body text, high-emphasis headers, and selected row indicators.
- **Tertiary (`#64748B`)**: Mid-tone slate for supporting metadata, table column headers, and structural icons.
- **Neutrals**: Canvas background rests on `#F8FAFC`, panel headers and neutral striping use `#F1F5F9`, and surface cards sit on crisp `#FFFFFF`. Component dividers and grid lines strictly utilize `#E2E8F0`.

### Functional Status System
Semantic status tokens are applied sparingly to preserve their urgency:
- **Success (`#16A34A`)**: Used for completed production orders, balanced ledgers, and positive audit checkpoints. Background tint: `#F0FDF4`.
- **Warning (`#D97706`)**: Low raw-material inventory alerts, unverified purchase orders, and pending approvals. Background tint: `#FFFBEB`.
- **Danger (`#DC2626`)**: Critical machine failures, payment delinquencies, and negative variance indicators. Background tint: `#FEF2F2`.

## Typography

Typography prioritizes Traditional Chinese glyph legibility alongside tabular numerical data. The font stack defaults to **Noto Sans** (with standard fallbacks: `PingFang TC`, `Microsoft JhengHei`, `sans-serif`) for optimal visual rendering across Windows and macOS manufacturing office workstations.

- **Scale & Density**: Form inputs, standard labels, and body text use `body-default` (14px). Dense enterprise data tables, ledger lines, and BOM (Bill of Materials) trees use `body-dense` (13px) to maximize row visibility without eye strain.
- **Numbers & Accounting**: SKU numbers, inventory counts, currency, and timestamps utilize tabular numbers (`font-variant-numeric: tabular-nums`) or `data-mono` using **JetBrains Mono** to guarantee vertical decimal alignment across multi-row tables.
- **Header Structure**: Restrained scale caps top-level view titles at 24px (`headline-xl`), preventing banner overhead from consuming vertical screen real estate.

## Layout & Spacing

The layout is built for high-resolution desktop enterprise environments (1280px minimum baseline, optimized for 1920×1080 display standards common in plant management offices).

### Shell Structure
- **Global Top Navigation Bar**: Fixed at `56px` height. Contains company workspace switchers, global breadcrumbs, quick search (`Ctrl/Cmd + K`), and operational role profile controls.
- **Collapsible Sidebar Navigation**: Default width of `240px`, collapsible to `48px` icon-only mode to provide maximum horizontal width for multi-column accounting sheets and production grids.
- **Primary Content Workspace**: Fluid grid spanning full remaining viewport width, framed by a consistent `16px` (`margin`) outer padding.

### Grid & Density Rules
- Content panels and form controls follow a strict 4px/8px incremental rhythm (`space-xs` = 4px, `space-sm` = 8px, `space-md` = 12px, `space-lg` = 16px).
- Internal form fields use compact vertical paddings (`6px` to `8px`) with horizontal padding of `10px`.
- Multi-column forms enforce an 8-column or 12-column subgrid with a fixed `16px` column gap (`gutter`) to ensure predictable visual grouping across complex order entry forms.

## Elevation & Depth

Visual hierarchy is maintained through **Low-Contrast Outlines** and **Tonal Layering**, deliberately avoiding heavy, fuzzy shadows that muddy scanning accuracy.

- **Borders over Shadows**: Spatial surfaces, toolbars, splitters, and cards use a 1px solid border of `#E2E8F0`. Contrast is achieved through structural line separation rather than ambient diffusion.
- **Base Canvas**: Global backdrop sits at `#F8FAFC`. Operational cards and workspaces sit atop this base in `#FFFFFF` with standard 1px `#E2E8F0` borders.
- **Hover & Active Depth**: Interactive components (dropdown triggers, clickable table rows) shift background tones rather than lifting in physical elevation. Table rows highlight using `#F8FAFC` on hover and `#EFF6FF` when selected.
- **Floating Overlays (Modals & Popovers)**: Dropdown menus, date-pickers, and modal dialogues employ a flat `#FFFFFF` surface with a 1px `#CBD5E1` border reinforced by a subtle, restrained shadow (`0 4px 12px -2px rgba(15, 23, 42, 0.08)`).

## Shapes

The design system enforces a **Soft (`1`)** shape language with predominantly 4px corner radii (`0.25rem`). This geometric discipline echoes enterprise precision, provides clear structural demarcation for dense grids, and avoids excessive curves that consume space inside compact tables and forms.

- **Form Fields & Small Buttons**: `4px` border radius (`rounded-sm`).
- **Cards, Panels & Containers**: `6px` to `8px` border radius (`rounded-md` / `rounded-lg`).
- **Status Badges & Chips**: `4px` border radius (`rounded-sm`) to maintain an architectural, tag-like feel; fully rounded pills are prohibited to distinguish operational statuses from consumer tags.

## Components

### Buttons
- **Primary**: Solid `#2563EB` background with `#FFFFFF` text. Height: `32px` (compact enterprise default), padding: `0 12px`. Hover: `#1D4ED8`. Active: `#1E40AF`.
- **Secondary / Default**: `#FFFFFF` background with 1px border `#CBD5E1`, text `#334155`. Hover: `#F8FAFC`, border `#94A3B8`.
- **Tertiary / Ghost**: Transparent background, text `#475569`. Hover: `#F1F5F9`.
- **Destructive**: Outline with `#DC2626` text and border or solid `#DC2626` background for irreversible purge actions.

### Data Tables & Grids (Core Component)
- **Header Row**: Height `36px`, background `#F1F5F9`, 1px solid bottom border `#CBD5E1`. Font: `12px`, weight `600`, text `#475569`, tracking `0.02em`.
- **Data Rows**: Default height `36px` (compact mode: `30px`). Alternating subtle striping or pure white separated by 1px bottom border `#E2E8F0`. Hover state: `#F8FAFC`.
- **Numeric Columns**: Text right-aligned with monospace formatting.
- **Actions Column**: Sticky right position, displaying quiet icon buttons on row hover.

### Form Inputs & Selects
- **Height**: Fixed `32px` with font size `13px` or `14px`.
- **Border**: 1px `#CBD5E1` border on `#FFFFFF` fill. Focus: 1px `#2563EB` border with a 1px ring (`box-shadow: 0 0 0 1px #2563EB`).
- **Labels**: Placed directly above the input, `12px` font size, weight `600`, color `#334155`, with a `4px` bottom gap.

### Status Chips & Badges
- **Structure**: Height `20px`, padding `0 6px`, font size `11px`, weight `600`, border radius `4px`.
- **Variants**:
  - *Success*: Background `#F0FDF4`, border 1px solid `#BBF7D0`, text `#15803D`.
  - *Warning*: Background `#FFFBEB`, border 1px solid `#FDE68A`, text `#B45309`.
  - *Danger*: Background `#FEF2F2`, border 1px solid `#FECACA`, text `#B91C1C`.
  - *Neutral/Draft*: Background `#F1F5F9`, border 1px solid `#E2E8F0`, text `#475569`.

### Checkboxes & Radios
- Size: `16px × 16px` with 1px `#CBD5E1` border. Checked state uses solid `#2563EB` with an internal white checkmark or center point. Hover produces a `#93C5FD` border accent.

### Cards & Grouping Containers
- Built on `#FFFFFF` surfaces with 1px `#E2E8F0` borders. Header zones feature a compact title and utility button strip with a light `#F8FAFC` background separated by a 1px border.