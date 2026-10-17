# Packaging

Two ways in, and they build from the same source: the flake, and the packages a
distribution's package manager understands.

## Build the release

```
./packaging/release.sh 0.2.0
```

That builds the flake, then `.deb`, `.rpm` and an AppImage into `dist/`, and
prints the `git tag` / `gh release create` lines to publish them. Nothing is
tagged or uploaded by the script itself. `build-packages.sh` does the same
without the instructions at the end.

## The service

Two units, because the tool belongs in two different places:

| file | where it goes | what it is for |
| --- | --- | --- |
| `systemd/komplement.service` | `/usr/lib/systemd/system/` | the whole machine; `User=` and the keyboard's group are the caller's to set |
| `systemd/komplement.user.service` | `~/.config/systemd/user/` | one session; reads `~/.config/komplement/config`, stops with the session |

Both restart on failure, because a tool that cannot find its keyboard should
come back when the keyboard is plugged in again.

## Access

`udev/99-komplement.rules` grants the group `audio` read/write access to the
keyboard and to `/dev/uinput` — the latter is how the tool sends the key presses
a DAW never hears from the keyboard's own MIDI port. Add your user to that group
and log back in. A distribution that names its group differently should edit
`GROUP=` in the rule rather than add a second one.

## Nix

```
nix run github:UncleAndyV/kontrol-rs

# NixOS
services.kontrol-rs = {
  enable = true;
  group = "audio";
  runAsService = true;                    # the system service, off by default
  configFile = "/etc/komplement/config";  # the system service has no home
};

# home-manager
services.kontrol-rs = {
  enable = true;
  settings = { pid = 1740; mcu = "self"; knob1 = "volume"; knob2 = "pan"; };
};
```

The home-manager module writes `~/.config/komplement/config` from `settings` and
runs the user service; the NixOS module installs the binaries, the udev rules,
loads `uinput` and can run the system service.
