# Formula template. VERSION and the four SHA256 values are placeholders;
# the release fills them (see docs/technical.md, "Releasing").
# Needs publicly downloadable release assets; not available otherwise.
class Bar < Formula
  desc "Fast, configurable and extensible status line for Claude Code"
  homepage "https://github.com/hop-top/bar"
  version "VERSION"
  license "Apache-2.0"

  base = "https://github.com/hop-top/bar/releases/download/v#{version}"

  on_macos do
    on_arm do
      url "#{base}/bar-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256_AARCH64_APPLE_DARWIN"
    end
    on_intel do
      url "#{base}/bar-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_X86_64_APPLE_DARWIN"
    end
  end

  on_linux do
    on_arm do
      url "#{base}/bar-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_AARCH64_UNKNOWN_LINUX_GNU"
    end
    on_intel do
      url "#{base}/bar-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_X86_64_UNKNOWN_LINUX_GNU"
    end
  end

  def install
    bin.install "bar"
    doc.install "LICENSE-models.dev"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/bar --version")
  end
end
