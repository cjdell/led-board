let
  pkgs = import <nixpkgs> { };
  overrides = (builtins.fromTOML (builtins.readFile ./rust-toolchain.toml));
in
pkgs.callPackage (
  {
    stdenv,
    just,
    fzf,
    mkShell,
    cargo,
    rustc,
    rustup,
    rustPlatform,
    pkg-config,
    pipewire,
    libx11,
    libxcursor,
  }:
  mkShell {
    strictDeps = true;
    nativeBuildInputs = [
      just
      fzf
      # cargo
      # rustc
      rustup
      rustPlatform.bindgenHook
      pkg-config
    ];
    buildInputs = [
      pipewire
      libx11
      libxcursor
    ];
    RUSTC_VERSION = overrides.toolchain.channel;
    RUST_SRC_PATH = "${rustPlatform.rustLibSrc}";
    RUSTFLAGS = [
      "-C"
      "linker=${stdenv.cc.targetPrefix}ld"
      "--sysroot"
      "/home/cjdell/Projects/led-board"
    ];
  }
) { }
