# GTK design pass — plan

Status: HIG chrome pass landed (kit + Contacts + Music stage). Phase 5
kit sequence 5.1–5.7 + 5.9 year rail / ADR+docs landed; leftover GTK
removal is still owner; `gtk-after/` is a written gap (mutter absent).
Named 5.8 leftovers and **5.10 leftover-parity GTK** keep Phase 5
open. Written 2026-09-16; revised 2026-09-17 (5.7 promotions + font;
5.9 year rail + close-out); revised 2026-09-18 (5.10 named)
(kit-first sequence, two-consumer rule, builders as the derivation
lever); revised again against `main` @ `9082c72` (Phase 2 split into
2a/2b/2c, accent-text rule for apps without ink, filter controls,
Gallery on `gallery-ffi` view windows, `chart-row`, Contacts settings
from spec). D1: Ubuntu 26.04 archive is GTK 4.22.2 / libadwaita 1.9.0 /
Pango 1.57.0; floor versus Fedora 44 is those versions; L6 fallbacks
not required. Fedora Contacts “before” shots are in
`docs/screenshots/gtk-before/`.
**Landed:** `shells/` gtk4 0.11 / libadwaita 0.9; `gen_r14.py` accent-fg
and authored Gallery dark surfaces; `init_style` (token CSS then kit
`data/style.css`; apps no longer ship `ADW_ACCENT`); colour-literal
check on `shells/**/*.css` and `shells/**/*.rs`; Phase 0 screenshot
harness (`shell-kit-gtk::snapshot`, debug `--route` / `--snapshot` /
`--size` / `--folder`, `scripts/gtk-snapshots.sh`); Phase 2a structure
(`adaptive_shell`, `page`, `clamped`, sized `sheet`; Contacts and Music
chrome swap; Add remains on the shared header); Phase 2b typed builders
(`ListScreen`, `SettingsScreen`, `FormSheet`, `empty_state`,
`selection_bar`; Contacts list/settings/tags/logs/edit and Music
settings assembled through them; Music playlists are a browse push);
Phase 2c shared row
polish (`Leading` on `text_row`, nav-row dim suffix + chevron,
status-row severity icons, `action_button_row`, `progress_row` as
`AdwActionRow`, `scope_toggle` / `Filter::Scope` as `ToggleGroup`,
`header_action` / `inline_primary` / `overflow(menu)`; Contacts list
passes avatar initials and letter section keys; Music L4 only). Host
`gtk-before/` PNGs are written only when mutter actually captures a
frame — this change does not commit placeholders.

**Host review 2026-09-16.** 2a clamp/page chrome is visible.
The wide IA is still a lonely column; Settings is a fourth tab; pushed
pages stack two headers so the back button sits under the switcher;
contact detail is an ungrouped field dump (Note leaks raw `ITEM1.*`);
playlist actions are four full-bleed pills; GTK never calls
`apply_metadata` (file-stem titles, Unknown Artist, 0:00). The release
`localmusic` was built **without** `gstreamer-playback`.

Revisions that land before / with Phase 3–4:

- **L1 exception.** Settings, forms, and contact field groups stay
  clamped. Library / playlist track lists and the Music browse column
  use the remaining width. A 720px card on a 1400px window is the
  empty-column feeling.
- **HIG chrome (both apps).** Settings is **not** a `ViewStack` tab
  and **not** a lone cog. GNOME primary menu is `open-menu-symbolic`
  (“Main Menu”), last group Settings / Keyboard Shortcuts / About
  {App}. Settings opens `AdwPreferencesDialog`. One header per pane;
  back is top-start of the page that pops. Add lives on the list pane.

### HIG pass — how to implement

Read against [developer.gnome.org/hig](https://developer.gnome.org/hig/)
(principles, guidelines, all four pattern groups) and libadwaita 1.9
adaptive layouts. This supersedes the earlier “settings cog” note.

**Principles.** One job per view; progressive disclosure; frequent
actions close, rare actions in a menu; do automatically when we can
(metadata, disjoint merge *preview*); do not interrupt (toasts for
events, banners for ongoing states). ADR 0007 R4 still requires a
named confirm for destructive writes — keep that, do not invent undo
as a replacement.

**Navigation (guidelines + patterns).** Prefer in-window over extra
windows. Flat: view switcher for 3–5 *equivalent* views, sidebar for
many or *dynamic* locations. Hierarchical: list → object, **one**
level, back top-start. Preferences / About are secondary windows by
convention. Do not mix those types in non-standard ways.

| App | Top-level | Hierarchical | Secondary windows |
|---|---|---|---|
| Contacts | none (one primary view) | list \| detail via `AdwNavigationSplitView` | Settings, About, conflict sheet, edit form |
| Music | view switcher: Songs · Artists · Albums · Playlists | Artist / album / playlist → that location’s tracks (one push, back). Now Playing is the wide stage, not a tab | Settings, About, add-tracks, conflict |

Contacts must drop the bottom Contacts \| Settings bar. Music drops
Settings **and** Now Playing from the switcher. The four tabs are
equivalent *browse* views (HIG 3–5). Do **not** put those four in a
sidebar, and do not put a song list in a `navigation-sidebar`.

Now Playing is the persistent right-hand stage on wide windows. Compact
hides it behind a mini-player (tap the track to open the player sheet).

**Adaptiveness.** Same functions at 360×294 (phone) through large
desktops. Start from compact. Breakpoints we already have: compact
`max-width: 550sp` (bottom switcher). Split collapse: `max-width:
860sp` (existing L12 wide). Libadwaita examples collapse nearer
`400sp`; 860sp is right for a contact/playlist list that must stay
readable. `AdwHeaderBar` inside a split hides middle window buttons
and, when collapsed, draws the back button.

**Clamp.** HIG large-size rule: boxed lists and text get a max width.
Library / playlist *track* lists and split content panes do not sit in
a 720px card. Settings, forms, contact field groups stay `AdwClamp`.

**Chrome widgets (kit).**

- `primary_menu`: `GtkMenuButton` `open-menu-symbolic`, `primary=true`,
  tooltip and a11y “Main Menu”. Last section always Settings,
  Keyboard Shortcuts (if we ship a map), About {App}. Extra items
  (Reload, Choose Folder…) go above that group. F10. No Close / Quit.
  On a split, pack this on the **sidebar** header (HIG: menu above
  the sidebar list). On hierarchical push, hide it (HIG: primary menu
  only on the top-level view); object actions use a secondary menu
  (`view-more-symbolic`, tooltip “Menu” / “Contact Menu”).
- `preferences_dialog`: present `SettingsScreen` inside
  `AdwPreferencesDialog` (not a stack page). Label remains
  **Settings** (ADR 0007 R1). `Ctrl+,`.
- `about_dialog`: `AdwAboutDialog`.
- `split_list_detail`: `AdwNavigationSplitView` + two
  `AdwNavigationPage`s, each `AdwToolbarView` + `AdwHeaderBar`.
  Sidebar controls that affect the list live on the sidebar header.
  Content header updates with the object (Edit, overflow).
- `adaptive_shell`: Music only, **four** browse roots (Songs · Artists
  · Albums · Playlists). Contacts stops using it. Kill the double
  header: a shared shell `HeaderBar` plus `page()`’s inner bar is why
  Back sits under the switcher. Music roots use the shell header;
  artist / album / playlist tracks *push* and hide the shell header
  (HIG hierarchical: back on the object page). Compact: browse is
  full width, mini-player + bottom `ViewSwitcherBar`.
- Search: `GtkSearchBar` under the list-pane header. Activate with
  type-to-search, `Ctrl+F`, header `loupe-symbolic`. Library may keep
  a permanent entry (HIG allows that when search is central). Live
  filter; empty → symbolic `AdwStatusPage` “No Results”.
- Selection (Contacts): `.selection-mode` header (Cancel + “N
  selected”) + bottom `GtkActionBar`. HIG wants ≥3 bulk actions;
  today we have Assign Tag and Delete — add Export as the third or
  keep two and document the exception.
- Edit mode (playlists): HIG’s own example. Header Edit → inline
  remove / reorder; Done to exit. Not four stacked pills.
- One suggested **or** destructive control per view. Playlist body:
  one `pill` “Play All”. Delete lives in the secondary menu and
  still confirms (R4).
- Dialogs: `AdwDialog` / `AdwAlertDialog`. Cancel before affirmative.
  Affirmative is a verb (Save, Delete, Keep). Never surprise-modal.
- Placeholders: illustration `AdwStatusPage` only for first-run no
  folder; symbolic for empty folder / empty playlist / no results.
- Banner = ongoing (Sync Conflict). Toast = event. Header-bar
  buttons on the primary window all get tooltips.

**Copy (writing style).** Header capitalization on buttons, menus,
titles. Sentence capitalization on field labels. No trailing period
on single-line toasts/headings. Ellipsis when more input is required
(`Delete Contact…`, `Choose Folder…`). ADR 0007 words stay exact:
Folder, Reload, Sync Conflict, Settings.

**Keyboard.** `Ctrl+,` Settings; `Ctrl+F` search; `Ctrl+N` new
contact / playlist; `Ctrl+R` Reload; `Alt+Left` back; `F10` menu;
Space play/pause on Now Playing. `Ctrl+W` / `Ctrl+Q` are already
application defaults. Shortcuts dialog if we ship more than a handful.

**Do not use `AdwSidebar` for the contact list.** That widget is a
short list of locations. Contacts is a long sectioned content list
in the sidebar *pane* (`navigation-sidebar` rows). Playlists *can*
use `AdwSidebar` (dynamic locations) or the same list binding;
prefer the existing list until a second sidebar-of-locations exists.

**Spec.** `contact-list` already says Settings is chrome, not a row.
Music `settings` stays a screen kind; GTK presents it as a dialog.
No new R4 kind. Playlist `actions` section is assembled as header +
one pill, not `action-row`s in the scroll.

**Landed in this pass.** Kit: `primary_menu`, `about_dialog`,
`preferences_dialog`, `split_list_detail`, `pill_primary`,
`search_chrome`. Contacts drops the view switcher for a list|detail
split; Settings is a preferences dialog; detail is hero + groups;
conflicts show a field diff. Search is a HIG `GtkSearchBar` under the
header (loupe, type-to-search, Ctrl+F), not an inline entry next to
Select / tags. Results name the matching field and bold the hit
(`Phone: **650**…`) with a symbolic type icon. Music is a 4-tab
browse switcher (Songs · Artists · Albums · Playlists); Now Playing
is the wide stage (mini-player when compact). Artist / album /
playlist open as one hierarchical push of that location’s tracks.
Playlist body is one Play All pill + header actions. `lofty` drains
metadata **and** embedded artwork after folder open. Playback is
still `--features gstreamer-playback` on the host build.
- **Contacts wide.** `AdwNavigationSplitView`: sidebar list
  (`navigation-sidebar`) + styled detail (avatar 96, `title-2`,
  groups). Empty content: "Select a contact".
- **Conflict.** Field-level diff (both sides), editable surviving
  values, confirm that names what is deleted. No silent "Resolve".
- **Music metadata.** Host port in `music-gtk` using `lofty` (no
  GStreamer required). Drain `metadata_requests` → `apply_metadata`.
- **Music playback.** Rebuild `--features gstreamer-playback` when
  `gstreamer-1.0` devel is on the machine. Do not invent a second
  engine.
- **Music wide.** Browse column (unclamped flush lists) + persistent
  Now Playing column. Compact: browse full width + mini-player
  (tap → Now Playing sheet). Track lists are flush (not boxed).
  Playlist header: add + overflow. One in-content `Play All` pill.

Goal: take the design language of the iOS apps (screenshots in
`docs/screenshots/local*-*.jpg`, also on
<https://j23n.com/public/posts/2026/localios>) and express it in native
GTK 4 / libadwaita across Contacts, Music and Gallery. The current GTK apps
work; this is about layout, hierarchy, density and polish, not behaviour.

It is **not** a port of iOS chrome. ADR 0004 still governs:

- R2: the UI spec stays semantic. Nothing in this plan adds geometry to
  `screens.toml`. Presentation is chosen by the shell from screen kind +
  item kind.
- R8: native controls and navigation, no imitation of another platform.
- R10–R12: brand = name, icon, accent, copy, IA. Colours only from generated
  tokens or libadwaita named colours.
- R14: generate only enums, screen ids, and tokens. Shells never read the
  spec at runtime. Typed Rust builders in the kit are compatible; a spec
  interpreter is the framework this ADR exists to prevent.

Health is out of scope for this pass. Keep the kit domain-neutral so a
later `health-gtk` can consume `ListScreen` / `SettingsScreen` / `FormSheet`
without inheriting photo tiles or a mini-player.

`docs/IMPLEMENTATION-PLAN.md` is the living backlog. This file is the
GTK design sequence. When a finding here changes coverage, kit status,
or phase ownership, update the implementation plan in the same change.

The kit already binds Health's `chart-row` (R4 amendment, `6210119`).
It has no GTK app consumer yet, so it is audited in 2c but not redesigned.

## Decisions (settled with the owner)

| # | Decision |
|---|---|
| D1 | Platform baseline is **Ubuntu 26.04 and Fedora 44** (GNOME 50 stack: GTK 4.22, libadwaita 1.9, Pango 1.57). Fedora 44 verified locally: GTK 4.22.4, libadwaita 1.9.3, Pango 1.57.1. Ubuntu 26.04 (resolute) archive: libgtk-4-1 / libgtk-4-dev 4.22.2, libadwaita-1-0 / libadwaita-1-dev 1.9.0, libpango-1.0-0 1.57.0 (26.04.1 desktop images have GTK 4.22.4 / libadwaita 1.9.1). Floor is GTK 4.22.2, libadwaita 1.9.0, Pango 1.57.0. L6 fallbacks are not required (libadwaita ≥ 1.7). |
| D2 | **No large in-content titles.** Page titles live in the header bar. |
| D3 | **Gallery moves onto `shell-kit-gtk`** as a new `shells/gallery-gtk` crate. The hand-built UI in `apps/gallery/linux/src/ui` stays buildable, frozen, as a reference. |
| D4 | **Music accent is red** (`#C0392B` / `#D14738`, as tokenised). The orange in the iOS screenshots is not the target. **Gallery gets light and dark** surface tokens. Dark Gallery surfaces are an **authored exception** to the current “sourced companions only” token rule; Phase 1 must update `design/tokens/README.md` to say so. |
| D5 | Gallery memory titles use a **bundled font**: Newsreader Italic (SIL OFL 1.1). `Design.swift` already names Newsreader as the intended face. This is a brand-as-typeface exception (ADR 0004 R10/R11 amendment). **Do not block Phases 0–4 on it** — land with `gallery-gtk` in Phase 5. |

## How to iterate

Screens are **assembled**, not generated. Feedback belongs in the layer
every later screen will inherit:

| Bucket | Lands in | When |
|---|---|---|
| Screen inventory, sections, affordances, destinations | `apps/<app>/ui-spec/screens.toml` | That app, both platforms |
| Cannot be said with the current R4 kinds | Amend ADR 0004 + `vocabulary.toml`, then both kits | Immediately, before inventing a widget |
| Copy / Settings order / empty-state cases | ADR 0007 | When it is a convention, not a GTK idiom |
| Accent / surfaces | `design/tokens/*.toml` + `gen_r14.py` | Phase 1 |
| How a *kind* looks on GTK | `shell-kit-gtk`, **only once a second app uses it** | Phase 2, or promotion in the phase that adds the second consumer |
| Facts a row carries | App-core display/command DTOs | With the screen that needs them |
| Host ports (folder picker, MPRIS, GStreamer) | The app shell | Never the kit |
| Gallery leftover chrome | `apps/gallery/ui-spec/` then `gallery-gtk` | Phase 5 — do not keep investing in `apps/gallery/linux/src/ui` |

**Two-consumer rule.** A binding enters the kit when two product apps
use it **in landed code**. A later phase that plans to use it does not
count. Until then it stays in the app (`contacts-gtk`, `music-gtk`,
`gallery-gtk`), and the phase that adds the second consumer promotes it.
That is how `choice-dropdown` and diagnostics entered the kit.

| Binding | First consumer (in-app) | Promoted to kit |
|---|---|---|
| Media row (rounded 48 thumb) + `thumb_radius` + `.thumb` | Music, Phase 4 | **5.7** — Gallery folders (64px). Face review still unbound |
| `chip_bar` (display chips; removable variant) | Contacts detail tags, Phase 3 | **5.7** — Gallery photos tag chips |
| Flush `GtkListView` + section model | Music, Phase 4, only if measured | Phase 5, if Gallery lists use the same adapter |
| `media_tile(Timeline\|Card\|Hero)`, carousel, scrim cards | Gallery, Phase 5 | Not in this pass |
| Mini-player | Music, Phase 4 | Not in this pass |

**Phases 3–5 are worked examples of L1–L13**, not a widget shopping
list. If a screen recipe cannot be said with a kit builder plus core
rows, either the design is wrong or the vocabulary is incomplete.

**The only allowed path after 2a:** apart from the in-app first
consumers in the table above, a kit gap found in an app goes
back into the kit and is re-snapshotted in every consumer. No
one-off `add_css_class` in `contacts-gtk` / `music-gtk` /
`gallery-gtk` except documented kit or libadwaita classes.

## What is wrong today (Contacts on Fedora)

Screenshots: `docs/screenshots/gtk-before/contacts-*.png`.

| Shot | Symptom | Cause |
|---|---|---|
| `contacts-list` | Boxed list runs edge to edge, top corners rounded, bottom not; rows are title-only; no sections; 1200px-wide rows | `list_box_page()` has no `AdwClamp` or margins; `text_row` has no leading visual; no section headers |
| `contacts-conflict-sheet` | Dialog collapsed to ~60px, "Resolve" clipped | `shell_kit_gtk::sheet()` sets no `content-width`/`content-height`; `AdwDialog` takes the child's tiny natural width |
| `contacts-tags` | No title, no back button; suffix buttons stretched to row height; "Add" visible on the Settings tab | `push_page()` puts content straight into `AdwNavigationPage` with no `AdwToolbarView`/`AdwHeaderBar`; one global header owns all actions; suffix buttons lack `valign(Center)` |
| `contacts-settings` | Settings groups with a column of entry-row edit icons | same sheet/row chrome as above |

Also found in code:

- `contacts-gtk` and `music-gtk` both set
  `@define-color accent_color var(--accent)`. That overrides libadwaita's
  contrast-derived accent *text* colour with the raw background accent.
- Chrome switches through a width notify (`apply_chrome`), not
  `AdwBreakpoint`. **2a:** both apps use `adaptive_shell` breakpoints.
- Empty state is a `status_row` inside the list, not an `AdwStatusPage`
  with ADR 0007 R3's three cases.
- Selection mode check boxes are `insensitive`.
- Gallery's hand-built UI says "Preferences", "Library", "Reload folder".
  ADR 0007 R1 wants Settings, Folder, Reload. Fix this in the new crate only.

The four screenshot rows split across exits (see Phase 2). Clamp, sheet
size, and page chrome are kit-only (2a exit); accent is Phase 1. Leading
avatars, letter sections, and moving Add onto the page need app data
wiring (2b/2c exits and Phase 3).

## What the kit can fix without per-app UI work

The kit cannot know a screen's sections, affordances, or where actions
go. That splits the work three ways:

| Kit only (apps unchanged or one-line swap) | Kit + small app wiring (data, not layout) | Per-app composition (by design) |
|---|---|---|
| Sheet collapse (`sheet()` sizing) | Leading avatars (app passes `leading`); thumbnails once promoted | Contacts split view |
| List clamp and inset corners (`list_box_page`) | Letter/month sections (app passes a section key) | Now Playing layout; Music mini-player |
| Pushed page title + back button (`page` wraps a toolbar view) | "Add" moving from global header to its page | Collections: memory carousel, people tiles |
| Accent contrast, dark mode (tokens + `init_style`) | Empty-state variant choice (R3) | Photo timeline, viewer, photo info |
| Status row icons, nav-row trailing, suffix alignment | Selection bar, scope filter/chips | Detail header (avatar 96 + name) |
| Breakpoints (`adaptive_shell`) | Tag row buttons (use a kit suffix helper) | Edit form photo header |

The middle column shrinks when Phase 2b lands **typed screen builders**.
The app fills a struct; the kit lays it out. Hand-written Rust, R14-safe.
The right-hand column stays per-app: those screens carry identity.

## Design language → GTK

These are the rules every app follows. The kit implements the **shared**
ones (L1–L2, L3 avatars/symbols, L4, L6–L8, L10–L13, inset L5). L3
thumbnails, flush lists (L5) and media shapes (L9) are the language for
Music/Gallery, but they stay in those apps until the two-consumer table
promotes them.

**L1 — Content column.** Lists, detail, forms and settings sit in an
`AdwClamp` (maximum-size 720, tightening-threshold 480) with 12px side
margins, so boxed lists are always inset cards with all four corners.
Media grids and the viewer are full bleed.

**L2 — Every page owns its chrome.** Each `AdwNavigationPage` is an
`AdwToolbarView` + `AdwHeaderBar` carrying that page's title, back button
and actions. The view switcher sits in the header of **root** pages only.
The bottom `AdwViewSwitcherBar` lives in the outer toolbar view. "Add"
belongs to the page that adds.

**L3 — Leading visuals.**
- Person → `AdwAvatar` (40 in rows, 96 in detail/edit), initials from the
  core row, image when a photo exists.
- Media (tracks, playlists, folders) → rounded thumbnail: 48 in flush rows,
  64 in folder rows, radius `thumb_radius`.
- Everything else → symbolic icon or nothing.

**L4 — Quiet secondary text.**
- Subtitles and trailing values use `dim-label`.
- Numbers (durations, counts) add `numeric`.
- Trailing values are suffix labels, right-aligned, `valign(Center)`.
- A chevron `go-next-symbolic` appears only when the row pushes.

**L5 — Two list styles.**
- **Inset** (kit, Phase 2b): one `boxed-list` `GtkListBox` per section
  inside L1, with a `heading` section title (contacts letters `#`, `A`…,
  settings groups, folders). Enough for lists up to a few thousand rows.
- **Flush** (app until a second consumer): windowed `GtkListView` with
  the `rich-list` style class and `GtkSectionModel` (music tracks at
  scale, gallery grids). ADR 0003 R4 requires windowing for gallery and
  music scale. Do not put Flush in the kit during Phase 2.

**L6 — Filters are visible.** Pick the control by how many options there
can be, not by what they mean:
- **Small fixed set** (at most ~5 options known at build time, e.g. log
  levels) → `AdwToggleGroup` (libadwaita ≥ 1.7). Fallback below 1.7:
  `GtkToggleButton`s grouped with `set_group` in a `linked` box.
- **Open-ended set** (tags, playlists: user data, any count) → the
  existing kit `choice_dropdown` (a `GtkDropDown` with "All …" first,
  plus search when there are more than ~10 options). A toggle group
  doesn't wrap or scroll, so twenty tags will not fit at 540px.
- **Sort** → `choice_dropdown` as today (Music uses it). It is not a
  menu button.
- Active removable filters (Gallery tags) → pill buttons with a trailing
  `window-close-symbolic`, in an `AdwWrapBox` (≥ 1.7; below that a
  horizontal `ScrolledWindow`).
- Search → `GtkSearchEntry` at the top of the content column. On compact,
  a header toggle reveals a `GtkSearchBar` if space is tight.

**L7 — One prominent action per page.**
- Page-level create/add → header icon button (`list-add-symbolic`,
  tooltip).
- Content-level main action (Play All, Choose Folder) → in-content
  `pill suggested-action`, at most one per page.
- Secondary actions → overflow `view-more-symbolic` menu.
- Link-style actions in settings → `AdwButtonRow`; destructive ones get
  `destructive-action`.

**L8 — Accent is sparse.** Accent appears only on:
- the primary action,
- selection and checked state,
- the current lyric line,
- small section glyphs (Gallery "Memories").

No accent-filled surfaces.

**L9 — Media shapes** (Gallery/Music language; implement in the app
until promotion).
- Timeline grid: square tiles, 2px gutters, no rounding.
- Cards and tiles (memories, people, albums): radius `card_radius` or
  `memory_radius`, with a bottom scrim (token `scrim`) under white-on-scrim
  text (token `on_scrim`).
- Badges (LIVE, contact-linked): `osd` pill, caption size, top corner.

**L10 — States.**
- Empty/no-folder → `AdwStatusPage` with the three R3 variants: no folder
  (icon + Choose Folder pill), folder empty, no matches (clear filters
  button).
- Loading → `AdwSpinner` status page; never "nothing found" before first
  load.
- Long work → Settings `progress-row` with cancel (R5), plus chrome
  `progress` (header spinner + label) after 500 ms. Do not reuse
  `AdwBanner` for work; banners stay folder-level status.
- Transient notices → `AdwToast`.
- Folder-level status → `AdwBanner`.

**L11 — Type.** Use only libadwaita type classes: `title-1`…`title-4`,
`heading`, `body`, `caption`, `caption-heading`, `numeric`, `monospace`.
Sentence case, never CSS uppercase; copy comes from localization. The one
exception is Gallery memory titles in Newsreader Italic (D5, Phase 5).

**L12 — Adaptive layout.** Two `AdwBreakpoint`s, applied the same way in
every app:
- `max-width: 550sp` → compact. Bottom `AdwViewSwitcherBar` revealed,
  header switcher hidden. This is ADR 0004 R8.
- `min-width: 860sp` → wide. Two-pane layouts where listed below
  (`AdwNavigationSplitView` / `AdwOverlaySplitView`). Wide split is a
  **shell decision** (kit README), not an ADR amendment, unless iOS is
  later required to match.

**L13 — Don't.**
- No floating pill tab bar.
- No glass/blur materials.
- No floating circular toolbar buttons.
- No bottom-floating search field.
- No large-title collapse.
- No iOS chevron-right in headers.
- No custom-drawn switches or segmented controls.
- No colour literals outside generated CSS.

## Phases

Each phase ends with before/after screenshots from the Phase 0 harness at
540×620 and 1280×800, light and dark, when the harness exists. CI stays
on `cargo test`. Snapshots are a local script.

Execute **0 → 1 (accent / surfaces only) → 2a (structure) →
re-snapshot Contacts → 2b (builders) → 2c (row polish) → 3**, then 4
and 5. Hold the font, thumbnails, chips, flush lists, and Gallery tiles
until their phase.

### Phase 0 — Baseline and toolchain

Work on `main` (the repository keeps no other branches or worktrees).
The base is `origin/main` @ `9082c72`: `shells/` already has
`shell-kit-gtk`, `contacts-gtk` and `music-gtk`, and the kit has
`choice_dropdown`, diagnostics, and `chart_row`. The
`list_box_page` change (Contacts + Music, `shell-kit-gtk/src/screen.rs`)
is already on the tree.

Phase 0 note (2026-09-16): Fedora Contacts “before” shots are
`docs/screenshots/gtk-before/contacts-{list,conflict-sheet,tags,settings}.png`.
Ubuntu 26.04 versions are confirmed (see D1). The
`shells/` crate bump (gtk4 0.11 / libadwaita 0.9) has landed; tests are
green on those crates. The snapshot harness is in; further mutter
captures stay optional.

1. **Landed.** Fedora screenshots renamed to
   `docs/screenshots/gtk-before/contacts-{list,conflict-sheet,tags,settings}.png`.
2. Confirm Ubuntu 26.04 versions (`apt policy libgtk-4-1 libadwaita-1-0 libpango-1.0-0`).
   Floor = the lower of 26.04 and Fedora 44. If the floor is below
   libadwaita 1.7, write the L6 fallbacks before Phase 2c uses
   `ToggleGroup`, and before Phase 5 uses `WrapBox`.
3. **Landed.** Bump the `shells/` workspace crates:
   - `gtk4` 0.8 → 0.11 (feature `v4_22`)
   - `libadwaita` 0.6 → 0.9 (feature `v1_9`)
   - `glib` / `gio` to match
   - `pango` only if something in 0–4 needs it; the Newsreader
     `FontMap::add_font_file` pin waits for Phase 5

   API drift is fixed. `apps/gallery/linux` keeps its own lockfile and stays
   on 0.8 (frozen reference). `cargo test --workspace` in `shells/` is
   green on 0.11 / 0.9.
4. Update README "Linux requirements" (GTK 4.22+, libadwaita 1.9+, Fedora
   44 / Ubuntu 26.04) and the `apt`/`dnf` lines.
5. **Screenshot harness — landed.** `shell-kit-gtk` (`snapshot` module, debug-only):
   - Launch with `--route <screen-id> --snapshot <out.png> --size WxH`.
   - After the first frame, render the window through
     `gtk::WidgetPaintable` → `gtk::Snapshot` →
     `GskRenderer::render_texture` → `Texture::save_to_png`, then quit.
   - Routes come from the generated screen ids (`ContactsScreen`,
     `MusicScreen`, later `GalleryScreen`).
   - Force the scheme with `ADW_DEBUG_COLOR_SCHEME=prefer-dark|prefer-light`.
   - Script: `scripts/gtk-snapshots.sh <app>` runs every implemented
     route × 2 sizes × 2 schemes under headless mutter
     (`--headless --wayland --virtual-monitor 1280x800`, plus `--no-x11`
     when that flag exists) when mutter is present. Fixture folders:
     Contacts `core/contacts-core/fixtures/r8/disjoint/`; Music
     `shells/fixtures/music/` (silent note; music-core r8 is playlists
     only). Without mutter the script prints a skip and exits 0.
   - Write baselines under `docs/screenshots/gtk-before/` **only when a
     capture succeeds**. Do not commit empty or invented PNGs.
   - **CI does not run mutter.** Unit tests must not require a display
     beyond what `cargo test` already does in `shells/`.
   - Gallery UI timings: `localgallery --bench --folder` prints
     `[gallery-gtk-perf]` and quits. `scripts/gtk-perf.sh smoke|20k`
     starts mutter the same way; 20k also runs ignored
     `gallery-gtk` `e2e_catalog` (no display). Not a `rust.yml` job.

Exit: `contacts-gtk` and `music-gtk` build and test on gtk4 0.11 /
libadwaita 0.9; the snapshot harness is in both apps (**landed**).
Baseline PNGs exist only after a successful mutter capture — they are
not claimed here. Gallery linux still builds on 0.8.

### Phase 1 — Tokens and theming (not the font)

1. **Generator** (`scripts/gen_r14.py`): replace the `:root { --accent }`
   output with libadwaita 1.9 variables, light plus
   `@media (prefers-color-scheme: dark)` (GTK ≥ 4.20):
   - `--accent-bg-color` from `accent`.
   - `--accent-fg-color`: the generator chooses, per scheme, among
     candidates `#FFFFFF`, `#000000`, and the app's `ink` for that scheme
     **if the app has one** (only Gallery does). Pick the highest
     contrast on `accent`. The generator **fails** if the best candidate
     is below 4.5:1. It never adjusts a sourced accent; a failure goes to
     the owner. Expected output (checked 2026-09-16):

     | App | Scheme | Accent | White | Black | Ink | Chosen |
     |---|---|---|---|---|---|---|
     | Contacts | light | `#336BC7` | 5.16 | 4.07 | — | white |
     | Contacts | dark | `#4D85DE` | 3.67 | 5.73 | — | black |
     | Music | light | `#C0392B` | 5.44 | 3.86 | — | white |
     | Music | dark | `#D14738` | **4.497** (fails) | 4.67 | — | black |
     | Gallery | light | `#C48A3E` | 2.98 | 7.05 | 5.83 (`#1C1A16`) | black |
     | Gallery | dark | `#D4994D` | 2.48 | 8.46 | 7.00 (`#1C1A16`, light-ink candidate) | black |

     Gallery dark `#1C1A16` at 7.00 is the *light-ink candidate*, not
     the authored dark ink `#F2EDE5`. Black text on a red accent in dark
     mode is what libadwaita itself does for light system accents. Review
     Music dark in the Phase 1 snapshots; if the owner rejects it, the
     options are a sourced dark accent change on both platforms, or
     accepting white at 4.497 (AA-large only) as a recorded exception.
     The generator must not round 4.497 up to pass.
   - Do **not** set `--accent-color`; libadwaita derives a readable text
     accent.
   - Keep emitting `--accent` as a leftover alias of `--accent-bg-color`
     (same hex). The kit maps `accent_bg_color` from `--accent-bg-color`;
     do not treat `--accent` as the API.
   - Optional surface roles map to `--window-bg-color`, `--view-bg-color`,
     `--headerbar-bg-color`, `--card-bg-color`, `--dialog-bg-color`,
     `--popover-bg-color`, `--window-fg-color`/`--view-fg-color`/`--card-fg-color`,
     `--destructive-bg-color`, `--border-color` (from `separator_ink` ×
     `separator_opacity`).
   - Apps without surface tokens (contacts, music) emit accent only and
     keep system surfaces (R12).
2. **No new metric tokens in Phase 1.** `thumb_radius` (8) arrives with
   its first consumer (Music media row, Phase 4). `grid_gutter` arrives
   with Gallery (Phase 5). `memory_radius` and `card_radius` already
   exist.
3. **Gallery dark companions (D4 exception).** Update
   `design/tokens/README.md`: Gallery dark surfaces are authored, not
   sourced. Starting values below. The generator's contrast test must
   pass; adjust if not. Contacts and Music do not gain invented
   surfaces.

   | Role | Light (existing) | Dark (proposed) | Contrast on dark bg / card / grouped |
   |---|---|---|---|
   | bg | `#FAF7F2` | `#1A1815` | — |
   | bg_card | `#FFFFFF` | `#26231F` | — |
   | bg_grouped | `#F2EDE5` | `#131210` | — |
   | ink | `#1C1A16` | `#F2EDE5` | 15.2 / 13.4 / 16.1 |
   | ink2 | `#5E574D` | `#B8AFA3` | 8.2 / 7.2 / 8.7 |
   | ink3 | `#958D82` | `#8C8479` | 4.8 / 4.2 / 5.1 |
   | destructive | `#B24A3A` | `#E07A68` | 6.0 / 5.3 / 6.4 |
   | accent | `#C48A3E` | `#D4994D` (exists) | 7.1 / 6.3 / 7.5 |

   Note: light Gallery accent is 2.8:1 against `bg` and 3.0:1 with white
   text. That is why `--accent-fg-color` must be computed (ink gives 5.8:1)
   and `--accent-color` left to libadwaita.
4. `scrim` / `on_scrim` wait until a consumer (Music now-playing or
   Gallery cards) needs them — Phase 4 or 5, not here.
5. Replace `apply_token_css` + per-app `ADW_ACCENT` glue with
   `shell_kit_gtk::init_style(app_token_css)`. It loads the generated token
   CSS, then the kit stylesheet (`shell-kit-gtk/data/style.css`,
   `include_str!`), both at `STYLE_PROVIDER_PRIORITY_APPLICATION`. Delete
   `ADW_ACCENT` from contacts and music.
6. Conformance: extend the colour-literal check to `shells/**/*.css` and
   `shells/**/*.rs` (generated CSS excluded).

Exit: `gen_r14.py --check` green; Contacts and Music show their accent in
light and dark without clobbering `--accent-color`; Gallery token table
has authored dark surfaces; no colour literals in the kit. **Newsreader
is not in this exit.**

### Phase 2 — Kit: structure, then builders, then row polish

Three sub-phases, each with its own exit. 2a is the quick visible win
and must not wait for the builders.

**2a. Structure (landed; fixes the screenshot bugs).**

- `adaptive_shell(app_title, pages: &[RootPage]) -> AdaptiveShell`.
  `RootPage` = id, title, icon, `NavigationView`. It builds
  `AdwToolbarView` + `AdwViewStack` + `AdwViewSwitcherBar` and both
  breakpoints (L12). Compact `title-widget` uses a typed null GValue
  (`Some(&None::<gtk::Widget>.to_value())`); a skipped `None` is a NULL
  `GValue*` and libadwaita drops the setter. This replaces
  `Window::apply_chrome` in every app.
- `page(title, content, PageChrome { start, end, root: bool }) -> adw::NavigationPage`
  → `AdwToolbarView` + `AdwHeaderBar` (switcher as title widget when
  `root`). Replaces `push_page`.
- `clamped(child) -> gtk::ScrolledWindow` per L1. `list_box_page` returns a
  clamped inset list (keep returning the `ListBox` handle; GTK 4.22 may
  wrap it in a viewport).
- `sheet(title, content, SheetSize::{Form, Picker, Alert})`: `AdwDialog`
  with `content-width`/`content-height` (Form 480×720, Picker 420×560,
  Alert 360×240), its own toolbar view/header, and
  `follows-content-size` false.

Tests: sheet has a content width, page has a header bar, list page is
clamped. No mutter.

**2a exit (kit-only column) — landed:** Contacts and Music swapped onto
`adaptive_shell` / `page` / `list_box_page` / `sheet`. This is a
mechanical swap of existing call sites with **no new row data and no
builders**. Re-snapshot Contacts when mutter can capture. These four
“wrong today” symptoms must be gone: edge-to-edge list, collapsed
conflict sheet, missing pushed-page title/back, collapsed edit sheet.
Accent contrast from Phase 1 is visible. Leading avatars, letter
sections, and moving Add are **not** in this exit.

**2b. Typed builders (landed; the derivation lever).** The app fills a struct;
the kit lays it out. Moving a screen onto a builder is a real rewrite of
that screen's assembly, which is why this is separate from 2a.

- `ListScreen { search, filter, sections, primary, selection, banner, empty }`.
  `filter` is `Filter::{Scope(options), Choice(ChoiceData)}` per L6;
  `sections` are inset only (L5).
- `SettingsScreen { groups }`. Groups are passed in the order of the
  app's `settings` sections in `screens.toml`; the builder asserts that
  Folder comes first and Info last (ADR 0007 R2) in debug builds.
- `FormSheet { title, cancel, confirm, body }`: Cancel at start, confirm
  `suggested-action` at end.
- `empty_state(EmptyKind::{NoFolder, EmptyFolder, NoMatches, Loading, Error}, copy)`.
- `selection_bar(actions)` in a `GtkRevealer` at the page bottom.

Tests: builders populate the expected children (search present iff
requested, one boxed list per section, empty state replaces the list).

**2b exit (landed):** Contacts `contact-list`, `settings`, `tag-management`,
`logs`, and `contact-edit`, plus Music `playlist-list` and `settings`,
are assembled through builders, each with **the rows it has today**.
Re-snapshot when mutter can capture. No new row data yet. Scope filters
reuse `choice_dropdown` (no `AdwToggleGroup`). Contacts settings pass
today’s rows as folder first / diagnostics / info last. Spec and both
shells now put `info` last (ADR 0007 R2). Titles are Folder / Info.

**2c. Shared row polish (landed; L3 avatars/symbols, L4, L6–L8, L10).** No
thumbnails, chips, tiles, or flush lists; see the two-consumer table.

| Binding | Change |
|---|---|
| `text_row` | `TextRowData.leading: Option<Leading>`, where `Leading::{Avatar{text, texture}, Symbol(icon)}`; trailing dim `numeric` suffix; `valign(Center)` on all suffixes |
| `nav_row` | trailing value as dim suffix (not subtitle) + chevron |
| `media_item` | **unchanged in 2c.** The 48px thumbnail row is built in `music-gtk` in Phase 4 and promoted in Phase 5 |
| `status_row` | severity icon prefix + `success`/`warning`/`error` class instead of severity text subtitle |
| `action_row` | `AdwButtonRow` in list/settings contexts; plain `gtk::Button` stays for toolbars |
| `progress_row` | `AdwActionRow` with inline `GtkProgressBar`, optional cancel (R5) |
| `toggle_row`, `field_row` | unchanged except L4 alignment |
| `chart_row` | **audit only** (Health, no GTK app consumer): trailing latest value gets `dim-label numeric` (L4); sparkline keeps taking its colour from the widget style (`area.color()`, no literals); no layout redesign |
| `search_entry` | placed by `ListScreen`, not a free-floating helper |
| `filter` | `scope_toggle(options) -> adw::ToggleGroup` for small fixed sets; existing `choice_dropdown` for open-ended sets (L6) |
| `sort` | existing `choice_dropdown` (unchanged API) |
| `overflow` | menu button built from a `gio::Menu` the app passes |
| `primary_action` | `header_action(icon, tooltip)` and `inline_primary(label, icon)` (`pill suggested-action`) |
| `grid_page` | leave as-is; `FlowBox` removal and `GtkGridView` wait for Gallery |

Stay out of the kit in 2c (in-app first consumer, then promotion per
the two-consumer table):

- Media row with rounded thumbnail, `thumb_radius`, `.thumb`
- `chip_bar`
- Flush `GtkListView` + `GtkSectionModel`
- `media_tile(Timeline | Card | Hero)`, `media_grid`, carousel, scrim cards
- Mini-player chrome
- `.timeline-grid`, `.media-card`, `.hero-card`, `.lyrics`, `.badge`

Kit stylesheet after 2c: no custom classes are needed yet. The file
exists (from Phase 1) and holds only token-driven overrides.

Tests: data → widget (leading/trailing present, status icon per
severity, chart trailing is dim). No mutter.

**2c exit (landed):** Contacts passes `leading` (initials from the
contact title), letter section keys derived from that title, and
empty-state kind through the existing builders. Re-snapshot when mutter
can capture. Add-on-the-page and one-boxed-list-per-letter remain
Phase 3 (app data, not kit scope creep). `media_item` is unchanged.

### Phase 3 — Contacts

Worked example of L1–L13 using the Phase 2 builders. Kit gaps go back
to the kit and get re-snapshotted.

| Screen | Compact (≤550) | Wide (≥860) |
|---|---|---|
| folder-picker | `empty_state(NoFolder)`: `address-book-symbolic`, app name, one sentence, pill "Choose Folder" | same, clamped |
| contact-list | `ListScreen`: root header, end add + selection toggle; search; tag filter as `choice_dropdown` ("All tags" first, tags with counts; L6 open-ended set); Inset sections by letter (`#` first); rows = avatar 40 + name + subtitle (org or first phone/email, from core row) + chevron. Conflict `AdwBanner` with "Review". Selection: active check prefixes + `selection_bar` (Assign Tag, Delete destructive → confirm) | `AdwNavigationSplitView`: list in sidebar (`navigation-sidebar` rows), contact-detail in content; empty content shows `empty_state` "Select a contact". Split view is per-app composition. |
| contact-detail | Header end: Edit, overflow (Export, Delete). Centered avatar 96, name `title-2`, org `dim-label`. One `AdwPreferencesGroup` per field kind; rows copyable; tags as a display-only chip row built **in `contacts-gtk`** (first `chip_bar` consumer; `pill` buttons, libadwaita classes only) | content pane of the split |
| contact-edit | `FormSheet` "Edit Contact"/"New Contact", Cancel/Save. Centered avatar 96, flat accent "Change Photo", flat destructive "Remove Photo". Groups: Name (First, Middle, Last, Prefix, Suffix entry rows), Organization (Company, Job Title, Nickname), Phone/Email/URL (repeated rows with remove + `AdwButtonRow` "Add …"), Addresses, Birthday, Tags, Notes | same sheet, 480 wide |
| settings | `SettingsScreen`, groups **exactly from the spec's `settings` sections**, rows only for facts and actions the core exposes today. Do not add iOS-only rows such as Storage Layout or Last Synced. On `main` the spec lists `folder, sync, tags, info, diagnostics`; see the notes after this table | clamped |
| tag-management | `page("Tags")`, Inset list, row = tag + dim count + flat circular rename/remove centred; `empty_state` when none | same |
| logs | `page("Logs")`, header search toggle, level filter as `scope_toggle` (small fixed set), **Inset** list (not Flush), `monospace` message, dim timestamp | same |
| sync-conflict-group | `sheet(Picker)` **field diff**, not a silent merge. Intro names the contact. Per-field rows: label + both values (Local / Incoming), radio or editable surviving value. Auto-mergeable fields pre-selected but still visible. Confirm names every discarded value (ADR 0007 R4). No "Resolve" that writes without a preview | same |

Contacts settings notes. Landed on both shells:

- **Order.** Spec is `folder`, `sync`, `tags`, `diagnostics`, `info`
  (Info last, ADR 0007 R2). GTK omits `sync`.
- **`sync` section.** Apple Contacts sync is iOS-only (ADR 0007 R15).
- **Group titles.** **Folder** and **Info** (ADR 0007 R1/R2).
- **Rows per group on GTK:**
  - Folder: path subtitle, Change Folder, Reload.
  - Tags: nav row with trailing count.
  - Diagnostics: Logs nav row.
  - Info: counts and version.

  Anything more needs a core fact first.

Exit: after-shots for every routed Contacts screen; `cargo test
--workspace` green; no `add_css_class` string outside the kit's
documented classes or libadwaita classes.

### Phase 4 — Music

Accent red (D4). Worked example of the same builders. **The media row
with a 48px rounded thumbnail, `thumb_radius` (token added here), the
mini-player, and Flush lists live in `music-gtk`**. The media row is
promoted in Phase 5; the others stay until a second consumer exists.
Settings groups follow Music's `settings` spec sections, with the same
rule as Contacts: no rows the core doesn't expose.

| Screen | Compact | Wide |
|---|---|---|
| folder-picker | as contacts, `folder-music-symbolic` | — |
| library (Songs) | Flush track list; sort on the shell header. Activate plays from that row through the rest of the visible list | Same list in the browse column; Now Playing stays on the right |
| artists / albums / playlists | Location list (art + name + count). Activate pushes that location’s tracks with back + Play All | Same, still one push in the browse column — not a second sidebar |
| mini-player | Music chrome, not a new R4 kind. Bottom bar above the view switcher: 40 art, title/artist, play/pause, next. Click the track → Now Playing sheet | Hidden; the Now Playing column is the stage |
| now-playing | Sheet from the mini-player. Large rounded art; title; transport | Persistent right column (~400px). Not a switcher tab |
| playlist-detail | Header: back, title, add tracks, overflow (delete). One in-content `Play All` pill. Count caption. Flush track rows. Edit sheet for remove / reorder | Same pushed page in the browse column |
| add-tracks | `FormSheet` Cancel / "Add (n)", search, rows with check prefixes | — |
| settings / logs / sync-conflict-group | kit builders from Phase 3 | — |

Exit: after-shots; headless flow test (`tests/headless_flow.rs`) still
green. 20k-track scroll is a **measurement**, not a Phase 4 blocker: if
Inset janks, implement Flush in `music-gtk` and record whether the kit
should take it (needs a second consumer).

### Phase 5 — Gallery on the kit

Living slices and gates: [`IMPLEMENTATION-PLAN.md`](IMPLEMENTATION-PLAN.md)
§Phase 5 (5.1–5.9). Kit sequence 5.1–5.7 + 5.9 year rail / ADR+docs
landed. Leftover GTK removal is still **5.9 owner**. Next is named
5.8 leftovers and **5.10 leftover-parity GTK** (Phase 5 stays open).

1. **Spec first (5.1)**: write `apps/gallery/ui-spec/screens.toml` from the
   hand-built UI and iOS IA, R4 kinds only. Extend `gen_r14.py` →
   `GalleryScreen`. Proposed ids and kinds:
   - `folder-picker` (detail)
   - `folders` (list)
   - `folder` (grid)
   - `photos` (grid; search, filter)
   - `collections` (list)
   - `memory` (grid)
   - `people` (grid)
   - `person` (grid)
   - `events` (list)
   - `album` (grid)
   - `face-review` (list; selection, confirm)
   - `viewer` (viewer; overflow)
   - `photo-info` (detail)
   - `settings` (settings)
   - `logs` (list)

   If a screen will not fit, stop and amend R4. Don't invent a widget.
2. **Data source: `gallery-ffi` view windows, not the old host UI
   paths.** `main` has generation-checked, sectioned view models in
   `apps/gallery/core/gallery-ffi` (`e90b4c3`, `cf20c42`, `695d903`),
   and iOS already uses them. `gallery-gtk` consumes the same ones as a
   Rust library (`crate-type` includes `lib`) and doesn't implement its
   own windowing.
   - `ViewStructure { state, sections, actions, generation }` drives the
     screen. `ViewContentState::{Loading, Empty, Content, Error}` maps to
     `empty_state` (L10). `sections` map to `GtkSectionModel` headers.
     `actions` drive enabled/disabled chrome.
   - Items come from `photo_window` / `tag_window(section_id, offset, limit, generation)`,
     at most `MAX_VIEW_WINDOW` (256). The `gio::ListModel` adapter fetches
     pages on demand.
   - On `ViewError::StaleGeneration`, re-read the structure and replace
     the model. Never patch items across generations.
   - **Gap:** only the photo and tag windows exist today. Folders,
     collections (memories, people, events, albums) and face review have
     no window API. Add them to `gallery-ffi` first, with the R6
     conformance check and a Swift consumer or explicit iOS deferral.
     Don't build them in `gallery-gtk`.
3. **Crate**: `shells/gallery-gtk` depends on `gallery-ffi` (path
   `../../apps/gallery/core/gallery-ffi`) for view models, and on
   `localgallery = { path = "../../apps/gallery/linux", default-features = false }`
   only for host concerns (config, XDG thumbnails, folder watch, ops).
   Binary `localgallery`, app id `com.j23n.LocalGallery`.
   - **Workspace risk.** Path dependencies into `apps/gallery/core` are
     resolved by the `shells/` lockfile, not Gallery's. Before writing UI:
     - copy Gallery's exact pins (`image = "=0.25.10"`, `uniffi`, `ort`
       behind `ml`) into the shells workspace;
     - `cargo tree -d` in `shells/` shows no second copy of those;
     - `cargo test` in `apps/gallery/core` stays green.
   - **Build cost.** Cargo builds `gallery-ffi`'s `staticlib`/`cdylib`
     types for dependents too. If that is slow or fails to link, split
     the view-model code into an rlib crate (e.g. `gallery-view`) that
     `gallery-ffi` re-exports. That is a core change shared with iOS, not
     a GTK workaround.
4. **Reference**: rename the old binary to `localgallery-reference` with
   app id `com.j23n.LocalGallery.Reference`, so both can run side by side.
   Mark `apps/gallery/linux/src/ui` frozen in its README: no new features,
   removed once gallery-gtk reaches parity and the owner agrees.
5. **Font (D5), landed in 5.7.**
   - `design/fonts/Newsreader-Italic[opsz,wght].ttf` + `OFL.txt` (SIL OFL 1.1).
   - ADR 0004 R10/R11 amended: `[fonts] display` is brand when present.
   - `gallery-gtk` loads the file with `pango::FontMap::add_font_file` on
     the default map (`CARGO_MANIFEST_DIR/../../design/fonts`, then XDG,
     then `/app/share/fonts`). Missing file logs and continues. No
     fontconfig crate.
   - Only `gallery-gtk` CSS uses the family, in class `.memory-title`.
6. **Screens** (worked example). Timeline tiles, hero cards, carousel,
   and viewer chrome start in `gallery-gtk`. Promotions in **5.7**,
   because Gallery is the second consumer:
   - Music's media row + `thumb_radius` + `.thumb` are in the kit
     (Gallery folders at 64px). Face review stays unbound.
   - Contacts' chip row is kit `chip_bar` with a removable variant
     (Gallery photos tags + typeahead). `AdwWrapBox` per L6.
   - Music's Flush list adapter stays in Music. Gallery does not use
     the same section adapter.
   - `grid_gutter` token drives `GtkGridView` spacing.

| Screen | Compact | Wide |
|---|---|---|
| root tabs | Folders, Collections, Photos (this order), icons `folder-symbolic`, `view-grid-symbolic` (or bundled symbolic), `image-x-generic-symbolic` | same |
| photos | Root; end: overflow. Search, removable tag chips with typeahead. Timeline grid full bleed, month headings, square cover-fit tiles, 2px gutter, LIVE badge. Column count from width (~110sp compact, ~160sp wide). Activate → viewer | same, more columns |
| folders | Root; sort as `choice_dropdown`. Inset "Subfolders": 64 `thumb`, name, photo count dim `numeric`, chevron. Below: that folder's photos as a timeline grid | clamp for the list, grid full width |
| collections | Root. **Memories** carousel of hero cards (scrim, Newsreader Italic title, dim caption) + dots. **People**: card tiles, name + count on scrim (MWG crop in **5.10-face-crops**). **Events** rail. Leftover Objects / Scenes / Places / Albums rails are **dropped** | Memories as a rail of 2–3 hero cards; People 5–6 columns |
| memory / person / album / folder | `page(title)` + timeline grid; memory adds a header subtitle with dates | same |
| face-review | Inset list with face crops as leading `thumb`, name entry, `selection_bar` | same |
| viewer | Full bleed, `osd` bars, auto-hide, swipe/zoom. Overflow: Info, Open With, Show in Folder. **5.10-viewer-chrome** lands zoom, filmstrip, prev/next, tap-to-hide (swipe + video already in kit) | `AdwOverlaySplitView` with photo-info sidebar |
| photo-info | `AdwBottomSheet` (compact): Date, Camera (Make/Model, **5.10-photo-info**), Location (place and/or GPS, no map), Tags, People, File, Faces, Sidecar | sidebar |
| settings | `SettingsScreen` with groups from the new Gallery spec's `settings` sections (proposed: Folder, Scan Photos with `progress_row` + cancel, Diagnostics, Info last). Vocabulary per ADR 0007 R1 | clamped |

7. Windowing: grids and large lists use `GtkGridView` / `GtkListView`
   over the `gio::ListModel` adapter from step 2 (ADR 0003 R4). No
   `FlowBox`. Thumbnails resolve `thumbnail_ref` in factory `bind` and
   cancel in `unbind`.
8. Year rail on the Photos tab **landed (5.9)** as `gallery-gtk`
   chrome (`years_from_structure` from existing month sections).
   Overlay on `photos_scroll`; hidden unless more than one year.
   Not a kit kind. Leftover GTK removal is still **5.9 owner**.

Exit:
- Every screen in `screens.toml` routed, each backed by a `gallery-ffi`
  structure/window (new ones included), not by `localgallery` UI-era
  queries.
- `cargo tree -d` clean for the pinned Gallery dependencies; Gallery
  core tests green.
- Screenshots of reference vs new at both sizes and schemes —
  `gtk-after/` is a written gap (mutter absent; no invented PNGs).
- Scroll measured on the 20k-photo tree (record jank; fix in this crate).
- Reference binary still builds.

### Phase 6 — Close-out

- ADR 0004:
  - add a "GTK design pass" section (what the kit owns, reuse evidence
    from the consumers that actually share bindings — do not claim
    three-app reuse for tiles or the mini-player);
  - apply the R10/R11 typeface amendment **only if Phase 5 landed the
    font**;
  - update Conformance: kit used by Contacts + Music, and by Gallery
    for the shared builders.
- ADR 0007: record L10's empty-state mapping only if it adds convention
  beyond R3. Everything else stays a shell decision and lives in
  `shells/shell-kit-gtk/README.md` (move §"Design language → GTK" there).
- Update `docs/IMPLEMENTATION-PLAN.md` in the same change whenever
  this pass changes inventory or sequence: leftover vs `gallery-gtk`,
  kit promotion / two-consumer evidence, token exceptions, deferred
  items (year rail **landed (5.9)**; leftover GTK removal still
  **5.9 owner**; Flush promotion still Music-only). The
  implementation plan is the living backlog; this file is the GTK
  design sequence. Findings do not live only here.
- `docs/screenshots/gtk-after/` is a **written gap**: mutter is
  absent; no after-shots were invented. README does not embed
  missing GTK shots.

## Order and dependencies

```
0 baseline/toolchain
  └► 1 tokens/accent/surfaces   (font held)
       └► 2a structure  ─► re-snapshot Contacts (kit-only bugs gone)
            └► 2b typed builders (existing rows)
                 └► 2c shared row polish (+ chart_row audit)
                      └► 3 contacts (spec settings fix; chips in-app)
                           ├► 4 music   (media row, Flush, mini-player in-app)
                           └► 5 gallery (spec → gallery-ffi windows → UI;
                                         promotes media row + chips; font)
                                3,4,5 ─► 6 close-out
```

Phase 5's promotions depend on Phase 3 (chips) and Phase 4 (media row)
having landed. If Gallery gets ahead, it builds its own versions and the
promotion happens when the later phase lands. Phases 3–5 can overlap
after 2c only if they do not grow the kit without a second consumer. A kit change found in 3, 4, or 5 goes back to the kit
and is re-snapshotted in every consumer.

## Open risks

- Ubuntu 26.04 D1 check (2026-09-16): resolute archive is libadwaita
  1.9.0 (26.04.1 images 1.9.1), not below 1.7. L6 fallbacks for
  `AdwToggleGroup` / `AdwWrapBox` are not required before Phase 2c / 5.
- Music dark accent text: best candidate is black at 4.67:1. The owner
  reviews it in Phase 1 snapshots (see Phase 1 table).
- Gallery screens without a `gallery-ffi` window (folders, collections,
  face review) turn Phase 5 into core work shared with iOS. Estimate it
  as such.
- Dependency resolution of `apps/gallery/core` under the `shells/`
  lockfile (pins, `gallery-ffi` crate types). **Checked in 5.3:** one
  `image` 0.25.10, one `uniffi` 0.32, no default-graph `ort`;
  `staticlib`/`cdylib` still usable as a rust dep (no `gallery-view`
  split).
- gtk4 0.8→0.11 and libadwaita 0.6→0.9 **landed**; Phase 2a structure
  **landed**; Phase 2b typed builders **landed**; Phase 2c row polish
  **landed**. Contacts `gtk-before/` baselines are in tree; further
  mutter captures stay optional. Contacts and Music kit stages landed.
  Phase 5 Gallery kit sequence 5.1–5.7 + 5.9 year rail / ADR+docs
  landed (spec, location windows, leftover freeze, shells pins, kit
  skeleton, remaining screens, promotions + font, Photos year rail).
  Named 5.8 leftovers and **5.10 leftover-parity GTK** keep Phase 5
  open. Leftover UI removal still needs an owner yes. `gtk-after/` is
  a gap (mutter absent).
- Headless mutter in CI may not be available. Snapshots stay a local
  script; unit tests must not require them.
- `GtkSectionModel` adapters, if Music needs Flush, stay in `music-gtk`
  until a second consumer exists.
- `pango::FontMap::add_font_file` (Phase 5 only). Fontconfig fallback
  is specified.
- Authored Gallery dark (D4) will look wrong if treated as a sourced
  companion later. The token README must keep calling it authored.
