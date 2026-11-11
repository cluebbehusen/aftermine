#!/bin/sh
# Every app target's Run Script phase: builds the Rust shell for the
# platform Xcode is building and puts the executable where Xcode signs and
# packages it. Xcode runs this with its build settings in the environment;
# see the shell README's iPad, Apple TV and Mac section for the commands
# that drive it.
set -eu

# Xcode's script environment does not include the user's shell setup.
PATH="$HOME/.cargo/bin:$PATH"
cd "$SRCROOT/.."

# Xcode exports the target platform's SDK for its own compiles. For an
# iPad or Apple TV build, Cargo build scripts run on the Mac and must not
# inherit that SDK; the device compiles find theirs themselves. A Mac build
# keeps it: Xcode's compiler finds no system headers without it. The
# deployment target stays exported so rustc and the C dependencies link for
# the same minimum OS.
case "$PLATFORM_NAME" in
    macosx)
        TARGET=aarch64-apple-darwin
        ;;
    iphonesimulator)
        TARGET=aarch64-apple-ios-sim
        unset SDKROOT
        ;;
    appletvos)
        TARGET=aarch64-apple-tvos
        unset SDKROOT
        ;;
    appletvsimulator)
        TARGET=aarch64-apple-tvos-sim
        unset SDKROOT
        ;;
    *)
        TARGET=aarch64-apple-ios
        unset SDKROOT
        ;;
esac

if [ "$CONFIGURATION" = "Release" ]; then
    cargo build -p aftermine-shell --target "$TARGET" --locked --release
    BUILT="target/$TARGET/release/Aftermine"
else
    cargo build -p aftermine-shell --target "$TARGET" --locked
    BUILT="target/$TARGET/debug/Aftermine"
fi

mkdir -p "$TARGET_BUILD_DIR/$EXECUTABLE_FOLDER_PATH"
cp "$BUILT" "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"

# The Resources phase, which runs before this one, copies all of assets/.
# The game reads only the atlas pages under sprites/, and the app icon comes
# from Assets.xcassets, so the loose sprite and icon PNGs, which the generator
# and tests read in the repository, leave the bundle.
ASSETS="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH/assets"
find "$ASSETS/sprites" -maxdepth 1 -name '*.png' ! -name 'atlas*.png' -delete
find "$ASSETS/icon" -maxdepth 1 -name '*.png' -delete

# The app wears the workspace version. App Store Connect requires every
# upload to carry a higher build number than the last, so the build number
# is the commit count.
VERSION="$(cargo pkgid -p aftermine-shell)"
VERSION="${VERSION##*[@#]}"
BUILD="$(git rev-list --count HEAD)"
/usr/libexec/PlistBuddy \
    -c "Set :CFBundleShortVersionString $VERSION" \
    -c "Set :CFBundleVersion $BUILD" \
    "$TARGET_BUILD_DIR/$INFOPLIST_PATH"

# The app's iCloud Drive folder, named Aftermine in Files and Finder. The
# entry is keyed by the container ID, and Xcode expands build settings only
# in Info.plist values, never in keys, so it is written here. Apple TV has
# no iCloud Drive.
case "$PLATFORM_NAME" in
    appletvos | appletvsimulator) ;;
    *)
        CONTAINER=":NSUbiquitousContainers:iCloud.$PRODUCT_BUNDLE_IDENTIFIER"
        /usr/libexec/PlistBuddy -c "Delete :NSUbiquitousContainers" \
            "$TARGET_BUILD_DIR/$INFOPLIST_PATH" 2> /dev/null || true
        /usr/libexec/PlistBuddy \
            -c "Add :NSUbiquitousContainers dict" \
            -c "Add $CONTAINER dict" \
            -c "Add $CONTAINER:NSUbiquitousContainerIsDocumentScopePublic bool true" \
            -c "Add $CONTAINER:NSUbiquitousContainerName string Aftermine" \
            -c "Add $CONTAINER:NSUbiquitousContainerSupportedFolderLevels string Any" \
            "$TARGET_BUILD_DIR/$INFOPLIST_PATH"
        ;;
esac

test -x "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
