{
  description = "purple — Open-source terminal SSH manager and SSH config editor";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        nativeBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
        ];

        buildInputs = with pkgs; [
          openssl
        ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
          darwin.apple_sdk.frameworks.Security
          darwin.apple_sdk.frameworks.SystemConfiguration
        ];
      in
      {
        packages = {
          default = self.packages.${system}.purple-ssh;
          purple-ssh = pkgs.rustPlatform.buildRustPackage {
            pname = "purple-ssh";
            version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            inherit nativeBuildInputs buildInputs;

            doCheck = false;

            OPENSSL_NO_VENDOR = 1;

            meta = with pkgs.lib; {
              description = "Open-source terminal SSH manager and SSH config editor";
              homepage = "https://getpurple.sh";
              license = licenses.mit;
              mainProgram = "purple";
              platforms = platforms.unix;
            };
          };
        };

        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [
            cargo-audit
            cargo-deny
          ]);
        };
      }
    );
}