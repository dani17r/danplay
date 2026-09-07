#!/bin/sh
exec "$(dirname "$0")/.venv/bin/python" -m danplay.cli "$@"
