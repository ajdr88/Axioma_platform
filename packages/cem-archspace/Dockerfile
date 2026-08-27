# docs/IMPLEMENTATION_KICKOFF.md Phase 2 (ADR-011). Single-stage, unlike fuml-runtime's
# build/runtime split — Python needs no separate compile step, `pip install` + a `protoc` codegen
# run at build time is the whole build.
FROM python:3.12-slim AS runtime
WORKDIR /app
COPY requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt
COPY proto ./proto
COPY src ./src
RUN python -m grpc_tools.protoc -I proto --python_out=src --grpc_python_out=src proto/cem_archspace.proto
EXPOSE 50052
ENV ARCHSPACE_PORT=50052
WORKDIR /app/src
CMD ["python", "server.py"]
