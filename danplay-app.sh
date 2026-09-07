#!/bin/sh
# Abre DanPlay. El nucleo Python lo arranca la propia app por socket Unix.
exec "$(dirname "$0")/desktop/src-tauri/target/release/danplay-app" "$@"
