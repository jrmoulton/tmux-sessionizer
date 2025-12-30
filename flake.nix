{
  description = "devShell for Rust projects";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    inputs@{
      self,
      nixpkgs,
      flake-parts,
      rust-overlay,
      crane,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      perSystem =
        {
          config,
          self',
          inputs',
          pkgs,
          system,
          ...
        }:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [
              self.overlays.default
              (import rust-overlay)
            ];
          };
          rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
          commonArgs = with pkgs; {
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;

            OPENSSL_NO_VENDOR = 1;
            buildInputs = [
              openssl
            ];
            nativeBuildInputs = [
              pkg-config
              installShellFiles
            ];
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          tmux-sessionizer = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              postInstall =
                with pkgs;
                lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
                  installShellCompletion --cmd tms \
                    --bash <(COMPLETE=bash $out/bin/tms) \
                    --fish <(COMPLETE=fish $out/bin/tms) \
                    --zsh <(COMPLETE=zsh $out/bin/tms)
                '';
              meta.mainProgram = "tms";
            }
          );
        in
        {
          packages = rec {
            default = tmux-sessionizer;
            inherit tmux-sessionizer;
          };

          checks = {
            inherit (self.packages.${system}) tmux-sessionizer;
            clippy = craneLib.cargoClippy (
              commonArgs
              // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--all-targets --all-features -- --D warnings";
              }
            );
            fmt = craneLib.cargoFmt commonArgs;
            test = craneLib.cargoTest (
              commonArgs
              // {
                inherit cargoArtifacts;
              }
            );
          };

          devShells.default = craneLib.devShell {
            OPENSSL_NO_VENDOR = 1;
            inputsFrom = [ tmux-sessionizer ];
            packages = with pkgs; [
              rust-analyzer
              cargo-dist
              rustup # required for cargo-dist
            ];
          };
        };

      flake = {
        overlays.default = final: prev: {
          inherit (self.packages.${final.stdenv.hostPlatform.system}) tmux-sessionizer;
        };

      };
    };
}
