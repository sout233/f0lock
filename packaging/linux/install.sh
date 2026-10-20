#!/bin/sh
set -eu

if [ "$#" -ne 0 ]; then
    printf '%s\n' 'Usage: ./install.sh (installs CLAP and VST3 for the current user)' >&2
    exit 1
fi

source_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
: "${HOME:?HOME must identify the user plugin directory}"

test -f "$source_dir/f0lock.clap"
test -f "$source_dir/f0lock.vst3/Contents/x86_64-linux/f0lock.so"

mkdir -p "$HOME/.clap" "$HOME/.vst3"
install -m 755 "$source_dir/f0lock.clap" "$HOME/.clap/f0lock.clap"
cp -R "$source_dir/f0lock.vst3" "$HOME/.vst3/"

printf '%s\n' "Installed $HOME/.clap/f0lock.clap" "Installed $HOME/.vst3/f0lock.vst3"
