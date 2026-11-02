# Security

## Reporting a problem

If you find a security problem in Modern Day Pioneer, please report it
privately with GitHub's **Report a vulnerability** button on this repository's
Security tab, rather than in a public issue. Include the app version
(Settings > About) and the steps that show the problem. We aim to reply within
a week. There is no bug bounty.

## What is in scope

- The Windows app and its installer from this repository's Releases page.
- The exported family history (the HTML pages and files the app writes).

Problems in FFmpeg, PDFium or whisper.cpp should go to those projects; we will
update the copies we ship once they release a fix.

## How releases are made

Installers are built from this repository's source by the "Windows release"
GitHub Actions workflow (`.github/workflows/windows-release.yml`). See the
"Code signing policy" section of [README.md](README.md).
