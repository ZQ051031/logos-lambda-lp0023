{
  description = "Logos Core Module for the LEZ Program Registry (LP-0023)";

  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder/0.3.3";
    # The repo-root flake builds the registry_ffi crate from the whole Cargo
    # workspace (it path-depends on the SDK and registry_core).
    lez_registry.url = "path:../..";
  };

  outputs = inputs@{ logos-module-builder, lez_registry, ... }:
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      externalLibInputs = {
        registry_ffi = { input = lez_registry; packages.default = "registry_ffi"; };
      };
    };
}
