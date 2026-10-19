cask "rustboard" do
  version "0.1.0"
  sha256 arm: "1e3d1aae1232ff67fd62dbcbb506c05d012cbeecfd53041683b76dec9059820f", intel: "ef647f6eba1107fe6328c3a95d1f4ac8f5622960b9b959d18f76ed825ea6274c"
  arch arm: "arm64", intel: "x86_64"
  url "https://github.com/myhades/rustboard/releases/download/v0.1.0/rustboard-#{version}-macos-#{arch}.tar.gz"
  name "Rustboard"
  desc "Native clipboard sync with lazy file transfer"
  homepage "https://github.com/myhades/rustboard"
  depends_on macos: :ventura
  depends_on formula: "python@3.13"

  installer script: {
    executable: "/bin/bash",
    args: ["#{staged_path}/platforms/macos/scripts/install.sh", "--no-path"],
    sudo: false,
  }
  binary "#{staged_path}/bin/rustboard"
  uninstall script: {
    executable: "/bin/bash",
    args: ["#{staged_path}/platforms/macos/scripts/uninstall.sh"],
    sudo: false,
  }
  caveats <<~EOS
    Development-signed and NOT notarized. Other-Mac File Provider acceptance is pending.
    macOS may require manual Open Anyway approval.
    Configure listen and pairing after installation: rustboard config
    Apply saved changes: rustboard reload
    Permissions: see docs/INSTALLATION.md for manual System Settings instructions.
    Full Disk Access is only needed for protected source locations.
    Uninstall retains identity, peers and configuration.
  EOS
end
