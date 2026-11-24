{
  description = "LEZ Program Registry (LP-0023): registry_ffi for the Logos Core Module";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || (builtins.match ".*/artifacts/.*\\.json$" path != null)
            || (builtins.match ".*/include/.*\\.h$" path != null);
        };
        commonArgs = {
          inherit src;
          strictDeps = true;
          pname = "registry_ffi";
          version = "0.1.0";
          cargoExtraArgs = "-p registry_ffi";
          RISC0_SKIP_BUILD = "1";
          doCheck = false;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.openssl ];
        };
        registryFfi = craneLib.buildPackage (commonArgs // {
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          postInstall = ''
            mkdir -p $out/include
            cp modules/registry/ffi/include/registry_ffi.h $out/include/
          '' + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
            if [ -f $out/lib/libregistry_ffi.dylib ]; then
              install_name_tool -id "$out/lib/libregistry_ffi.dylib" $out/lib/libregistry_ffi.dylib
            fi
          '';
        });
      in
      {
        packages.default = registryFfi;
        packages.registry_ffi = registryFfi;
      });
}
