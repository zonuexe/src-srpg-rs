{
  description = "SRC (Simulation RPG Construction) — Rust + WebAssembly port";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, fenix }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        fenixPkgs = fenix.packages.${system};

        # Stable Rust + wasm32-unknown-unknown target. Pinned via rust-toolchain.toml.
        rustToolchain = fenixPkgs.combine [
          (fenixPkgs.stable.withComponents [
            "cargo"
            "rustc"
            "rust-src"
            "rustfmt"
            "clippy"
            "rust-analyzer"
          ])
          fenixPkgs.targets.wasm32-unknown-unknown.stable.rust-std
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          name = "src-srpg-rs";
          packages = [
            rustToolchain
            pkgs.trunk
            pkgs.wasm-bindgen-cli
            pkgs.binaryen # wasm-opt
            pkgs.nodejs_20 # 一部の補助ツール用 / for auxiliary tooling
            pkgs.just
            pkgs.cargo-nextest
          ];

          shellHook = ''
            echo "src-srpg-rs dev shell"
            echo "  rustc  : $(rustc --version)"
            echo "  trunk  : $(trunk --version 2>/dev/null || echo 'n/a')"
            echo ""
            echo "Try:   trunk serve   (then open http://127.0.0.1:8080)"
          '';
        };

        # 原典 SRC.Sharp (C# エンジン) を macOS でビルド/テストするための shell。
        # 差分オラクル用: SRCCore (netstandard2.1) と SRCCoreTests (net10.0) が対象。
        # WinForms ターゲット (SRCSharpForm 等 net10.0-windows) は macOS では不可。
        # 使い方: nix develop .#dotnet
        devShells.dotnet = pkgs.mkShell {
          name = "src-srpg-rs-dotnet";
          packages = [ pkgs.dotnet-sdk_10 ];

          shellHook = ''
            export DOTNET_CLI_TELEMETRY_OPTOUT=1
            export DOTNET_NOLOGO=1
            echo "src-srpg-rs dotnet (oracle) shell"
            echo "  dotnet : $(dotnet --version)"
            echo ""
            echo "Build engine: dotnet build SRC.Sharp/SRC.Sharp/SRCCore/SRCCore.csproj -c Release"
            echo "Run tests   : dotnet test  SRC.Sharp/SRC.Sharp/SRCCoreTests/SRCCoreTests.csproj -c Release"
          '';
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
