#!/bin/sh
set -eu

if [ "$#" -lt 1 ]; then
    echo 'Usage: launch-appimage-linux.sh /path/to/QuickConfiguration.AppImage [args...]' >&2
    exit 2
fi

appimage=$1
shift

unset APPIMAGE APPDIR ARGV0 OWD LD_LIBRARY_PATH REDIRECT_APPIMAGE TARGET_APPIMAGE ELECTRON_RUN_AS_NODE

if [ -e /proc/driver/nvidia/version ] && [ -z "${WEBKIT_DISABLE_DMABUF_RENDERER+x}" ]; then
    export WEBKIT_DISABLE_DMABUF_RENDERER=1
fi

exec "$appimage" "$@"
