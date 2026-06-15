cask "terminal-studio" do
  version "VERSION_PLACEHOLDER"
  sha256 "SHA256_PLACEHOLDER"

  url "https://github.com/dpkay-io/terminal-studio/releases/download/v#{version}/terminal-studio-macos-arm.dmg"
  name "Terminal Studio"
  desc "GPU-accelerated terminal multiplexer"
  homepage "https://github.com/dpkay-io/terminal-studio"

  depends_on arch: :arm64

  app "Terminal Studio.app"

  postflight do
    system_command "/usr/bin/xattr",
      args: ["-dr", "com.apple.quarantine", "#{appdir}/Terminal Studio.app"]
  end

  zap trash: "~/.config/terminal-studio"
end
