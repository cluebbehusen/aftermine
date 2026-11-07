# aftermine-icloud

`aftermine-icloud` is the shell's one door to iCloud. On iPad and Mac it finds
the app's iCloud Drive folder, coordinates file access so iCloud never syncs a
half-written save, and starts downloads for files another device wrote. On iPad,
Mac and Apple TV it reads and writes iCloud's key-value store, which carries the
settings that follow the player. Saves stay ordinary files; which folder each
one lives in is the shell's decision, in `shell/src/paths.rs`, and which
settings sync is the shell's decision, in `shell/src/config/sync.rs`.

## Main pieces

- `documents` looks up the folder Files and Finder show under the app's name, or
  answers that iCloud Drive is unavailable.
- `coordinate` runs a read, a whole-file replace or a delete under
  `NSFileCoordinator`.
- `download` starts downloading what iCloud holds but this device does not, and
  keeps iCloud's choice of copy when two devices changed a file at once.
- `KeyValueStore` reads and writes strings in iCloud's key-value store and
  counts the changes that reach it from other devices.
- `src/icloud.m` holds the iCloud Drive calls and `src/key_value.m` the
  key-value store calls. Every iCloud Drive call may wait on iCloud, so callers
  keep those off the main thread; the key-value store answers from memory.

## Boundaries

This is the only workspace crate that allows `unsafe` code, confined to the
modules that call into Objective-C. Apple TV has no iCloud Drive, so there, as
on other platforms, `documents` answers `None`, `coordinate` runs its access
directly and `download` has nothing to fetch. Off Apple platforms there is no
key-value store either.

An app needs the iCloud Documents entitlement for its container and the
`com.apple.developer.ubiquity-kvstore-identifier` entitlement for the key-value
store; without them, as in tests and unsigned builds, there is no folder and the
store keeps its values on the device. The iPad, Mac and Apple TV builds must
carry the same key-value store identifier, `<team ID>.<bundle ID>`
(`dev.luebbehusen.aftermine` for the maintainer's builds), or their settings
land in separate stores. Xcode writes it as
`$(TeamIdentifierPrefix)$(CFBundleIdentifier)`.

## Development

```sh
cargo test -p aftermine-icloud --locked
cargo clippy -p aftermine-icloud --target aarch64-apple-ios --locked -- -D warnings
cargo clippy -p aftermine-icloud --target aarch64-apple-tvos --locked -- -D warnings
```
