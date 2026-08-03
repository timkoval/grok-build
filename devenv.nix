{
  pkgs,
  lib,
  config,
  ...
}:
{
  # https://devenv.sh/languages/
  languages.rust = {
    enable = true;
    # Single source of truth: rust-toolchain.toml (channel 1.92.0, components,
    # targets, profile) is read via rust-overlay's fromRustupToolchainFile.
    # Do NOT set channel/version/components/targets here — devenv asserts they
    # are mutually exclusive with toolchainFile, and an explicit `components`
    # list *replaces* the defaults (dropping rustc/cargo, which breaks the
    # toolchain symlinkJoin on darwin).
    toolchainFile = ./rust-toolchain.toml;
  };

  # Build dependencies per README:
  # - DotSlash (hermetic bin/protoc resolver)
  # - protoc (protobuf codegen)
  packages = with pkgs; [
    dotslash
    protobuf
    pkg-config
    openssl
    # Common C deps for Rust -sys crates
    zlib
    curl
  ];

  # protoc on PATH for proto codegen fallback
  env.PROTOC = "${pkgs.protobuf}/bin/protoc";

  # https://devenv.sh/tasks/
  tasks = {
    "grok-build:setup" = {
      exec = ''
        echo "✓ grok-build devenv ready"
        echo "  Rust: $(rustc --version)"
        echo "  Cargo: $(cargo --version)"
        echo "  protoc: $(protoc --version)"
        echo "  dotslash: $(dotslash --help 2>&1 | head -1 || echo 'installed')"
        echo ""
        echo "Build commands:"
        echo "  cargo check -p xai-grok-pager-bin            # fast validation"
        echo "  cargo build -p xai-grok-pager-bin --release  # release binary"
        echo "  cargo run -p xai-grok-pager-bin              # build + launch TUI"
      '';
    };
  };
}
