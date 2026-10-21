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
  zap script: {
    executable: "/bin/bash",
    args: ["#{staged_path}/platforms/macos/scripts/purge.sh"],
    sudo: false,
  }
end
