#!/bin/sh
# Abre DanPlay. El nucleo Python lo arranca la propia app por socket Unix.
exec "$(dirname "$0")/target/release/danplay-app" "$@"
