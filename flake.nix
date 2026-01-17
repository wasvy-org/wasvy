{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          inherit (pkgs) lib;
          # TODO: submit wkg to nix-packages
          wkg = pkgs.rustPlatform.buildRustPackage rec {
            pname = "wkg";
            version = "0.13.0";

            src = pkgs.fetchFromGitHub {
              owner = "bytecodealliance";
              repo = "wasm-pkg-tools";
              rev = "v${version}";
              hash = "sha256-6adUBw3jtmEq1y+hdnE7EBMgF5KChXr2MtOiSEPi1Ao=";
            };
            
            cargoHash = "sha256-BAHdOrLrSspSN1WsCtglCOQebI39zw6Byj9EgvU3onA=";

            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.openssl ];

            doCheck = false;

            meta = with lib; {
              description = "WebAssembly Kit Generator - tools for working with WebAssembly components";
              homepage = "https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wasm-package-cli";
              license = licenses.asl20;
              mainProgram = "wkg";
            };
          };
          wkgConfigFile = pkgs.writeText "config.toml" ''
            default_registry = "wa.dev"
          '';
          wkgConfigHook = ''
            mkdir -p ~/.config/wasm-pkg
            cp ${wkgConfigFile} ~/.config/wasm-pkg/config.toml
          '';
          craneLib = (crane.mkLib pkgs).overrideToolchain (
            p: p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml
          );
          packages = with pkgs; [
            # Dev tools
            just
            wkg

            poetry
            python3
            uv
          ];
          buildInputs = with pkgs; [
            # Build tools
            pkg-config
          ] ++ lib.optionals stdenv.isLinux [
            alsa-lib
            libxkbcommon
            udev
            vulkan-loader
            wayland
            xorg.libX11
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
          ] ++ lib.optionals stdenv.isDarwin [
            darwin.apple_sdk_11_0.frameworks.Cocoa
            rustPlatform
          ];
        in
        {
          devShells.default = craneLib.devShell {
            inherit packages buildInputs;

            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;

            shellHook = ''
              ${wkgConfigHook}

              # Impure python setup for now
              unset PYTHONPATH
              uv sync --directory examples/python_example
              . examples/python_example/.venv/bin/activate
            '';
          };
        }
      );
}