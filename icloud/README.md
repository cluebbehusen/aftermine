# aftermine-icloud

`aftermine-icloud` is the shell's one door to iCloud Drive on iPad and Mac. It
finds the app's iCloud Drive folder, coordinates file access so iCloud never
syncs a half-written save, and starts downloads for files another device wrote.
Saves stay ordinary files; which folder each one lives in is the shell's
decision, in `shell/src/paths.rs`.

## Main pieces

- `documents` looks up the folder Files and Finder show under the app's name, or
  answers that iCloud Drive is unavailable.
- `coordinate` runs a read, a whole-file replace or a delete under
  `NSFileCoordinator`.
- `download` starts downloading what iCloud holds but this device does not, and
  keeps iCloud's choice of copy when two devices changed a file at once.
- `src/icloud.m` holds the Foundation calls. Every call may wait on iCloud, so
  callers stay off the main thread.

## Boundaries

This is the only workspace crate that allows `unsafe` code, confined to the
module that calls into Objective-C. Elsewhere, and on Apple TV, which has no
iCloud Drive, `documents` answers `None`, `coordinate` runs its access directly
and `download` has nothing to fetch. An app needs the iCloud Documents
entitlement for its container; without it, as in tests and unsigned builds,
there is no folder.

## Development

```sh
cargo test -p aftermine-icloud --locked
cargo clippy -p aftermine-icloud --target aarch64-apple-ios --locked -- -D warnings
```
