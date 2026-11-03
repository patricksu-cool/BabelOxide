#!/bin/bash
# Finder opens this in Terminal; only this child process gets these tool paths.
set -u
project=$(cd -P -- "${BASH_SOURCE[0]%/*}" && pwd -P) || exit 1
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export PYTHONUTF8=1
result=0
if ! command -v uv >/dev/null 2>&1; then
    printf '%s\n' '[ERROR] uv is required. Install uv, then run this launcher again.' >&2
    result=1
else
    uv run --locked --no-dev --python 3.13.16 --project "$project" python -B "$project/script/dev.py" "$@" || result=$?
fi
if [ "$result" -ne 0 ]; then
    printf 'Development launch failed. Exit code: %s\n' "$result" >&2
fi
if [ "$#" -eq 0 ] && [ -t 0 ]; then
    printf '\nPress Return to close. '
    IFS= read -r reply || :
fi
exit "$result"
