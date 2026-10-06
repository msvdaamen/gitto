# Installing Gitto

Gitto runs on Windows (x64), macOS (Apple Silicon) and Arch Linux (x86_64). Download it from the
[releases page](https://github.com/msvdaamen/gitto/releases):

- **A release**, like Gitto 1.3.0, is the version to use. The newest is at
  [releases/latest](https://github.com/msvdaamen/gitto/releases/latest).
- **Gitto Nightly** is built from every change to `main`, so it has what's newest but may be broken.
  It's at [releases/tag/nightly](https://github.com/msvdaamen/gitto/releases/tag/nightly), and
  replaced with each build.

Gitto stays on what you installed: a release only updates to a newer release, and Gitto Nightly to
the next nightly. How to update it is in [Updating](#updating).

## Git

Gitto uses the Git installed on your computer, and needs **Git 2.41 or newer**. If it can't find Git,
or finds an older one, it says so when it starts, and works as soon as a newer one is installed.

- **Windows:** [Git for Windows](https://git-scm.com/download/win).
- **macOS:** `brew install git` with [Homebrew](https://brew.sh). The Git that comes with Xcode's
  command line tools may be older than 2.41.
- **Arch Linux:** installed with Gitto, as the package depends on it.

## Windows

1. Download `Gitto-<version>.Setup.exe`.
2. Run it. Gitto isn't code-signed, so Windows may say "Windows protected your PC": choose
   **More info**, then **Run anyway**.

It installs for your user only, without asking for administrator rights, in
`%LocalAppData%\Gitto`, and opens when it's done. Start it again from the **Gitto** shortcut in the
Start menu or on the desktop.

To uninstall it, open **Settings → Apps → Installed apps**, and choose **Uninstall** in Gitto's menu.

## macOS

1. Download `Gitto-darwin-arm64-<version>.zip`. It's for Apple Silicon Macs (M1 and later) only.
2. Open the zip to unpack it, and drag **Gitto** into your **Applications** folder.
3. Open Gitto. Since it isn't signed by Apple, macOS won't open it the first time, and says it can't
   verify it or that it's damaged. Either:
   - open **System Settings → Privacy & Security**, and choose **Open Anyway** next to the message
     about Gitto, or
   - run this in Terminal, then open Gitto again:

     ```sh
     xattr -dr com.apple.quarantine /Applications/Gitto.app
     ```

To uninstall it, move Gitto from Applications to the Trash.

## Arch Linux

1. Download `gitto-<version>-1-x86_64.pkg.tar.zst`.
2. Install it with pacman, which installs what it depends on (Git among them) too:

   ```sh
   sudo pacman -U ./gitto-*.pkg.tar.zst
   ```

Start Gitto from your app launcher, or with `gitto` in a terminal. To uninstall it, run
`sudo pacman -R gitto`.

The package also works on Arch-based distributions, such as CachyOS, EndeavourOS and Manjaro.

## Updating

The bottom of Gitto's window says which one you have and its version, like **Gitto v1.3.0** or
**Gitto Nightly v1.3.1-nightly29853462**. Compare it with the newest on the
[releases page](https://github.com/msvdaamen/gitto/releases). Updating keeps your data, such as the
repositories you've added.

### Windows

Gitto updates itself. It checks for a newer version when it starts and every hour, and downloads it
in the background, saying **Downloading update…** at the bottom of the window. Once it's downloaded,
choose **Restart to update to …** there to restart into it now; otherwise it's installed the next
time Gitto starts.

Gitto doesn't check the first time it runs after being installed, only an hour later or when it next
starts. To update it by hand, download the newer `Gitto-<version>.Setup.exe` and run it, as when
installing it.

### macOS

Gitto will update itself as on Windows once its builds are signed by Apple. Until then, update it by
hand:

1. Quit Gitto.
2. Download the newer `Gitto-darwin-arm64-<version>.zip`, and open it to unpack it.
3. Drag **Gitto** into your **Applications** folder, and choose **Replace**.
4. Open Gitto. macOS won't open the new version the first time either, so allow it again as in
   step 3 of [installing it](#macos).

### Arch Linux

pacman will update Gitto once it's on the AUR. Until then, download the newer
`gitto-<version>-1-x86_64.pkg.tar.zst` and install it the same way, which replaces the one you have:

```sh
sudo pacman -U ./gitto-*.pkg.tar.zst
```

Then quit Gitto and start it again.

### Switching between a release and Gitto Nightly

Gitto doesn't update from one to the other. To switch, install the other one as above, over the one
you have: they're the same app, so it replaces it and keeps your data.

On Windows, uninstall Gitto Nightly first when switching to a release. A nightly's version is newer
than the release before it, and the installer won't replace a newer version with an older one.

## Your data

Uninstalling Gitto leaves its data (the repositories you've added to it, not the repositories
themselves) where it was, for a later install to pick up. To remove it too, delete this folder:

| System  | Folder                                |
| ------- | ------------------------------------- |
| Windows | `%AppData%\Gitto`                     |
| macOS   | `~/Library/Application Support/Gitto` |
| Linux   | `~/.config/Gitto`                     |
