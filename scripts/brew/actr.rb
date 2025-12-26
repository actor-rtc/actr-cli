class Actr < Formula
  desc "Actr CLI"
  homepage "https://github.com/actor-rtc/actr-cli"
  version "0.1.5"
  license "MIT"

  on_macos do
    on_arm do
      # TARGET: macos-arm64
      url "https://github.com/actor-rtc/actr-cli/releases/download/v0.1.5/actr-0.1.5-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_ME"
    end

    on_intel do
      # TARGET: macos-x86_64
      url "https://github.com/actor-rtc/actr-cli/releases/download/v0.1.5/actr-0.1.5-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_ME"
    end
  end

  on_linux do
    on_intel do
      # TARGET: linux-x86_64
      url "https://github.com/actor-rtc/actr-cli/releases/download/v0.1.5/actr-0.1.5-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_ME"
    end
  end

  def install
    bin.install "actr"
  end
end
