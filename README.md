# Uplare

Easily and accurately keep track of installed software.

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

1. [Install `pkl`](https://pkl-lang.org/main/current/pkl-cli/index.html#installation)
2. Create `mac.pkl`:

```pkl
// Replace with version from `uplare --version`
amends "package://github.com/nihaals/uplare/releases/download/v0.0.0/uplare-pkl@0.0.0#/MacOs.pkl"

mac {
  installHomebrew = true
  apps = new {
    new ManualApp {
      name = "Wolfram"
      appPaths = new {
        "/Applications/Wolfram.app"
      }
    }
    new HomebrewCask {
      caskName = "visual-studio-code"
      appPaths = new {
        "/Applications/Visual Studio Code.app"
      }
    }
    new MacAppStoreApp {
      appStoreId = 497799835
      appPaths = new {
        "/Applications/Xcode.app"
      }
    }
  }
}
```

3. Run `pkl eval mac.pkl | uplare diff macos /dev/stdin`
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
    uri = "package://github.com/nihaals/uplare/releases/download/v0.0.0/uplare-pkl@0.0.0"
  }
}
```

Which can be referenced like so:

```pkl
amends "@uplare/MacOs.pkl"
```

Note: You'll need to run `pkl project resolve` as suggested by Pkl before this works.

## Running without Pkl

If you would like to run `uplare` on a system without `pkl` installed, you can run `pkl eval` on another machine and transfer the JSON. You can also avoid Pkl entirely and create the system config JSON yourself by using the Pkl modules and `serde` types as reference or running `pkl eval` on examples. While Pkl is the only official way of generating a system config's JSON, validation is implemented for both the Pkl modules and in the CLI and there are tests to help avoid drift.
