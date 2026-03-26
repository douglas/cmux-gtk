# Trigger Flash and Surface Health

Operational checks useful in automation loops.

## Trigger Flash

Flash a surface to provide visual confirmation in UI:

```bash
cmux surface flash                           # flash focused surface
cmux surface flash --surface <id>            # flash specific surface
```

## Surface Health

Use health output to detect hidden/detached/non-windowed surfaces:

```bash
cmux surface health                          # check focused surface
cmux surface health --surface <id>           # check specific surface
```

Use this before routing focused input if UI state may be stale.
