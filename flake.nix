{
  description = "Sn0wflakeOS / icepeak - clean-room Rust hobby operating system";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in {
      devShells.${system}.default = pkgs.mkShell {
        name = "icepeak-dev";
        packages = with pkgs; [
          rustc
          cargo
          gcc
          binutils
          nasm
          grub2
          xorriso
          mtools
          qemu
          OVMF
          gdb
          git
          findutils
        ];
        shellHook = ''
          export ICEPEAK_ROOT="$PWD"
          export OVMF_FD="${pkgs.OVMF.fd}/FV/OVMF.fd"
          echo "icepeak dev shell: rustc $(rustc --version | cut -d' ' -f2), qemu $(qemu-system-x86_64 --version | head -1 | cut -d' ' -f4)"
          echo "build: ./build.sh    run: ./run.sh"
        '';
      };

      packages.${system}.default =
        pkgs.runCommand "sn0wflake.iso" { nativeBuildInputs = with pkgs; [ nasm rustc cargo gcc binutils grub2 xorriso ]; } ''
          cp -r ${./.} src
          chmod -R u+w src
          cd src
          export HOME=$TMPDIR
          ./build.sh
          mkdir -p $out
          cp sn0wflake.iso $out/
        '';
    };
}
