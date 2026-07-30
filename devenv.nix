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
    channel = "stable";
    # rust-toolchain.toml pins 1.92.0 — devenv uses rustup which respects it
    components = [ "rustfmt" "clippy" ];
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
