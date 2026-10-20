#!/usr/bin/env bash
# User systemd unit. --boot enables lingering for startup before login.
set -euo pipefail
role=serve
binary=$(command -v tdssh || true)
state_dir="${XDG_DATA_HOME:-$HOME/.local/share}/tdssh"
peer=''
listen=127.0.0.1:2222
target=127.0.0.1:22
ticket=''
interface=''
force=0
boot=0
dry=0
remove=0
relays=()
while (($#)); do
    case "$1" in
        --binary) binary=$2; shift 2;;
        --state-dir) state_dir=$2; shift 2;;
        --role) role=$2; shift 2;;
        --peer) peer=$2; shift 2;;
        --listen) listen=$2; shift 2;;
        --target) target=$2; shift 2;;
        --ticket-file) ticket=$2; shift 2;;
        --relay) relays+=(--relay "$2"); shift 2;;
        --interface) interface=$2; force=1; shift 2;;
        --force-direct) force=1; shift;;
        --boot) boot=1; shift;;
        --dry-run) dry=1; shift;;
        --uninstall) remove=1; shift;;
        *) echo "Unknown option: $1" >&2; exit 2;;
    esac
done
unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
unit="$unit_dir/tdssh.service"
if ((remove)); then
    if ((dry)); then printf 'Would disable and remove %s\n' "$unit"; exit; fi
    systemctl --user disable --now tdssh.service
    rm -f -- "$unit"
    systemctl --user daemon-reload
    echo 'Removed tdssh.service; identity and SSH settings retained.'
    exit
fi
[[ "$role" == serve || "$role" == forward ]] || { echo '--role must be serve or forward' >&2; exit 2; }
[[ -n "$binary" && -x "$binary" ]] || { echo 'Pass --binary /absolute/path/to/tdssh' >&2; exit 2; }
binary=$(realpath -- "$binary")
state_dir=$(realpath -m -- "$state_dir")
args=("$binary" --state-dir "$state_dir")
((force)) && args+=(--force-direct)
[[ -z "$interface" ]] || args+=(--interface "$interface")
args+=("${relays[@]}" "$role")
if [[ "$role" == serve ]]; then
    args+=(--target "$target")
else
    [[ -n "$peer" ]] || { echo '--peer is required for forward' >&2; exit 2; }
    args+=(--peer "$peer" --listen "$listen")
    [[ -z "$ticket" ]] || args+=(--ticket-file "$(realpath -- "$ticket")")
fi
# Escape systemd quoting, specifier expansion and environment expansion.
exec_line=$(python3 - "${args[@]}" <<'PY'
import sys

def quote(s):
    if '\n' in s or '\r' in s or '\0' in s:
        raise SystemExit('Newlines and NUL are not allowed in unit arguments')
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"').replace('%', '%%').replace('$', '$$') + '"'
print(' '.join(map(quote, sys.argv[1:])))
PY
)
content="[Unit]
Description=tdssh encrypted cross-NAT SSH $role
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=$exec_line
Restart=on-failure
RestartSec=10
UMask=0077

[Install]
WantedBy=default.target"
if ((dry)); then printf '%s\n' "$content"; exit; fi
"$binary" --state-dir "$state_dir" init
install -d -m 700 -- "$unit_dir"
printf '%s\n' "$content" > "$unit"
chmod 600 -- "$unit"
if ((boot)); then loginctl enable-linger "$(id -un)"; fi
systemctl --user daemon-reload
systemctl --user enable --now tdssh.service
systemctl --user --no-pager status tdssh.service
