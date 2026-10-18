# Packaging

Two ways in, and they build from the same source: the flake, and the packages a
distribution's package manager understands.

## Build the release

```
./packaging/release.sh 0.2.0
```

That builds the flake, then `.deb`, `.rpm` and a portable single file into
`dist/`, and prints the `git tag` / `gh release create` lines to publish them.
Nothing is tagged or uploaded by the script itself. `build-packages.sh` does the
same without the instructions at the end.

The single file is `komplement-<version>-<arch>.run`: a shell header with a
tar.gz payload appended. Run it and it unpacks into a directory of its own and
starts; `./komplement-….run install` puts both binaries in `~/.local/bin` and the
mappings in `~/.local/share/komplement`. It is not an AppImage, and that is
deliberate: an AppImage needs FUSE at run time, and the AppImageKit tools
(`appimagetool`, `appimagekit`) are no longer in nixpkgs; `linuxdeploy`, which is
still there, cannot read a Nix store binary because it runs `ldd`, which needs a
normal `/usr`.

A real AppImage therefore has its own script, `packaging/appimage.sh`, which
builds it inside a `buildFHSEnv` and packs it with `appimagetool` fetched from
the AppImage project's own releases (nixpkgs has neither `appimagetool` nor
`appimagekit` any more):

```
./packaging/appimage.sh 0.1.0       # dist/komplement-0.1.0-x86_64.AppImage
APPIMAGE_EXTRACT_AND_RUN=1 ./dist/komplement-0.1.0-x86_64.AppImage   # without FUSE
```

It is a separate script rather than part of `release.sh` because it is the one
step that may need a second look at your machine: three of its four stages are
measured (the FHS environment, `strip`, the icon), and the last one - the pack
itself - has not been run here.

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
