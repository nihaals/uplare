# Uplare

A CLI to easily and accurately keep track of installed software.

## Installation

### `cargo install`

```bash
cargo install uplare
```

### `cargo install` from Git

```bash
cargo install --git https://github.com/nihaals/uplare
```

### `cargo binstall`

```bash
cargo binstall uplare
```

### GitHub Releases

There are pre-compiled binaries available with [GitHub Releases](https://github.com/nihaals/uplare/releases). For example:

```bash
wget -O uplare "https://github.com/nihaals/uplare/releases/latest/download/uplare-cli-aarch64-apple-darwin"
chmod +x uplare
./uplare --help
```

## Getting started

### macOS

1. Install [Pkl](https://github.com/apple/pkl)
2. Create `mac.pkl`:

```pkl
// Replace with version from `uplare --version`
amends "package://github.com/nihaals/uplare/releases/download/v0.0.0/uplare-pkl@0.0.0#/MacOs.pkl"

mac {
  homebrew = new Homebrew {
    explicitlyInstalledFormulae {
      "ffmpeg"
    }
    nonAppCasks {
      "font-fira-code"
    }
  }
  apps {
    new ManualApp {
      name = "Wolfram"
      appPaths {
        "/Applications/Wolfram.app"
      }
    }
    new HomebrewCask {
      caskName = "visual-studio-code"
      appPaths {
        "/Applications/Visual Studio Code.app"
      }
    }
    new MacAppStoreApp {
      appStoreId = 497799835
      appPaths {
        "/Applications/Xcode.app"
      }
    }
  }
}
```

3. Run `pkl eval mac.pkl | uplare diff macos /dev/stdin`
4. Run `uplare --help`

### SteamOS

1. Install [Pkl](https://github.com/apple/pkl)
2. Create `deck.pkl`:

```pkl
// Replace with version from `uplare --version`
amends "package://github.com/nihaals/uplare/releases/download/v0.0.0/uplare-pkl@0.0.0#/SteamOs.pkl"
// Optional, only needed if you use `files`
import "package://github.com/nihaals/uplare/releases/download/v0.0.0/uplare-pkl@0.0.0#/FileCheck.pkl"

steamOs {
  hostname = "my-device"
  steamOsSettings {
    steamDeveloperMode = true
    chargeLimit = 80
  }
  steamSettings {
    twentyFourHourClock = true
    signIntoFriends = false
  }
  installedFlatpaks {
    "org.mozilla.firefox"
    "com.github.Matoking.protontricks"
  }
  decky = new Decky {
    settings {
      updateChannel = "stable"
      storeChannel = "default"
      deckyUpdateNotification = true
      pluginUpdateNotification = true
      developerMode = true
    }
    plugins {
      new { name = "Brightness Bar" }
      new {
        name = "HLTB for Deck"
        disabled = true
      }
    }
  }
  enabledSystemdUnits {
    "sshd.service"
    "plugin_loader.service"
  }
  desktop = new Listing {
    "Return.desktop"
    "steam.desktop"
  }
  kdePlasmaDock = new Listing {
    "applications:systemsettings.desktop"
    "applications:org.kde.discover.desktop"
    "preferred://filemanager"
    "preferred://browser"
  }
  files {
    new FileCheck.FileContainsStrings {
      path = "/etc/hosts"
      substrings = new {
        "127.0.0.1"
        "::1"
      }
    }
  }
}
```

3. Run `pkl eval deck.pkl | uplare diff steamos /dev/stdin`
4. Run `uplare --help`

## Supported properties

While there's no official documentation site yet, you can refer to the [Pkl module](./pkl/) or the [Rust `serde` types](./src/pkl_types/) for the platform you are using. There are also [Pkl examples](./pkl/examples/).

## Referencing Uplare in Pkl

You may prefer to create a `PklProject` so you can reference Uplare's Pkl modules more easily:

```pkl
amends "pkl:Project"

dependencies {
  ["uplare"] {
    // Replace with version from `uplare --version`
    local version = "0.0.0"
    uri = "package://github.com/nihaals/uplare/releases/download/v\(version)/uplare-pkl@\(version)"
  }
}
```

Which can be referenced like so:

```pkl
amends "@uplare/MacOs.pkl"
```

Note: You'll need to run `pkl project resolve` as suggested by Pkl before this works.

## Running without Pkl

If you would like to run `uplare` on a system without `pkl` installed, you can:

- Run `pkl eval` on another machine and copy the JSON or simply pipe it over SSH (e.g. `pkl eval deck.pkl | ssh deck 'uplare diff steamos /dev/stdin'`)
- Avoid Pkl entirely and create the system config JSON yourself by using the Pkl modules and `serde` types as reference or running `pkl eval` on examples
  - While Pkl is the only official way of generating a system config's JSON, validation is implemented for both the Pkl modules and in the CLI and there are tests to help avoid drift between the two
