# Windows and Workspaces

Window/workspace lifecycle and ordering operations.

## Inspect

```bash
cmux window list
cmux window current
cmux workspace list
cmux workspace current
```

## Create/Focus/Close

```bash
cmux window new

cmux workspace new
cmux workspace select 3                      # select by 0-based index (positional arg)
cmux workspace close 3                       # close by 0-based index (closes selected if omitted)
cmux workspace next
cmux workspace previous
cmux workspace last
cmux workspace latest-unread
```

## Rename and Status

```bash
cmux workspace rename "my-workspace"                      # title is positional arg
cmux workspace set-status --key build --value "passing"   # --key and --value are flags (correct)
cmux workspace clear-status
cmux workspace set-progress 0.75                          # value is positional (0.0 to 1.0)
cmux workspace clear-progress
cmux workspace report-git main                            # branch is positional (optional: --dirty)
cmux workspace report-pr open --url "https://github.com/..."  # status is positional (open, merged, closed, draft)
```

## Reorder and Move

```bash
cmux workspace reorder 3 1                    # reorder workspace at index 3 to index 1 (positional: <FROM> <TO>)
```

## Logging

```bash
cmux workspace log "Build started"                       # message is positional (optional: --level, --source)
cmux workspace list-log
cmux workspace clear-log
```
