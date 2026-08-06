#!/usr/bin/env bash
# Vendors fuml-runtime's dependencies as plain jar downloads — no Maven/Gradle installed in this
# environment (or assumed for anyone else's), and none of these need a build tool to fetch: every
# Maven Central artifact is a plain HTTPS file at a predictable URL. Run this once before
# build.sh. Exact versions were pinned during the ADR-005 spike (see memory/commit history) —
# the fUML RI needs the three JAXB-related jars added back because they shipped inside the JDK
# through Java 8 and were removed starting in Java 11.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p lib

MAVEN=https://repo1.maven.org/maven2
GRPC_VERSION=1.68.1
PROTOBUF_VERSION=4.29.3 # must match PROTOC_VERSION in build.sh — protoc X.Y generates code
# that requires protobuf-java runtime 4.X.Y exactly (confirmed directly: protoc 29.3 + a
# mismatched 4.28.2 runtime threw ProtobufRuntimeVersionException at class-init time).

fetch() {
  local path=$1
  local out="lib/$(basename "$path")"
  if [ -f "$out" ]; then
    echo "skip (already present): $out"
    return
  fi
  echo "fetching: $out"
  curl -sL -o "$out" "$MAVEN/$path"
}

# --- fUML Reference Implementation + its own runtime deps (ADR-005) ---
fetch "org/modeldriven/fuml/1.4.1/fuml-1.4.1.jar"
fetch "commons-collections/commons-collections/3.2/commons-collections-3.2.jar"
fetch "commons-lang/commons-lang/2.1/commons-lang-2.1.jar"
fetch "xerces/xercesImpl/2.11.0/xercesImpl-2.11.0.jar"
fetch "xml-apis/xml-apis/1.4.01/xml-apis-1.4.01.jar"
fetch "xalan/xalan/2.6.0/xalan-2.6.0.jar"
fetch "log4j/log4j/1.2.8/log4j-1.2.8.jar"
fetch "net/java/dev/stax-utils/stax-utils/20040917/stax-utils-20040917.jar"
fetch "javax/xml/stream/stax-api/1.0/stax-api-1.0.jar"
fetch "com/sun/xml/stream/sjsxp/1.0.1/sjsxp-1.0.1.jar"
fetch "commons-logging/commons-logging/1.1.1/commons-logging-1.1.1.jar"

# --- JDK 11+ compatibility: JAXB was removed from the JDK itself; these restore it as plain
# libraries (confirmed necessary and sufficient by the ADR-005 spike) ---
fetch "javax/xml/bind/jaxb-api/2.3.1/jaxb-api-2.3.1.jar"
fetch "org/glassfish/jaxb/jaxb-runtime/2.3.1/jaxb-runtime-2.3.1.jar"
fetch "com/sun/istack/istack-commons-runtime/3.0.7/istack-commons-runtime-3.0.7.jar"
fetch "javax/activation/javax.activation-api/1.2.0/javax.activation-api-1.2.0.jar"
# Generated protobuf/grpc code references @javax.annotation.Generated — also part of the JDK
# through Java 8, also removed starting in Java 11 (same story as JAXB above).
fetch "javax/annotation/javax.annotation-api/1.3.2/javax.annotation-api-1.3.2.jar"

# --- gRPC + protobuf runtime (this pass's own gRPC sidecar) ---
fetch "io/grpc/grpc-netty-shaded/$GRPC_VERSION/grpc-netty-shaded-$GRPC_VERSION.jar"
fetch "io/grpc/grpc-protobuf/$GRPC_VERSION/grpc-protobuf-$GRPC_VERSION.jar"
fetch "io/grpc/grpc-protobuf-lite/$GRPC_VERSION/grpc-protobuf-lite-$GRPC_VERSION.jar"
fetch "io/grpc/grpc-stub/$GRPC_VERSION/grpc-stub-$GRPC_VERSION.jar"
fetch "io/grpc/grpc-core/$GRPC_VERSION/grpc-core-$GRPC_VERSION.jar"
fetch "io/grpc/grpc-api/$GRPC_VERSION/grpc-api-$GRPC_VERSION.jar"
fetch "io/grpc/grpc-context/$GRPC_VERSION/grpc-context-$GRPC_VERSION.jar"
fetch "io/grpc/grpc-util/$GRPC_VERSION/grpc-util-$GRPC_VERSION.jar"
fetch "com/google/protobuf/protobuf-java/$PROTOBUF_VERSION/protobuf-java-$PROTOBUF_VERSION.jar"
fetch "com/google/guava/guava/33.3.1-jre/guava-33.3.1-jre.jar"
# Guava 27+ split its internal AtomicFutures support into this separate artifact — Guava's own
# jar references it but does not bundle it (confirmed directly: without it, the server throws
# NoClassDefFoundError on the very first incoming RPC, not at startup, since it's only touched
# once ListenableFuture internals are actually exercised).
fetch "com/google/guava/failureaccess/1.0.2/failureaccess-1.0.2.jar"
fetch "com/google/code/findbugs/jsr305/3.0.2/jsr305-3.0.2.jar"
fetch "com/google/errorprone/error_prone_annotations/2.28.0/error_prone_annotations-2.28.0.jar"
fetch "io/perfmark/perfmark-api/0.27.0/perfmark-api-0.27.0.jar"
fetch "org/codehaus/mojo/animal-sniffer-annotations/1.24/animal-sniffer-annotations-1.24.jar"
fetch "com/google/android/annotations/4.1.1.4/annotations-4.1.1.4.jar"
fetch "com/google/j2objc/j2objc-annotations/3.0.0/j2objc-annotations-3.0.0.jar"

echo "done — $(ls lib/*.jar | wc -l) jars in lib/"
