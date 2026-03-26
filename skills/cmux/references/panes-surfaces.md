# Panes and Surfaces

Split layout, surface creation, focus, move, and reorder.

## Inspect

```bash
cmux pane list
cmux pane surfaces
cmux surface list
cmux surface current
```

## Create Surfaces (Tabs)

```bash
cmux surface create                          # new terminal tab in focused pane
cmux surface create --type browser           # new browser tab
cmux surface create --json                   # returns { result: { panel_id: "uuid" } }
```

## Create Splits

```bash
cmux surface split --orientation horizontal  # split focused pane horizontally
cmux surface split --orientation vertical    # split focused pane vertically
cmux pane new --orientation horizontal       # same as above (canonical form)
```

## Send Text / Keys to Terminal

```bash
cmux surface send-text 'echo hello\n'                     # to focused surface
cmux surface send-text --surface <id> 'npm run dev\n'     # to specific surface
cmux surface send-key c --mods ctrl --surface <id>         # KEY is positional, --mods for modifiers
```

## Read Terminal Output

```bash
cmux surface read-screen                     # read focused surface
cmux surface read-screen --surface <id>      # read specific surface
```

## Focus and Close

```bash
cmux surface focus <id>                       # positional UUID
cmux pane focus <id>                          # positional UUID
cmux pane focus-direction right               # positional direction (left, right, up, down)
cmux pane last                                # switch to previously focused pane
cmux surface close <id>                       # positional UUID (optional, closes focused if omitted)
cmux pane close <id>                          # positional UUID (optional, closes focused if omitted)
```

## Move/Reorder Surfaces

```bash
cmux surface move --panel <id> --workspace <ws_id>   # uses --panel (not --surface)
cmux surface reorder <INDEX> --panel <id>             # INDEX is positional (0-based tab position)
cmux surface drag-to-split right --surface <id>       # DIRECTION is positional (left, right, up, down)
```

Surface identity is stable across move/reorder operations.

## Pane Layout

```bash
cmux pane resize 0.05 --panel <id>           # AMOUNT is positional (-0.05 to shrink, 0.05 to grow)
cmux pane equalize                           # equalize all splits
cmux pane swap <A> <B>                       # two positional UUIDs
cmux pane break                              # break pane into new workspace
cmux pane join <id>                          # positional UUID
```

## Surface Actions

```bash
cmux surface action toggle_zoom
cmux surface action clear_screen
cmux surface action refresh
cmux surface action flash
cmux surface refresh --surface <id>
cmux surface clear-history --surface <id>
```

## Tab Actions

```bash
cmux tab action rename --title "My Tab"
cmux tab action duplicate
cmux tab action pin
cmux tab action unpin
cmux tab action close_left
cmux tab action close_right
cmux tab action close_others
cmux tab action mark_read
cmux tab action mark_unread
```
