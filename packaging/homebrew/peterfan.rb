# Homebrew formula for PeterFan.
#
# To install from this tap:
#   brew tap uulab/peterfan
#   brew install peterfan
#
# Or directly:
#   brew install uulab/peterfan/peterfan
#
# To update this formula after a release:
#   1. Download the new Universal tarball and compute SHA256:
#      curl -sL <url> | sha256sum
#   2. Update `url`, `sha256`, and `version` below.
#   3. Run `brew audit --new-formula peterfan` to check.

class Peterfan < Formula
  desc "Tiny hardware monitor & fan controller for developers"
  homepage "https://github.com/uulab/peterfan"
  version "1.2.1"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/uulab/peterfan/releases/download/v#{version}/peterfan-v#{version}-universal-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_UNIVERSAL"
    else
      url "https://github.com/uulab/peterfan/releases/download/v#{version}/peterfan-v#{version}-universal-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_UNIVERSAL"
    end
  end

  on_linux do
    url "https://github.com/uulab/peterfan/releases/download/v#{version}/peterfan-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "PLACEHOLDER_SHA256_LINUX"
  end

  license "MIT"

  def install
    bin.install "peterfan"
    bin.install "peterfan-tui"
    bin.install "peterfand"
    # peterfan-menubar is a GUI app; only install on macOS
    if OS.mac?
      bin.install "peterfan-menubar"
      # If the tarball contains the .app bundle, install it too.
      if File.directory?("PeterFan.app")
        prefix.install "PeterFan.app"
      end
    end
  end

  def caveats
    <<~EOS
      The fan-control daemon (peterfand) needs to run as root to write the SMC.
      Set it up once with:
        peterfan install-daemon

      To start the menu-bar app at login:
        peterfan login-item install

      Diagnose your setup at any time:
        peterfan doctor
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/peterfan --version")
    assert_match "PeterFan", shell_output("#{bin}/peterfan --mock status")
  end
end
