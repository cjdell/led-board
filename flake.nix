{
  description = "LED board firmware — Raspberry Pi Pico 2 W (RP2350) + ESP32-C3 slave radio";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    { self, nixpkgs, fenix }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;

      # Fenix nightly "default" = every target (thumbv8m… + riscv32imc…) and
      # every component, which is exactly what this repo needs:
      #   thumbv8m.main-none-eabihf     → firmware/     (RP2350)
      #   riscv32imc-unknown-none-elf   → slave_radio/  (ESP32-C3)
      mkRust = system: fenix.packages.${system}.nightly.default;

      devShell =
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          rust = mkRust system;
        in
        pkgs.mkShell {
          packages =
            [
              rust # rustc + cargo, nightly, all embedded targets
              pkgs.just
              pkgs.fzf
              pkgs.pkg-config

              # ESP32-C3 (slave_radio)
              pkgs.espflash

              # RP2350 (firmware)
              pkgs.picotool
              pkgs.probe-rs
              pkgs.cargo-binutils # provides `cargo objcopy` (used by `just main`)

              # USB access to the boards
              pkgs.libusb1
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.openssl_3 ];

          shellHook = ''
            echo "led-board dev shell: $(rustc --version)"
          '';
        };
    in
    {
      devShells = forAllSystems (system: { default = devShell system; });

      # Quick both-sides build check (also handy in CI): nix run .#check
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          drv = pkgs.writeShellScriptBin "led-board-check" ''
            set -euo pipefail
            cargo check -p firmware --features wifi,defmt_rtt --bin ws --target thumbv8m.main-none-eabihf
            cargo check -p firmware --features radio,defmt_rtt --bin ws --target thumbv8m.main-none-eabihf
            cargo test -p radio_link
            (cd slave_radio && cargo check --release)
          '';
        in
        {
          default = drv;
          check = drv;
        }
      );
    };
}
