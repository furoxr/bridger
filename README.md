# Darwinia Bridger

[![CI](https://github.com/darwinia-network/bridger/workflows/CI/badge.svg)](https://github.com/darwinia-network/bridger/actions)
[![release](https://img.shields.io/github/v/release/darwinia-network/bridger)](https://github.com/darwinia-network/bridger/releases/latest)
[![doc](https://img.shields.io/badge/current-docs-brightgreen.svg)](https://github.com/darwinia-network/bridger/)
[![downloads](https://img.shields.io/crates/d/darwinia-bridger.svg)](https://github.com/darwinia-network/bridger/releases/)
[![license](https://img.shields.io/github/license/darwinia-network/bridger)](https://choosealicense.com/licenses/gpl/)

Relayers (aka. Bridgers) in Darwinia Network are offchain worker clients which help relay the headers and messages between source chains and target chains, they works between two chains and requires RPC access of two chains.

Darwinia Bridger (this repo) is an implementation of relayer client written in Rust.

## Installation

### Download from GitHub releases

Download the binary from [latest release](https://github.com/darwinia-network/bridger/releases/latest).

### Pull the Docker image

```bash
docker pull quay.io/darwinia-network/bridger:<VERSION>
```

### Install using cargo

> ⚠️ Because of [#76](https://github.com/darwinia-network/bridger/issues/76) we don't publish darwinia-bridger to crates.io, the [darwinia-bridger](https://crates.io/crates/darwinia-bridger) currently in crates is outdated.

If you want to install use cargo, you can run this command, the first you need found last version in [latest release](https://github.com/darwinia-network/bridger/releases/latest).

```bash
cargo install darwinia-bridger --git https://github.com/darwinia-network/bridger --tag <VERSION>
```

### Build from source

> Notice: please use last `nightly` toolchain. You might want to change the default toolchain using `rustup default nightly`, some bridger need wasm support, you can run `rustup target add wasm32-unknown-unknown`.

```bash
git clone https://github.com/darwinia-network/bridger.git
cd bridger/
cargo build --release
```

## Configuration

The currently darwinia-bridger supports these bridge, the config you can click docs link

| type                | bridge            | doc                                                  |
| ------------------- | ----------------- | ---------------------------------------------------- |
| substrate-ethereum  | darwinia-ethereum | [Guide](./task/task-darwinia-ethereum/docs/Guide.md) |
| substrate-substrate | pangolin-millau   | [Guide](./task/task-pangolin-millau/docs/Guide.md)   |

## Usage

The latest help texts are also available in `bridger --help`.

```
$ bridger
bridger 0.4.0
Darwinia bridger

USAGE:
    bridger <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    help      Prints this message or the help of the given subcommand(s)
    kv        The bridge kv db storage operation
    server    Start bridger server
    task      Task manager
```

No matter what bridge you wish to start, the first, you must run `bridger server` to start bridger server.

```text
$ bridger server --help
bridger-server 0.4.0
Start bridger server

USAGE:
    bridger server [OPTIONS]

FLAGS:
        --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --base-path <base-path>    The bridger config or data base path
    -h, --host <host>              Bridger service listen host [default: 127.0.0.1]
    -p, --port <port>              Bridger service listen port [default: 1098]
```

- `--base-path` all of bridger data, like task config, database will store in here, the default path is `$HOME/.bridger`
- `--host` `--port` the bridger server host and port.
