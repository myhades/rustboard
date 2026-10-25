cask "rustboard" do
  version "0.2.0"
  on_macos do
    sha256 arm: "0c033b46a7a549a101f1ae96e638d64c97691cf0442f0b246d26eb72d769c517", intel: "8a496d593e7e2aeb774ff35080b3cd99024a79ba5335b25e0a69720e13ffb182"
    arch arm: "arm64", intel: "x86_64"
    url "https://github.com/myhades/rustboard/releases/download/v0.2.0/rustboard-#{version}-macos-#{arch}.tar.gz"
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
    binary "#{Dir.home}/Library/Application Support/Rustboard/bin/rustboard"
    postflight_steps do
      remove "bin/rustboard"
    end
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
    sha256 "1c82f5c504c1d040b1e0bfc7a7add0583b48dd03caf9ce823c6b47bf1569fca5"
    url "https://github.com/myhades/rustboard/releases/download/v0.2.0/rustboard-0.2.0-linux-x86_64.tar.gz"
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
