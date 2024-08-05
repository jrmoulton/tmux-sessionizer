{
  description = "devShell for Rust projects";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs = inputs @ {
    self,
    nixpkgs,
    flake-parts,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      perSystem = {
        config,
        self',
        inputs',
        pkgs,
        system,
        ...
      }: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            self.overlays.default
          ];
        };
      in {
        packages = rec {
          default = tmux-sessionizer;
          inherit (pkgs) tmux-sessionizer;
        };
        devShells.default = pkgs.mkShell {
          name = "rust devShell";
          OPENSSL_NO_VENDOR = 1;
          buildInputs = with pkgs;
          with pkgs.rustPlatform; [
            cargo
            clippy
            rustc
            rustfmt
            rust-analyzer
            openssl
            pkg-config
          ]
          ++ lib.optionals stdenv.isDarwin [
            libgit2
            darwin.Security
           ];
        };
      };
      flake = {
        overlays.default = final: prev: {
          tmux-sessionizer = prev.tmux-sessionizer.overrideAttrs (oa: {
            src = self;
            version = ((final.lib.importTOML "${self}/Cargo.toml").package).version;
            cargoDeps = final.rustPlatform.importCargoLock {
              lockFile = self + "/Cargo.lock";
            };
            OPENSSL_NO_VENDOR = 1;

            nativeBuildInputs = oa.nativeBuildInputs ++ [ final.installShellFiles ];
            postInstall = ''
              installShellCompletion --cmd tms \
                --bash <($out/bin/tms --generate bash) \
                --fish <($out/bin/tms --generate fish) \
                --zsh <($out/bin/tms --generate zsh)
            '';
          });
        };

      };
    };
}
