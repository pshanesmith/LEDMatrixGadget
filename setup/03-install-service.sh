#!/usr/bin/env bash
# Deploy the display from GitHub into /opt/matrix and run it as a systemd service.
#
#   ./setup/03-install-service.sh                 dry run (default)
#   sudo ./setup/03-install-service.sh --apply    deploy, install, enable and start
# Run it again later (from /opt/matrix/setup or from a checkout) to UPDATE: it pulls, reinstalls, restarts.
#
# Options: --repo URL    GitHub clone URL (default: the 'origin' of the checkout you run this from)
#          --ref NAME    branch or tag to deploy (default: main)
#          --rebuild     rebuild the panel library instead of reusing a finished build from the checkout
#          --no-start    install and enable but do not start now
#          --no-drop     keep running as root (skip the privilege drop)
#          --uninstall   stop and remove the service (keeps /opt/matrix, /etc/matrix and the state dir)
#          --yes         skip confirmation prompts
# Layout:  /opt/matrix        clone of the repository, with .venv/ and vendor/ inside (root-owned, world-readable)
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
  local START=1 UNINSTALL=0 REBUILD=0 NODROP=0
  ASSUME_YES=0
  while (( $# )); do
    case $1 in
      --apply) DRY_RUN=0 ;;
      --repo) URL=$2; shift ;;
      --ref) REF=$2; shift ;;
      --rebuild) REBUILD=1 ;;
      --no-start) START=0 ;;
      --no-drop) NODROP=1 ;;
      --uninstall) UNINSTALL=1 ;;
      --yes) ASSUME_YES=1 ;;
      -h|--help) sed -n '2,23p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; return 0 ;;
      *) die "unknown option: $1 (try --help)" ;;
    esac
    shift
  done
  KEY=$ETC/deploy_key
  local PY=$DEPLOY/.venv/bin/python

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

  step "Python environment and panel library"
  # Reuse a finished build from the checkout this script was started from (a rebuild takes 10+ minutes
  # on this Pi and risks exhausting memory); otherwise build with setup/02-matrix-setup.sh.
  local reuse=""
  if (( ! REBUILD )) && [[ $SELF_REPO != "$DEPLOY" && -x $SELF_REPO/.venv/bin/python ]] \
      && "$SELF_REPO/.venv/bin/python" -c 'import rgbmatrix' 2>/dev/null \
      && [[ -x $SELF_REPO/vendor/rpi-rgb-led-matrix/examples-api-use/demo ]]; then
    reuse=$SELF_REPO
  fi
  if [[ ! -x $PY ]]; then run python3 -m venv --system-site-packages "$DEPLOY/.venv"; fi
  if [[ -x $PY ]] && "$PY" -c 'import rgbmatrix' 2>/dev/null; then
    echo "  rgbmatrix already installed in $DEPLOY/.venv"
  elif [[ -n $reuse ]]; then
    echo "  reusing the finished build from $reuse"
    if [[ ! -d $DEPLOY/vendor ]]; then run cp -a "$reuse/vendor" "$DEPLOY/vendor"; fi
    if (( ! DRY_RUN )); then
      sp_old=$(echo "$reuse"/.venv/lib/python3*/site-packages)
      sp_new=$(echo "$DEPLOY"/.venv/lib/python3*/site-packages)
      cp -a "$sp_old"/rgbmatrix "$sp_old"/rgbmatrix-*.dist-info "$sp_new"/
      chown -R root:root "$DEPLOY/vendor" "$sp_new"
    else
      echo "  [dry-run] copy rgbmatrix package files into the new virtualenv"
    fi
  else
    run "$DEPLOY/setup/02-matrix-setup.sh" --apply --yes --only deps,library,python
  fi
  # Editable install: a later 'git pull' is all an update needs; no pip packages come from the internet
  # except the build backend (Pillow is the system package, rgbmatrix is built from the pinned source).
  run "$PY" -m pip install -q -e "$DEPLOY"
  if (( ! DRY_RUN )); then
    "$PY" -c 'import matrix_display, rgbmatrix, PIL; print("  imports OK, Pillow from", PIL.__file__)' \
      || die "the deployed environment cannot import the app or the panel bindings"
  fi

  # tools/health-check.sh compares this with the installed Pillow (the panel bindings depend on its layout).
  if (( ! DRY_RUN )) && [[ ! -f /var/lib/matrix-display/pillow-version ]]; then
    install -d -m 755 /var/lib/matrix-display
    dpkg-query -W -f='${Version}\n' python3-pil > /var/lib/matrix-display/pillow-version
    echo "  recorded the Pillow version of this build: $(cat /var/lib/matrix-display/pillow-version)"
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
  # After GPIO setup the panel library drops to user 'daemon'; code imported later must stay readable.
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
  unit=$(sed -e "s#@REPO@#$DEPLOY#g" -e "s#@PYTHON@#$PY#g" -e "s#@DROP_ENV@#$DROP_ENV#g" "$SELF_REPO/systemd/matrix-display.service")
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
