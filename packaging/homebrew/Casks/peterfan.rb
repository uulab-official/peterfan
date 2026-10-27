cask "peterfan" do
  version "1.32.11"
  sha256 "813824f5f650c883ba3809f953e5bbe8014eeb7d699fb7a276ec3788bd9ebe94"

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
