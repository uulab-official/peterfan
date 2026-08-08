cask "peterfan" do
  version "1.31.3"
  sha256 "4ed21dc6883922c07f8454d165665c9921b8bae459f0c1cd90485d49bd48f07b"

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
