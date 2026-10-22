cask "peterfan" do
  version "1.32.2"
  sha256 "e8edcec56126ad7afc570b94163e8272f21123c158ed8ba68e6c076ed3ef2cd6"

  url "https://github.com/uulab-official/peterfan/releases/download/v#{version}/PeterFan-v#{version}.dmg"
  name "PeterFan"
  desc "Hardware monitor and fan controller for macOS"
  homepage "https://github.com/uulab-official/peterfan"

  depends_on macos: :big_sur

  auto_updates true
  app "PeterFan.app"

  zap trash: [
    "~/Library/Application Support/PeterFan",
    "~/Library/Application Support/peterfan",
    "~/Library/Logs/PeterFan",
  ]

  caveats do
    <<~EOS
      PeterFan runs as a menu-bar app. Open PeterFan Settings to enable the
      optional fan-control daemon; macOS will request administrator approval
      once before the daemon can write fan settings.
    EOS
  end
end
