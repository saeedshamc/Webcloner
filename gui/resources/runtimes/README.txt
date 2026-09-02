PHP and .NET runtimes for the local server (bundled inside the app).

This folder is populated by running from the repo root:

  .\scripts\setup-runtimes.ps1

Expected layout after setup:

  runtimes/php/php.exe
  runtimes/dotnet/dotnet.exe

These binaries are not committed to git (too large). The setup script downloads them once.
