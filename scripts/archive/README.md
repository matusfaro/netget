# Archived one-off scripts

One-shot migration and fixup scripts kept for historical reference. Each was written
for a single sweep (adding mocks, migrating event types, fixing parameter definitions,
etc.) that has long since landed; none of them are safe to re-run against the current
tree. The maintained build wrapper is `./cargo-isolated.sh` at the repository root
(which delegates to `./cargo.sh` — both stay in the root).
