{
  description = "Open-source terminal SSH manager and SSH config editor for macOS and Linux.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    { self, nixpkgs, rust-overlay, crane }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;

      # Memoized: each system's nixpkgs is imported once and reused
      # across packages, devShells, formatter and checks.
      pkgsFor = forAllSystems (system: import nixpkgs {
        inherit system;
        overlays = [ (import rust-overlay) ];
      });

      # Whitelist source filter: skips docs, screenshots, goldens and
      # other churn that would invalidate the cache for nothing.
      filteredSrc = pkgs: pkgs.lib.cleanSourceWith {
        src = ./.;
        name = "purple-source";
        filter = path: type:
          let
            rootStr = toString ./.;
            pathStr = toString path;
            relPath =
              if pathStr == rootStr
              then ""
              else pkgs.lib.removePrefix (rootStr + "/") pathStr;
          in
            (pkgs.lib.cleanSourceFilter path type)
            && (
              relPath == ""
              || pkgs.lib.elem relPath [
                   "Cargo.toml"
                   "Cargo.lock"
                   "build.rs"
                   "rust-toolchain.toml"
                   "CHANGELOG.md"
                 ]
              || relPath == "locales"
              || pkgs.lib.hasPrefix "locales/" relPath
              || relPath == "src"
              || pkgs.lib.hasPrefix "src/" relPath
              || relPath == "tests"
              || (pkgs.lib.hasPrefix "tests/" relPath
                  && !pkgs.lib.hasPrefix "tests/visual_golden" relPath)
            );
      };

      mkCraneLib = pkgs:
        let
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
        (crane.mkLib pkgs).overrideToolchain rustToolchain;

      # Shared build arguments between the deps-only derivation and the
      # full package. Splitting them is what makes crane fast: the deps
      # layer is hashed on Cargo.lock and reused whenever lockfile and
      # toolchain are unchanged, so source-only pushes skip the 800+
      # dep recompile.
      mkCommonArgs = pkgs:
        let
          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        in {
          pname = "purple-ssh";
          version = cargoToml.package.version;

          src = filteredSrc pkgs;
          strictDeps = true;

          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = [ pkgs.openssl ];

          # Use system openssl. Cargo.toml only vendors openssl for
          # cfg(target_env = "musl"); Nix builds against glibc/darwin.
          OPENSSL_NO_VENDOR = 1;

          # Tests need $HOME, ~/.ssh, a working ssh binary and serialized
          # PATH manipulation (vault_ssh_tests::PATH_LOCK). The Nix
          # sandbox provides none of those, so the test suite runs in CI
          # instead. `nix flake check` still verifies the package builds.
          doCheck = false;
        };

      mkPurple = pkgs:
        let
          craneLib = mkCraneLib pkgs;
          commonArgs = mkCommonArgs pkgs;
          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

          # Deps layer hashes on Cargo.toml verbatim; cargo requires
          # package.version so a release commit invalidates it once.
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in
        craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;

          # Skip LTO inside Nix so the link phase parallelises. Release
          # binaries via cargo publish keep full LTO from Cargo.toml.
          CARGO_PROFILE_RELEASE_LTO = "false";

          meta = with pkgs.lib; {
            description = cargoToml.package.description;
            homepage = "https://getpurple.sh";
            license = licenses.mit;
            mainProgram = "purple";
            platforms = platforms.unix;
          };
        });
    in
    {
      packages = forAllSystems (system:
        let purple = mkPurple pkgsFor.${system};
        in {
          default = purple;
          purple-ssh = purple;
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/purple";
        };
      });

      devShells = forAllSystems (system:
        let
          pkgs = pkgsFor.${system};
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.default ];
            packages = with pkgs; [
              rustToolchain
              cargo-audit
              cargo-deny
            ];
            OPENSSL_NO_VENDOR = 1;
          };
        });

      formatter = forAllSystems (system: pkgsFor.${system}.nixfmt-rfc-style);

      checks = forAllSystems (system:
        let
          pkgs = pkgsFor.${system};
          craneLib = mkCraneLib pkgs;
          commonArgs = mkCommonArgs pkgs;
        in {
          package = self.packages.${system}.default;

          fmt = craneLib.cargoFmt {
            inherit (commonArgs) src pname version;
          };
        });

      overlays.default = final: _prev: {
        purple-ssh = mkPurple final;
      };
    };
}
