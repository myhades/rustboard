cask "rustboard" do
  version "0.1.0"
  on_macos do
    sha256 arm: "15bf4b4aa6d6968bcb5394ab6d8a8f8c4b1ad72e72e72369bf10a4c90dc51d02", intel: "2accc8e99f1506e4355525b17d682b798640d3c10255e7822f9d684fd35baa8b"
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
  on_linux do
    sha256 "93ce3ea406409fcd4b5998a619658c16320f2c26d8de9a833259b15bbc6cc957"
    url "https://github.com/myhades/rustboard/releases/download/v0.1.0/rustboard-0.1.0-linux-x86_64.tar.gz"
    name "Rustboard"
    desc "Native clipboard sync with lazy file transfer"
    homepage "https://github.com/myhades/rustboard"
    depends_on :linux
    depends_on arch: :intel
    depends_on formula: ["libsecret", "glib", "dbus", "patchelf"]
  
    installer script: {
      executable: "/bin/sh",
      args: ["#{staged_path}/platforms/linux/scripts/install.sh", "#{staged_path}/bin", "--no-path", "#{HOMEBREW_PREFIX}"],
      sudo: false,
    }
    binary "#{Dir.home}/.local/lib/rustboard/rustboard"
    uninstall script: {
      executable: "/bin/sh",
      args: ["#{staged_path}/platforms/linux/scripts/uninstall.sh"],
      sudo: false,
    }
    zap script: {
      executable: "/bin/sh",
      args: ["#{staged_path}/platforms/linux/scripts/purge.sh", "#{HOMEBREW_PREFIX}/bin/secret-tool"],
      sudo: false,
    }
  end
end
