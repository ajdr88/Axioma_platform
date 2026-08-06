# P1.4 (roadmap: Behavioral Simulation, ADR-005/008). Builds via the same fetch-deps.sh/build.sh
# used for local dev — no Maven/Gradle anywhere in this repo (see fetch-deps.sh's own comment).
# The ADR-005 spike proved the fUML RI runs cleanly on JDK 22, so the runtime stage needs no
# legacy Java 8 base image.
FROM eclipse-temurin:22-jdk AS build
RUN apt-get update && apt-get install -y --no-install-recommends curl unzip \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY proto ./proto
COPY src ./src
COPY vendor-src ./vendor-src
COPY fetch-deps.sh build.sh ./
RUN chmod +x fetch-deps.sh build.sh && ./build.sh

FROM eclipse-temurin:22-jre AS runtime
WORKDIR /app
COPY --from=build /build/out ./out
COPY --from=build /build/lib ./lib
EXPOSE 50051
ENV FUML_RUNTIME_PORT=50051
CMD ["java", "-cp", "out:lib/*", "org.axioma.fumlruntime.FumlRuntimeServer"]
