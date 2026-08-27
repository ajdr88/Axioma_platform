#!/usr/bin/env bash
# Local (non-Docker) dev startup — mirrors packages/fuml-runtime/run.sh's role. Assumes a venv
# already exists at .venv with requirements.txt installed and stubs generated (see README.md).
set -euo pipefail
cd "$(dirname "$0")"
source .venv/Scripts/activate 2>/dev/null || source .venv/bin/activate
cd src
python server.py
