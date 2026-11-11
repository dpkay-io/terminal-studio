cask "terminal-studio" do
  # Keep in sync with `version` in Cargo.toml — bump both in the release PR.
  # Guarded by `cask_version_matches_cargo_version` and scripts/release.sh.
  version "0.16.0"
  sha256 :no_check

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
