#!/bin/sh
# A package manager has just put the unit and the udev rules in place: tell the
# two daemons that read them, and say what a person still has to do, because
# none of it can be done from here.
set -e

if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload >/dev/null 2>&1 || true
fi
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules >/dev/null 2>&1 || true
    udevadm trigger --subsystem-match=misc --sysname-match=uinput >/dev/null 2>&1 || true
fi

# The system unit starts with `-c /etc/komplement/config`, and a path named that
# way has to be there, so the package leaves one behind rather than a service
# that cannot start. Yours, if there is already one, is never touched.
if [ ! -e /etc/komplement/config ]; then
    cp /etc/komplement/config.example /etc/komplement/config
    echo "komplement: a starting /etc/komplement/config was written from the example."
fi

cat <<'EOF'

komplement is installed.

  Your user needs access to the keyboard and to /dev/uinput; the rules granted
  it to the group "audio":

      sudo usermod -aG audio "$USER"      # then log out and back in

  The settings are in /etc/komplement/config - the example's values are a
  starting point, and /etc/komplement/mappings carries the mappings.

  And start it - one line, depending on whose machine this is:

      systemctl enable --now komplement
      # or, for your own session only:
      sudo cp /usr/share/komplement/komplement.user.service ~/.config/systemd/user/
      systemctl --user enable --now komplement

EOF
