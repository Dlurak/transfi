{ lib, rustPlatform }:
let
  manifest = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = manifest.package.name;
  version = manifest.package.version;

  src = lib.cleanSource ./..;
  cargoLock.lockFile = ../Cargo.lock;

  meta = {
    description = "transfer files between computers over network";
    homepage = "https://github.com/dlurak/${manifest.package.name}/";
    mainProgram = manifest.package.name;
    maintainers = [ lib.maintainers.dlurak ];
  };
}
