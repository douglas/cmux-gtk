# Handles and Identify

Use `identify` and short handles for deterministic automation targeting.

## Handle Inputs

Most v2-backed commands accept:
- UUID
- short ref (`window:N`, `workspace:N`, `pane:N`, `surface:N`)
- index (where legacy/index-based commands still allow it)

## Self Identify

```bash
cmux identify --json
```

Returns current focused topology plus optional caller resolution.

## Output

```bash
cmux identify --json                 # JSON output with server info
```

Note: `identify` only reports server platform/version. It has no `--workspace` or `--id-format` options.
Use `cmux workspace current` and `cmux surface current` for topology context.
