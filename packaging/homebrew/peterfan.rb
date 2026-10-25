# Homebrew formula for PeterFan.
#
# To install from this tap:
#   brew tap uulab-official/peterfan
#   brew install peterfan
#
# Or directly:
#   brew install uulab-official/peterfan/peterfan
#
# To update this formula after a release:
#   1. Download the new Universal tarball and compute SHA256:
#      curl -sL <url> | sha256sum
#   2. Update `url`, `sha256`, and `version` below.
#   3. Run `brew audit --formula --strict uulab-official/peterfan/peterfan` to check.

class Peterfan < Formula
  desc "Tiny hardware monitor & fan controller for developers"
  homepage "https://github.com/uulab-official/peterfan"
  url "https://github.com/uulab-official/peterfan/releases/download/v1.32.9/peterfan-v1.32.9-universal-apple-darwin.tar.gz"
  sha256 "e27b21defe2f73ed1cb1eae667164975323101356aecbfc5237f84b245da998a"
  license "MIT"

  depends_on :macos

  def install
    bin.install "peterfan"
    bin.install "peterfan-tui"
    bin.install "peterfand"
    bin.install "peterfan-menubar"
    prefix.install "PeterFan.app"
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
