{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    cargo
    rustc
    rustPlatform.rustLibSrc
    rust-analyzer
    pkg-config
    openssl
    ffmpeg
    yt-dlp
    nil
  ];

  shellHook = ''
    export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig"
    export RUST_SRC_PATH="${pkgs.rustPlatform.rustLibSrc}"
    mkdir -p .direnv
    ln -snf "${pkgs.rustPlatform.rustLibSrc}" .direnv/rust-src
  '';
}
