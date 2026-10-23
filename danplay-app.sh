#!/bin/sh
# Abre DanPlay (la compilada con ./scripts/build.sh). El nucleo va dentro.
exec "$(dirname "$0")/target/release/danplay-app" "$@"
