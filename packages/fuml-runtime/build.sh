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
  curl -sL -o tools/protoc.zip \
    "https://github.com/protocolbuffers/protobuf/releases/download/v$PROTOC_VERSION/protoc-$PROTOC_VERSION-$PROTOC_PLATFORM.zip"
  unzip -o -q tools/protoc.zip -d tools/protoc-extracted
  cp "tools/protoc-extracted/bin/protoc$GRPC_PLUGIN_EXT" "$PROTOC_BIN"
  chmod +x "$PROTOC_BIN"
fi

if [ ! -f "$GRPC_PLUGIN_BIN" ]; then
  echo "fetching protoc-gen-grpc-java ($GRPC_PLUGIN_CLASSIFIER)..."
  curl -sL -o "$GRPC_PLUGIN_BIN" \
    "https://repo1.maven.org/maven2/io/grpc/protoc-gen-grpc-java/$GRPC_JAVA_VERSION/protoc-gen-grpc-java-$GRPC_JAVA_VERSION-$GRPC_PLUGIN_CLASSIFIER$GRPC_PLUGIN_EXT"
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
