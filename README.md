# checking
[<img alt="github" src="https://img.shields.io/badge/github-wc0x2c/checking-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20">](https://github.com/wc0x2c/checking)
[<img alt="crates.io" src="https://img.shields.io/crates/v/checking-cli.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20">](https://crates.io/crates/checking-cli)
[<img alt="build status" src="https://img.shields.io/github/actions/workflow/status/wc0x2c/checking/main.yml?branch=master&style=for-the-badge" height="20">](https://github.com/wc0x2c/checking/actions?query=branch%3Amaster)

> [!NOTE]
> Coding agents wrote much of this code.

An iRacing telemetry app and overlay. It shows the fuel strategy, the relative
and a radar of the cars alongside.

The app reads the iRacing shared memory on the Windows machine that runs
iRacing. It sends the telemetry over TCP to port 5555, so the GUI can run on
the same machine or on a second machine.

## Installation
Either build from published source in crates.io.
```
$ cargo install checking-cli --locked
```

Or download the Windows binary from [github releases](https://github.com/wc0x2c/checking/releases).

## Crates
You can use the libraries to read iRacing telemetry in your own tool.

| Crate | Contents |
|-------|----------|
| [`checking-protocol`](https://crates.io/crates/checking-protocol) | Wire format of the telemetry frames that the sender sends to the GUI |
| [`checking-iracingio`](https://crates.io/crates/checking-iracingio) | Read iRacing telemetry from shared memory and send it over TCP |
| [`checking-fuel`](https://crates.io/crates/checking-fuel) | Fuel strategy calculations |

## Development
All patches/merge requests are welcome!

```
just quick-test              # the unit tests
just lint                    # rustfmt and clippy
just release-assets v0.1.0   # the Windows zip of a release, with cross
```

## License
MIT or Apache-2.0, at your option.
