# Design System: Light Whisper Native macOS

## 1. Visual Theme & Atmosphere
A restrained native macOS workspace with warm monochrome surfaces, editorial spacing, and asymmetrical control panels. The interface should feel like a studio console rather than a default utility app: quiet, dense where work happens, and calm everywhere else.

## 2. Color Palette & Roles
- **Canvas Sand** (`#F4F1EA`) — Main application background
- **Canvas Shade** (`#ECE7DD`) — Secondary background wash and ambient gradients
- **Panel White** (`rgba(255,255,255,0.94)`) — Primary card and surface fill
- **Panel Warm** (`#F8F5EF`) — Secondary inset surfaces
- **Panel Accent** (`#F0ECE4`) — Editors, transcript wells, and composed empty states
- **Ink** (`#171513`) — Primary text and dark overlay fill
- **Muted Ink** (`#6B665E`) — Descriptions, metadata, and secondary labels
- **Border Linen** (`#DDD6CA`) — Structural borders and card outlines
- **Divider Mist** (`#E8E1D7`) — Keylines and subtle dividers
- **Workflow Rust** (`#A14A2C`) — Dictation and capture emphasis
- **Workflow Amber** (`#B77424`) — Translation and processing emphasis
- **Workflow Moss** (`#53714F`) — Assistant and validation emphasis
- **Workflow Ocean** (`#3E667A`) — Secondary system surfaces and navigation focus

## 3. Typography Rules
- **Display:** SF Pro Display / rounded system styles, semibold, tight tracking (`-0.6` to `-1.2`)
- **Body:** SF Pro Text / system body, relaxed line spacing, no more than ~70 characters per line in descriptive copy
- **Mono:** SF Mono / monospaced system styles for metadata, diagnostics, and eyebrow labels
- **No generic visual drift:** avoid default `headline/subheadline` stacks without intentional size, weight, and tracking choices

## 4. Component Stylings
- **Panels:** Rounded 26px cards with warm-white fill, linen stroke, and a second accent stroke tied to the active section
- **Buttons:** Rounded rectangles, tactile scale-down on press, no neon glows, no default blue accent leakage
- **Metric tiles:** Small inset cards with monospaced labels and compact rounded values
- **Sidebar rows:** Icon-led blocks with two-line summaries and a muted selection tint
- **Editors and transcript wells:** Warm inset surfaces with 1px structural borders, never raw `TextEditor` chrome
- **Overlay:** Darkened ink surface with the same workflow accent mapping as the app shell

## 5. Layout Principles
- Use asymmetrical splits instead of equal-width dashboards
- Keep the main window as a wide control desk: narrative/status left, metrics right, work surfaces below
- Keep settings as a left navigation rail plus right detail pane; avoid giant continuous forms
- Preserve desktop-first density while keeping consistent internal spacing (`16`, `18`, `22`, `24`, `28`)
- Use generous section breaks and keylines rather than stacking everything with default group spacing

## 6. Motion & Interaction
- Limit motion to hover/press feedback and surface changes
- Use scale-down press feedback for buttons (`~0.985`)
- Use opacity and stroke changes for selection and workflow emphasis
- Do not animate layout-critical properties or introduce decorative motion that competes with dictation work

## 7. Consistency Rules
- All new native views should use `AmbientCanvas`, `ChromePanel`, `ChromeSectionHeader`, and `ChromeButtonStyle`
- Workflows must keep the same accent mapping across main window, settings, history, and subtitle overlay
- Menu bar actions and overlay affordances must inherit the same terminology used in the main window
- Avoid introducing extra fonts, image assets, or external UI dependencies unless packaging and resources are updated end-to-end

## 8. Anti-Patterns
- No default SwiftUI accent blue as the primary visual identity
- No generic translucent blobs or purple AI gradients
- No raw `Form`-only settings pages
- No equal three-column “dashboard template” rows
- No oversaturated warning colors unless the state is actually destructive
- No unstructured piles of toggles, text fields, and pickers without panel grouping
