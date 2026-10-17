{
  description = "kontrol-rs - Rust port of komplementary-kontrol (Komplete Kontrol A-series support)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));

      # The two binaries plus the mapping/preset/doc files that the C project
      # also ships (same file format, so a mapping file works with either tool).
      mkKontrolRs =
        pkgs:
        pkgs.rustPlatform.buildRustPackage (finalAttrs: {
          pname = "kontrol-rs";
          version = "0.1.0";

          src = lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              let
                base = baseNameOf path;
              in
              !(builtins.elem base [
                "target"
                "result"
              ]);
          };

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [
            pkgs.alsa-lib
            pkgs.hidapi
            pkgs.libusb1
          ];

          # The lookup paths are compiled in (option_env!), the equivalent of the
          # C tool's MAPPINGS_PATH/PRESETS_PATH, so `-m mappings/reaper.map`
          # works from any working directory.
          env = {
            KONTROL_MAPPINGS_PATH = "${placeholder "out"}/share/kontrol-rs/mappings";
            KONTROL_PRESETS_PATH = "${placeholder "out"}/share/kontrol-rs/presets";
          };

          postInstall = ''
            install -Dm644 -t $out/share/kontrol-rs/mappings mappings/*.map
            install -Dm644 -t $out/share/kontrol-rs/presets presets/*.pst
            install -Dm644 -t $out/share/doc/kontrol-rs doc/*.md
          '';

          # `cargo test` covers the parser (against the shipped mapping files),
          # the SysEx generator (against the shipped preset) and the HID state
          # machine (synthetic reports). The smoke test then checks that both
          # binaries run without the keyboard attached.
          doCheck = true;
          postCheck = ''
            # The binaries are under target/<triple>/release in this build, so
            # find them instead of guessing the path.
            komplement="$(find target -maxdepth 3 -type f -name komplement -perm -u+x | head -1)"
            konfigure="$(find target -maxdepth 3 -type f -name konfigure -perm -u+x | head -1)"
            test -x "$komplement"
            test -x "$konfigure"

            "$komplement" -h > komplement.usage
            grep -q "0.1alpha8" komplement.usage

            "$konfigure" -h > konfigure.usage
            grep -q "Konfigure" konfigure.usage

            # -q keeps konfigure's verbose report off stdout.
            "$konfigure" -q -p presets/basic_cc.pst > sysex.syx
            test "$(od -An -tx1 -N1 sysex.syx | tr -d ' ')" = "f0"
            test "$(wc -c < sysex.syx)" = "536"
            echo "konfigure -q -p presets/basic_cc.pst: OK (536 bytes of SysEx)"

            rm -f komplement.usage konfigure.usage sysex.syx
          '';

          meta = {
            description = "Utilities to make the Komplete Kontrol A-series keyboards usable under Linux (Rust port)";
            homepage = "https://github.com/fsphil/komplementary-kontrol";
            license = lib.licenses.gpl3Plus;
            mainProgram = "komplement";
            platforms = lib.platforms.linux;
          };
        });
    in
    {
      packages = forAllSystems (pkgs: rec {
        kontrol-rs = mkKontrolRs pkgs;
        default = kontrol-rs;
      });

      # `nix develop` -> cargo, rustc, gcc and pkg-config with ALSA, hidapi and
      # libusb on the PKG_CONFIG_PATH, so plain `cargo build` works on NixOS.
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy
            pkgs.gcc
            pkgs.pkg-config
            pkgs.alsa-lib
            pkgs.alsa-utils
            pkgs.hidapi
            pkgs.libusb1
            pkgs.usbutils
          ];

          shellHook = ''
            export PKG_CONFIG_PATH="${pkgs.alsa-lib.dev}/lib/pkgconfig:${pkgs.hidapi}/lib/pkgconfig:${pkgs.libusb1.dev}/lib/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
            echo "kontrol-rs dev shell"
            echo "  pkg-config: $(pkg-config --modversion alsa) / $(pkg-config --modversion hidapi-libusb) / $(pkg-config --modversion libusb-1.0)"
            echo "  build: cargo build   tests: cargo test   install into the store: nix build"
          '';
        };
      });

      overlays.default = final: prev: {
        kontrol-rs = mkKontrolRs final;
      };

      # The same surface for one user's session: the package, the config file in
      # the place a session keeps one, and the service to run it.
      homeManagerModules.default =
        { config, lib, pkgs, ... }:
        let
          cfg = config.services.kontrol-rs;
        in
        {
          options.services.kontrol-rs = {
            enable = lib.mkEnableOption "the komplement user service";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              description = "The kontrol-rs package to install.";
            };

            settings = lib.mkOption {
              type = lib.types.attrsOf (lib.types.oneOf [ lib.types.str lib.types.int ]);
              default = { };
              example = {
                pid = 1740;
                mcu = "self";
                knob1 = "volume";
                knob2 = "pan";
                knob8 = "master";
              };
              description = ''
                Written out as ~/.config/komplement/config, one `key = value`
                per line. The tool reports and ignores anything it cannot use.
              '';
            };

            restart = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = "Start it with the session and bring it back if it dies.";
            };
          };

          config = lib.mkIf cfg.enable {
            home.packages = [ cfg.package ];

            xdg.configFile."komplement/config" = lib.mkIf (cfg.settings != { }) {
              text =
                lib.concatStringsSep "\n"
                  (lib.mapAttrsToList (key: value: "${key} = ${toString value}") cfg.settings)
                + "\n";
            };

            systemd.user.services.komplement = lib.mkIf cfg.restart {
              Unit = {
                Description = "Komplete Kontrol A-series surface (kontrol-rs)";
                After = [ "sound.target" ];
                Wants = [ "sound.target" ];
              };
              Service = {
                ExecStart = "${cfg.package}/bin/komplement";
                Restart = "on-failure";
                RestartSec = 2;
              };
              Install.WantedBy = [ "default.target" ];
            };
          };
        };

      # Optional NixOS module: installs the binaries, the udev rules for the
      # keyboard and a rule that lets the `uinput` group write to /dev/uinput
      # (komplement needs that to synthesise key presses), and can run the
      # surface as a system service.
      nixosModules.default =
        { config, lib, pkgs, ... }:
        let
          cfg = config.services.kontrol-rs;
        in
        {
          options.services.kontrol-rs = {
            enable = lib.mkEnableOption "kontrol-rs (Komplete Kontrol A-series support)";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              description = "The kontrol-rs package to install.";
            };

            group = lib.mkOption {
              type = lib.types.str;
              default = "audio";
              description = ''
                Group that gets read/write access to the keyboard and to
                /dev/uinput. Add your user to this group.
              '';
            };

            productIds = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [
                "1730"
                "1740"
              ];
              example = [ "1740" ];
              description = ''
                USB product IDs (hex, lower case) of the Komplete Kontrol
                keyboards to grant access to: 1730 is the A25, 1740 the A49.
                Check your own keyboard with `lsusb | grep -i 17cc`.
              '';
            };

            installUdevRules = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = "Install the udev rules for the keyboard and /dev/uinput.";
            };

            runAsService = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = ''
                Run komplement as a system service, for a machine that is always
                on. A desktop session wants the user unit instead
                (packaging/systemd/komplement.user.service).
              '';
            };

            serviceUser = lib.mkOption {
              type = lib.types.str;
              default = "root";
              description = "User the system service runs as.";
            };

            configFile = lib.mkOption {
              type = lib.types.nullOr lib.types.path;
              default = null;
              description = ''
                Config file for the service. The system service has no home of
                its own to read one from, so without this the tool starts with
                nothing but its defaults.
              '';
            };

            loadUinputModule = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = ''
                Load the uinput kernel module. Without it /dev/uinput does not
                exist at all, so the udev rule for it has nothing to apply to
                and komplement cannot send key presses.
              '';
            };
          };

          config = lib.mkIf cfg.enable {
            environment.systemPackages = [ cfg.package ];

            boot.kernelModules = lib.mkIf cfg.loadUinputModule [ "uinput" ];

            services.udev.extraRules = lib.mkIf cfg.installUdevRules (
              lib.concatMapStrings (pid: ''
                # Komplete Kontrol A-series keyboard (17cc:${pid})
                SUBSYSTEM=="usb", ATTRS{idVendor}=="17cc", ATTRS{idProduct}=="${pid}", MODE="0660", GROUP="${cfg.group}"
                KERNEL=="hiddev*", ATTRS{idVendor}=="17cc", ATTRS{idProduct}=="${pid}", MODE="0660", GROUP="${cfg.group}"
              '') cfg.productIds
              + ''
                # uinput, used by komplement to send key presses
                KERNEL=="uinput", MODE="0660", GROUP="${cfg.group}", OPTIONS+="static_node=uinput"
              ''
            );

            users.groups.${cfg.group} = { };

            systemd.services.komplement = lib.mkIf cfg.runAsService {
              description = "Komplete Kontrol A-series surface (kontrol-rs)";
              wantedBy = [ "multi-user.target" ];
              after = [ "sound.target" "systemd-udev-settle.service" ];
              wants = [ "sound.target" ];
              serviceConfig = {
                ExecStart =
                  "${cfg.package}/bin/komplement"
                  + lib.optionalString (cfg.configFile != null) " -c ${cfg.configFile}";
                User = cfg.serviceUser;
                SupplementaryGroups = [ cfg.group ];
                Restart = "on-failure";
                RestartSec = 2;
                NoNewPrivileges = true;
                PrivateTmp = true;
                ProtectSystem = "strict";
                ProtectHome = "read-only";
              };
            };
          };
        };
    };
}
