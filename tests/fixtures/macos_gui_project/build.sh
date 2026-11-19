#!/bin/sh
# Builds "Omnipackage Hello.app" (universal, ad-hoc signed) into a .dmg. Needs only the Xcode Command Line Tools.
# omnipackage runs it in a staged copy of the project with OMNIPACKAGE_OUTPUT_DIR and OMNIPACKAGE_VERSION set.
set -eu

VERSION=${OMNIPACKAGE_VERSION:-$(cat version.txt)}
OUT=${OMNIPACKAGE_OUTPUT_DIR:-$PWD/dist}
APP="Omnipackage Hello.app"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/stage/$APP/Contents/MacOS" "$OUT"
for arch in arm64 x86_64; do
  swiftc -O -target "$arch-apple-macos11" -o "$WORK/bin-$arch" main.swift
done
lipo -create -output "$WORK/stage/$APP/Contents/MacOS/omnipackage-macos-hello-gui" "$WORK/bin-arm64" "$WORK/bin-x86_64"
sed "s/@VERSION@/$VERSION/g" Info.plist.in > "$WORK/stage/$APP/Contents/Info.plist"
codesign --force --deep --sign - "$WORK/stage/$APP"
ln -s /Applications "$WORK/stage/Applications"
hdiutil create -volname "Omnipackage Hello" -srcfolder "$WORK/stage" -ov -format UDZO "$OUT/omnipackage-macos-hello-gui-$VERSION.dmg"
