#!/usr/bin/env bash
# Builds fuml-runtime with plain javac + a manually-fetched protoc/grpc-java plugin — no
# Maven/Gradle installed in this environment. See fetch-deps.sh for the jar dependencies and the
# ADR-005 spike notes (memory) for why each one is needed.
set -euo pipefail
cd "$(dirname "$0")"

./fetch-deps.sh

mkdir -p tools generated out

PROTOC_VERSION=29.3
GRPC_JAVA_VERSION=1.68.1
# `GRPC_PLUGIN_EXT` is the *local* binary's filename suffix only (Windows needs `.exe` to be
# directly executable; Linux/Mac don't care either way). It is NOT the download URL's extension —
# confirmed directly (a plain 404 on Maven Central without it): `protoc-gen-grpc-java` is published
# with a literal `.exe` suffix in its filename for *every* classifier, Linux and Mac included (a
# grpc-java packaging quirk, unrelated to the target OS). This only ever surfaced running this
# script inside a Linux container — every local run on this project's Windows dev machines took
# the already-`.exe`-suffixed Windows branch and never hit the bug.
GRPC_PLUGIN_URL_EXT=".exe"
case "$(uname -s)" in
  Linux*)   PROTOC_PLATFORM=linux-x86_64;  GRPC_PLUGIN_CLASSIFIER=linux-x86_64;  GRPC_PLUGIN_EXT="" ;;
  Darwin*)  PROTOC_PLATFORM=osx-x86_64;    GRPC_PLUGIN_CLASSIFIER=osx-x86_64;    GRPC_PLUGIN_EXT="" ;;
  MINGW*|MSYS*|CYGWIN*) PROTOC_PLATFORM=win64; GRPC_PLUGIN_CLASSIFIER=windows-x86_64; GRPC_PLUGIN_EXT=".exe" ;;
  *) echo "unsupported platform: $(uname -s)" >&2; exit 1 ;;
esac

PROTOC_BIN="tools/protoc$GRPC_PLUGIN_EXT"
GRPC_PLUGIN_BIN="tools/protoc-gen-grpc-java$GRPC_PLUGIN_EXT"

if [ ! -f "$PROTOC_BIN" ]; then
  echo "fetching protoc ($PROTOC_PLATFORM)..."
  curl -sSLf -o tools/protoc.zip \
    "https://github.com/protocolbuffers/protobuf/releases/download/v$PROTOC_VERSION/protoc-$PROTOC_VERSION-$PROTOC_PLATFORM.zip"
  unzip -o -q tools/protoc.zip -d tools/protoc-extracted
  cp "tools/protoc-extracted/bin/protoc$GRPC_PLUGIN_EXT" "$PROTOC_BIN"
  chmod +x "$PROTOC_BIN"
fi

if [ ! -f "$GRPC_PLUGIN_BIN" ]; then
  echo "fetching protoc-gen-grpc-java ($GRPC_PLUGIN_CLASSIFIER)..."
  # `-f`/`--fail`: without it, curl exits 0 on a 404 and happily writes the HTML error page as
  # the plugin binary — confirmed directly, that's exactly how this failed before (a confusing
  # "program not found or is not executable" error out of protoc, tens of lines later, instead of
  # a clear download failure right here).
  curl -sSLf -o "$GRPC_PLUGIN_BIN" \
    "https://repo1.maven.org/maven2/io/grpc/protoc-gen-grpc-java/$GRPC_JAVA_VERSION/protoc-gen-grpc-java-$GRPC_JAVA_VERSION-$GRPC_PLUGIN_CLASSIFIER$GRPC_PLUGIN_URL_EXT"
  chmod +x "$GRPC_PLUGIN_BIN"
fi

echo "generating gRPC stubs..."
"$PROTOC_BIN" \
  --plugin=protoc-gen-grpc-java="$GRPC_PLUGIN_BIN" \
  --java_out=generated --grpc-java_out=generated \
  --proto_path=proto \
  proto/fuml_runtime.proto

echo "compiling..."
javac -cp "lib/*" -d out $(find generated src vendor-src -name "*.java")

echo "build complete — run.sh to start the server"
