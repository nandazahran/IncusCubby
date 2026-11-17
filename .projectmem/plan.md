# IncusCubby — plan

> Editable **intent** file: ideas + plans — what we *mean to do*.
> This is NOT the event log. `events.jsonl` -> `summary.md` records what
> *happened*; this file records what we *intend*. The AI reads it at
> session start and edits it directly (like `PROJECT_MAP.md`): add ideas
> and plans, check items off, move done work down to Shipped. Plans are
> never logged as events.

## Ideas
_Loose thoughts, not yet committed to._

## Active plans
- [x] Scaffold Cargo project (Rust 2024, Slint, Tokio, hyperlocal)
- [x] Implement Incus 1.0 REST Unix socket client (`GET /1.0/instances?recursion=2`, create, state, delete, snapshots)
- [x] Build Slint standard-widget UI and background Tokio worker with `upgrade_in_event_loop`
- [x] Implement terminal shell (`incus exec <name> -- su -l root`) and IDE (`codium`/`zed`) launchers
- [x] Live state polling (3s interval), host workspace bind-mounts (`shift: "true"`), custom image/CPU creation options, inline delete/restore confirmations

## Next
- Custom visual styling / theming (explicitly deferred)
- Interactive `zeditor` fallback for Arch/CachyOS systems if `zed` is absent

## Someday / maybe

## Shipped
_Move completed plans here so the top stays about the future._
