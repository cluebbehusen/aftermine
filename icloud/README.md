# aftermine-icloud

`aftermine-icloud` is the shell's one door to iCloud. On iPad and Mac it finds
the app's iCloud Drive folder, coordinates file access so iCloud never syncs a
half-written save, and starts downloads for files another device wrote. On iPad,
Mac and Apple TV it reads and writes iCloud's key-value store, which carries the
settings that follow the player. It also reads and writes the app's user
defaults, the only local storage Apple TV never clears, which is where the shell
keeps its settings on Apple TV. On iPad and Apple TV it keeps the screen on
while the shell asks and holds background time, so a save or an iCloud pass
finishes after the player leaves the app. Saves stay ordinary files; which
folder each one lives in is the shell's decision, in `shell/src/paths.rs`, and
which settings sync is the shell's decision, in `shell/src/config/sync.rs`.

## Main pieces

- `documents` looks up the folder Files and Finder show under the app's name, or
  answers that iCloud Drive is unavailable.
- `coordinate` runs a read, a whole-file replace or a delete under
  `NSFileCoordinator`.
- `download` starts downloading what iCloud holds but this device does not, and
  keeps iCloud's choice of copy when two devices changed a file at once.
- `KeyValueStore` reads and writes strings in iCloud's key-value store and
  counts the changes that reach it from other devices.
- `cloudkit` mirrors record files through one zone of CloudKit's private
  database: it names the account, fetches what changed since a token, and saves
  files and delete records. Every call waits on CloudKit, so callers keep them
  off the main thread.
- `defaults` reads and writes strings in the app's user defaults, which stay on
  the device.
- `application` turns the idle timer off and on, and `BackgroundTask` holds
  background time until dropped; the system takes the time back first if it runs
  out.
- `src/icloud.m` holds the iCloud Drive calls, `src/key_value.m` the key-value
  store calls, `src/cloudkit.m` the CloudKit calls, `src/defaults.m` the user
  defaults calls and `src/application.m` the `UIApplication` calls. Every iCloud
  Drive call may wait on iCloud, so callers keep those off the main thread; the
  key-value store answers from memory.

## Boundaries

This is the only workspace crate that allows `unsafe` code, confined to the
modules that call into Objective-C. Apple TV has no iCloud Drive, so there, as
on other platforms, `documents` answers `None`, `coordinate` runs its access
directly and `download` has nothing to fetch. Off Apple platforms there is no
key-value store, no CloudKit and no user defaults either. Only iPad and Apple TV
have an idle timer and background time; elsewhere `application` does nothing.

An app needs the iCloud Documents entitlement for its container and the
`com.apple.developer.ubiquity-kvstore-identifier` entitlement for the key-value
store, and the CloudKit service in the same container for the mirror; without
them, as in tests and unsigned builds, there is no folder, the store keeps its
values on the device, and CloudKit answers that there is no account. The iPad,
Mac and Apple TV builds must carry the same key-value store identifier,
`<team ID>.<bundle ID>` (`dev.luebbehusen.aftermine` for the maintainer's
builds), or their settings land in separate stores. Xcode writes it as
`$(TeamIdentifierPrefix)$(CFBundleIdentifier)`.

## Development

```sh
cargo test -p aftermine-icloud --locked
cargo clippy -p aftermine-icloud --target aarch64-apple-ios --locked -- -D warnings
cargo clippy -p aftermine-icloud --target aarch64-apple-tvos --locked -- -D warnings
```
