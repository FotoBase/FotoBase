# Keyboard shortcuts

Every core flow is reachable from the keyboard (TASK-01-024, PLAN §2M
"Full keyboard control"). On macOS the modifier is **Cmd**; on Windows and
Linux it is **Ctrl**.

## Global

| Shortcut | Action |
| --- | --- |
| `Ctrl/Cmd + F` | Focus the live search box |
| `Ctrl/Cmd + Q` | Exit Fotobase |

## Library grid

| Shortcut | Action |
| --- | --- |
| `←` `→` `↑` `↓` | Move the selection to the neighboring photo (stops at the grid edges) |
| `Enter` | Open the selected photo in the viewer |
| double-click | Open the photo under the pointer in the viewer |

## Viewer

| Shortcut | Action |
| --- | --- |
| `←` `→` | Previous / next photo (pauses a running slideshow) |
| `Space` | Start / pause the slideshow |
| `F` | Toggle fullscreen |
| `I` | Toggle the info pane (EXIF + histogram) |
| `S` | Toggle the star (writes `xmp:Rating` = 5 to the sidecar) |
| `0` – `5` | Set the rating (only when Options → "Enable 1–5 ratings" is on; `0` clears) |
| `Esc` | Close the viewer |

## Implementation notes

- The shortcuts are plain Svelte actions (`app/src/lib/shortcuts.ts`); the
  matcher is a pure, unit-tested function and each layer (shell, grid,
  viewer) registers only the actions it owns.
- Theming is plain CSS variables keyed off a `data-theme` attribute on the
  document root (`app/src/lib/theme.ts` + `app/src/app.css`); `system`
  follows the OS via `prefers-color-scheme`, and the Options → Theme
  selector can force light or dark.
- Thumbnails are HiDPI-aware: on displays with `devicePixelRatio > 1` the
  grid requests a 2x variant (`thumb://<hash>?s=2`, 1024px max edge) through
  an `srcset`, so cells stay sharp up to the maximum zoom on Retina/4K.
