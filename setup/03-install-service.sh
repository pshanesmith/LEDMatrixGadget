#!/usr/bin/env bash
# Deploy the display from GitHub into /opt/matrix and run it as a systemd service.
#
#   ./setup/03-install-service.sh                 dry run (default)
#   sudo ./setup/03-install-service.sh --apply    deploy, install, enable and start
# Run it again later (from /opt/matrix/setup or from a checkout) to UPDATE: it pulls, reinstalls, restarts.
#
# Options: --repo URL    GitHub clone URL (default: the 'origin' of the checkout you run this from)
#          --ref NAME    branch or tag to deploy (default: main)
#          --binary PATH install a prebuilt matrix-display (from tools/cross-build.sh) instead of compiling here
#          --rebuild     compile from scratch instead of reusing the previous build
#          --no-start    install and enable but do not start now
#          --no-drop     keep running as root (skip the privilege drop)
#          --uninstall   stop and remove the service (keeps /opt/matrix, /etc/matrix and the state dir)
#          --yes         skip confirmation prompts
# Layout:  /opt/matrix        clone of the repository, with target/ (the build) and bin/ (the installed programs)
#                             inside (root-owned, world-readable)
#          /etc/matrix        config.toml, secrets.env (0600), deploy_key (0600, read-only deploy key)
#          /var/lib/matrix-display   weather / Home Assistant cache
# For a PRIVATE repository use an SSH URL (git@github.com:USER/REPO.git): the script creates a deploy key and
# shows the public half to add under GitHub > repository > Settings > Deploy keys (leave write access off).
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SELF_REPO=$(dirname "$HERE")
# shellcheck source=lib/common.sh
source "$HERE/lib/common.sh"

main() {
  local DEPLOY=${MATRIX_DEPLOY_DIR:-/opt/matrix} REF=${MATRIX_REF:-main} URL=${MATRIX_REPO_URL:-}
  local ETC=${ETC_DIR:-/etc/matrix} STATE=${STATE_DIR_PATH:-/var/lib/matrix-display}
  local UNIT=/etc/systemd/system/matrix-display.service KEY
  local START=1 UNINSTALL=0 REBUILD=0 NODROP=0 BINARY=
  ASSUME_YES=0
  while (( $# )); do
    case $1 in
      --apply) DRY_RUN=0 ;;
      --repo) URL=$2; shift ;;
      --ref) REF=$2; shift ;;
      --binary) BINARY=$2; shift ;;
      --rebuild) REBUILD=1 ;;
      --no-start) START=0 ;;
      --no-drop) NODROP=1 ;;
      --uninstall) UNINSTALL=1 ;;
      --yes) ASSUME_YES=1 ;;
      -h|--help) sed -n '2,21p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; return 0 ;;
      *) die "unknown option: $1 (try --help)" ;;
    esac
    shift
  done
  KEY=$ETC/deploy_key
  local BIN=$DEPLOY/bin/matrix-display TARGET_DIR=${CARGO_TARGET_DIR:-$DEPLOY/target}

  require_pi
  require_root

  if (( UNINSTALL )); then
    step "Uninstall"
    run systemctl disable --now matrix-health.timer matrix-display.service
    run rm -f "$UNIT" /etc/systemd/system/matrix-health.service /etc/systemd/system/matrix-health.timer
    run systemctl daemon-reload
    info "removed the service; $DEPLOY, $ETC and $STATE were kept"
    return 0
  fi

  step "Source"
  if [[ -z $URL ]]; then
    URL=$(git -C "$SELF_REPO" remote get-url origin 2>/dev/null || true)
    [[ -n $URL ]] || die "no repository URL: pass --repo git@github.com:USER/LEDMatrixGadget.git (or set a git 'origin')"
  fi
  echo "  repository: $URL   ref: $REF   target: $DEPLOY"
  if [[ $URL == git@* || $URL == ssh://* ]]; then
    if [[ ! -f $KEY ]]; then
      echo "  creating a deploy key at $KEY"
      run install -d -m 755 "$ETC"
      run ssh-keygen -q -t ed25519 -N "" -C "matrix-pi deploy key" -f "$KEY"
      if (( ! DRY_RUN )); then
        echo; echo "  Add this PUBLIC key in GitHub: repository > Settings > Deploy keys > Add deploy key"
        echo "  (title 'matrix-pi', leave 'Allow write access' OFF):"; echo; cat "$KEY.pub"; echo
        if (( ! ASSUME_YES )); then read -r -p "  Press Enter once the key is added: " _; fi
      fi
    fi
    export GIT_SSH_COMMAND="ssh -i $KEY -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=$ETC/known_hosts"
  fi

  step "Deploy to $DEPLOY"
  if [[ -d $DEPLOY/.git ]]; then
    run git -C "$DEPLOY" remote set-url origin "$URL"
    run git -C "$DEPLOY" fetch --tags --prune origin
    run git -C "$DEPLOY" checkout -q "$REF"
    if (( DRY_RUN )) || git -C "$DEPLOY" rev-parse -q --verify "origin/$REF" >/dev/null; then
      run git -C "$DEPLOY" merge --ff-only "origin/$REF"
    fi
  else
    run git clone --branch "$REF" "$URL" "$DEPLOY"
  fi
  run chown -R root:root "$DEPLOY"
  run chmod -R go+rX "$DEPLOY"

  step "Build and install the program"
  # The panel driver (the pure Rust rpi-led-panel crate) is compiled into the binary; nothing else is
  # installed system-wide. A source build needs about 1 GB of RAM plus swap and
  # 15+ minutes on a Zero 2 W: tools/cross-build.sh on another machine is much faster; pass its output with --binary.
  local built=$TARGET_DIR/pi/matrix-display
  if [[ -n $BINARY ]]; then
    [[ -x $BINARY ]] || die "--binary $BINARY is not an executable file"
    echo "  using the prebuilt binary $BINARY"
    built=$BINARY
  else
    if ! command -v cargo >/dev/null; then run "$DEPLOY/setup/02-matrix-setup.sh" --apply --yes --only deps; fi
    if (( REBUILD )); then run cargo clean --manifest-path "$DEPLOY/Cargo.toml" --target-dir "$TARGET_DIR" --profile pi; fi
    run nice -n 10 env CARGO_TARGET_DIR="$TARGET_DIR" cargo build --manifest-path "$DEPLOY/Cargo.toml" \
        --profile pi --features hardware --locked --bins -j1
  fi
  run install -D -m 755 "$built" "$BIN"
  # The helper programs sit next to the display binary (the build directory, or --binary's directory).
  local tool
  for tool in matrix-hello matrix-geocode matrix-set-config matrix-ha-discover; do
    if [[ -x $(dirname "$built")/$tool ]]; then run install -D -m 755 "$(dirname "$built")/$tool" "$DEPLOY/bin/$tool"; fi
  done
  if (( ! DRY_RUN )); then
    "$BIN" --version || die "the installed program does not run"
  fi

  step "Preflight"
  if lsmod | grep -q '^snd_bcm2835'; then die "snd_bcm2835 is loaded; finish setup/02-matrix-setup.sh and reboot first"; fi
  local avail; avail=$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo)
  (( avail >= 150 )) || warn "only ${avail} MB RAM available now; close Claude Code or other big programs before starting the display"

  step "Configuration in $ETC and state in $STATE"
  run install -d -m 755 "$ETC"
  if [[ ! -f $ETC/config.toml ]]; then
    local cfg_src=$DEPLOY/config/config.example.toml
    [[ -f $SELF_REPO/config/config.toml ]] && cfg_src=$SELF_REPO/config/config.toml
    run install -m 644 "$cfg_src" "$ETC/config.toml"
    [[ $cfg_src == *example* ]] && warn "installed the EXAMPLE config: set your location (tools/geocode.py <ZIP>) in $ETC/config.toml"
  else echo "  keeping existing $ETC/config.toml"; fi
  if [[ ! -f $ETC/secrets.env ]]; then
    if [[ -f $SELF_REPO/secrets.env ]]; then run install -m 600 -o root -g root "$SELF_REPO/secrets.env" "$ETC/secrets.env"
    else run install -m 600 -o root -g root "$DEPLOY/config/secrets.env.example" "$ETC/secrets.env"
      warn "put MATRIX_HA_TOKEN in $ETC/secrets.env or the Home Assistant row stays off"; fi
  else echo "  keeping existing $ETC/secrets.env"; fi
  run install -d -m 755 -o daemon -g daemon "$STATE"

  step "Privilege drop check"
  # After GPIO setup the panel library drops to user 'daemon'; config reloads and the cache still need to be readable.
  local DROP_ENV="" blocked="" d=$DEPLOY
  [[ -d $d ]] || d=$(dirname "$DEPLOY")
  while [[ $d != / ]]; do
    [[ $(stat -c %A "$d") == *x || $(stat -c %A "$d") == *t ]] || { blocked=$d; break; }
    d=$(dirname "$d")
  done
  if (( NODROP )); then
    DROP_ENV="Environment=MATRIX_DROP_PRIVILEGES=0"; echo "  --no-drop: the service keeps running as root"
  elif [[ -z $blocked ]]; then
    echo "  user 'daemon' can reach the code: the panel library drops root after GPIO setup"
  else
    warn "'$blocked' is private, so the service will run as root"; DROP_ENV="Environment=MATRIX_DROP_PRIVILEGES=0"
  fi

  step "systemd unit"
  local unit
  unit=$(sed -e "s#@REPO@#$DEPLOY#g" -e "s#@BIN@#$BIN#g" -e "s#@DROP_ENV@#$DROP_ENV#g" "$SELF_REPO/systemd/matrix-display.service")
  write_file "$UNIT" "$unit"
  write_file /etc/systemd/system/matrix-health.service \
    "$(sed -e "s#@REPO@#$DEPLOY#g" "$SELF_REPO/systemd/matrix-health.service")"
  write_file /etc/systemd/system/matrix-health.timer "$(cat "$SELF_REPO/systemd/matrix-health.timer")"
  confirm "This deploys the code from GitHub to $DEPLOY and installs a service that drives the LED panel."
  run systemctl daemon-reload
  run systemctl enable matrix-display.service
  run systemctl enable --now matrix-health.timer
  if (( START )); then run systemctl restart matrix-display.service; fi

  if (( ! DRY_RUN && START )); then
    step "Status"
    local i
    for i in $(seq 1 20); do systemctl is-active --quiet matrix-display && break; sleep 1; done
    sleep 3
    if systemctl is-active --quiet matrix-display; then
      echo "  service is active"
    else
      warn "the service is not active. If the log below mentions permissions, rerun with --no-drop."
    fi
    journalctl -u matrix-display.service --no-pager -n 12 | cut -c1-200
  fi
  echo
  if (( DRY_RUN )); then info "dry run complete; nothing changed."; else info "installed from $URL ($REF). Logs: journalctl -u matrix-display -f   Update: sudo $DEPLOY/setup/03-install-service.sh --apply"; fi
}

main "$@"; exit $?
