{
  description = "Grok Build — AI-native software engineering agent";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        lib = pkgs.lib;
      in
      {
        packages = {
          xai-grok-pager = pkgs.rustPlatform.buildRustPackage rec {
            pname = "xai-grok-pager";
            version = "0.2.112";
            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "async-openai-0.33.1" = "sha256-pCq9Wo50T6SKlVbZk58v8NrhTi9iwZQ5cErm7uB9+eY=";
                "async-openai-macros-0.1.1" = "sha256-pCq9Wo50T6SKlVbZk58v8NrhTi9iwZQ5cErm7uB9+eY=";
                "nucleo-0.5.0" = "sha256-ztSgjBI8vhKvrWmpT5K1UoHQRnbbrbEtSnvRkFmhSNc=";
                "nucleo-matcher-0.3.1" = "sha256-ztSgjBI8vhKvrWmpT5K1UoHQRnbbrbEtSnvRkFmhSNc=";
              };
            };

            nativeBuildInputs = with pkgs; [ pkg-config protobuf ];

            buildInputs =
              with pkgs;
              [ openssl zlib ] ++ lib.optionals pkgs.stdenv.isLinux [ curl ];

            # Some workspace crates (xai-grok-tools, xai-grok-shell) embed
            # ripgrep at build time by downloading it from GitHub releases.
            # In the Nix sandbox there's no network, so we point them at
            # the nixpkgs ripgrep binary instead.
            env.GROK_TOOLS_BUNDLE_RG_PATH = "${pkgs.ripgrep}/bin/rg";
            env.GROK_SHELL_BUNDLE_RG_PATH = "${pkgs.ripgrep}/bin/rg";

            cargoBuildFlags = [ "-p" "xai-grok-pager-bin" ];

            doCheck = false;

            meta = with lib; {
              description = "Grok Build TUI — AI-native software engineering agent";
              homepage = "https://github.com/xai/grok-build";
              license = licenses.asl20;
              mainProgram = "xai-grok-pager";
              platforms = [
                "x86_64-linux"
                "aarch64-linux"
                "x86_64-darwin"
                "aarch64-darwin"
              ];
            };
          };

          default = self.packages.${system}.xai-grok-pager;
        };
      }
    );
}
