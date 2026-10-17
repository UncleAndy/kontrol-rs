#!/bin/sh
# Stop the service while the files it runs from are still there, whatever state
# the package manager leaves the rest in.
set -e

if command -v systemctl >/dev/null 2>&1; then
    systemctl stop komplement >/dev/null 2>&1 || true
    systemctl --user stop komplement >/dev/null 2>&1 || true
fi
