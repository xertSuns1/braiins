# Overview [**MVP alpha**]

This is the root folder of bOSminer software. The project is intended as a
replacement for the existing *cgminer* software. However, it has been designed
from scratch and written in a more modern Rust programming language.

# Features

## Backend Agnostic Features

- native **Stratum V2** support. The miner can be tested against `v2.stratum.slushpool.com:3336`. Alternatively it can be tested in combination with
*V2->V1* [mining proxy](../stratum-proxy/README.md) running locally in your environment. 
- **toml** based persistent configuration, default path (`/etc/bosminer/bosminer.toml`) can be overridden on the command line. The configuration file is schema based, therefore the software would **complain** about **missing** or **unknown** configuration fields.
- **weighted pool switching** - User can specify multiple pools in the configuration and **bOSminer** will balance the hash rate across multiple pools. Currently it is not possible to specify weights for individual pools in the configuration nor on the command line.
- **cgminer** compatible *read-only* **API**
- **fan control** - user may specify a target temperature and the software will optimally control fan speed to reach the desired temperature. Alternatively, this mechanism can be overridden by a fixed fan speed.
- **temperature monitoring** - software periodically monitors temperatures of individual hash chains and issues a warning if temperature exceeds one of the configured levels - see `dangerous_temp` and `hot_temp` configuration options below.



## Antminer S9 Specific Features

- **AsicBoost** - enable/disable multi-mid-state hashing aka **AsicBoost**
- **per hash board** **voltage** and **frequency** configuration



## CGMiner-like API

CGMiner comes with many custom modifications to its API. We have chosen the basic subset of the upstream cgminer API and implemented it.

The following commands are recognized and provide useful information:

- `pools`
- `devs`
- `edevs`
- `summary`
- `config`
- `asccount`
- `asc`

The following commands are recognized but don't provide any useful information:

- `devdetails`
- `stats`
- `estats`
- `coin`
- `lcd`


## Example of Reading Pool Statistics

```
echo '{"command":"pools"}' | nc <YOUR_MINER_IP> 4028 | jq .
```

Example output:

```
{
  "STATUS": [
    {
      "STATUS": "S",
      "When": 1576573961,
      "Code": 7,
      "Msg": "1 Pool(s)",
      "Description": "bOSminer 0.1.0-a03f6e6"
    }
  ],
  "POOLS": [
    {
      "Accepted": 800,
      "Bad Work": 0,
      "Best Share": 8192,
      "Current Block Height": 0,
      "Current Block Version": 536870912,
      "Diff1 Shares": 64305,
      "Difficulty Accepted": 4147636,
      "Difficulty Rejected": 4878,
      "Difficulty Stale": 0,
      "Discarded": 0,
      "Get Failures": 0,
      "Getworks": 161,
      "Has GBT": false,
      "Has Stratum": true,
      "Has Vmask": true,
      "Last Share Difficulty": 4878,
      "Last Share Time": 1576573958,
      "Long Poll": "N",
      "POOL": 0,
      "Pool Rejected%": 0.11747100672026632,
      "Pool Stale%": 0,
      "Priority": 0,
      "Proxy": "",
      "Proxy Type": "",
      "Quota": 1,
      "Rejected": 1,
      "Remote Failures": 0,
      "Stale": 0,
      "Status": "Alive",
      "Stratum Active": true,
      "Stratum Difficulty": 4878,
      "Stratum URL": "v2.stratum.slushpool.com:3336",
      "URL": "v2.stratum.slushpool.com:3336",
      "User": "YOURUSERNAME.WORKERNAME",
      "Work Difficulty": 4878,
      "Works": 5267024
    }
  ],
  "id": 1
}
```


# Configuration and Command Line Options

Currently the software can be configured in 2 ways - sorted by priority:

- command line options
- configuration file


## Command Line Options

Command line options always override any configuration values. Currently, not all configuration aspects are covered by command line options. The easiest way to find out about supported command line options is to run:

`bosminer --help`

Expected output for an S9:

```
USAGE:
    bosminer-am1-s9 [FLAGS] [OPTIONS]

FLAGS:
        --disable-asic-boost    Disable ASIC boost (use just one mid-state)
    -h, --help                  Prints help information
    -V, --version               Prints version information

OPTIONS:
        --config <config>               Set config file path
        --frequency <frequency>         Set chip frequency (in MHz)
    -p, --pool <HOSTNAME:PORT>          Address the stratum V2 server
    -u, --user <USERNAME.WORKERNAME>    Specify user and worker name
        --voltage <voltage>             Set chip voltage (in volts)
```


## Configuration File

Below is an example configuration file for Antminer S9 with explanatory comments of all options.

```
# bOSminer configuration

# This option tells bosminer that we are using "alpha" version of config
# file format. It's mandatory.
config_version = "alpha"

asic_boost = true

# Frequency and voltage are set for all chains
frequency = 650
voltage = 9

# Override frequency and voltage for chain 8
[chain.8]
frequency = 560
voltage = 8.9

# Override voltage for chain 7
[chain.7]
voltage = 8.8

# Configuration of temperature control
#
# If you want to disable temperature control, then comment-out the following
# section.
[temperature]
dangerous_temp = 105.0
hot_temp = 95.0

# Configuration of fans
#
# If you want to disable fan control completely, then comment-out this section.
[fans]
# You can either control fans with target temperature (in Celsius):
temperature = 75
# Or you can set fixed fan speed (in %):
#speed = 70
# but these two options are mutually exclusive.

# Set this to minimum number of fans required for bOSminer to run
min_fans = 1

# Declares pools. This section can be repeated.
[[pool]]
url = "v2.stratum.slushpool.com:3336"
user = "YOURUSERNAME.WORKERNAME"
```


# Further Work

Below is a list of use cases that we plan on implementing in the Beta MVP phase of the project:

- fail-over between pools and upstream connection handling
- full writable part of cgminer compatible API
- stratum V1 support
- linear frequency scaling of factory calibration frequencies


# Developer Information

From here on, you can read if you are interested in building bOSminer from sources.

## Directory Layout

The project is structured as a set of Rust crates:

- [bosminer](bosminer/README.md) - generic part of the software, you should not need to build this crate separately unless you are a developer
- [bosminer-erupter](bosminer-erupter/README.md) - Block Erupter support is
  provided for development purpose - it serves as a test bed for bosminer code base
- [bosminer-am1-s9](bosminer-am1-s9/README.md) - Antminer S9 application

Below are generic guidelines how to setup your build environment, after that,
you can follow specific details of each backend.

## Build

We assume you have setup the generic part of the build environment as specified in the [generic documentation](../../README.md)
Follow the steps below and proceed to the sub-directory of the selected backend (e.g. [bosminer-am1-s9](bosminer-am1-s9/README.md))

### Prerequisites

- python prerequisites
- svd2rust
- rustfmt-preview

### Python

```
mkvirtualenv --python=/usr/bin/python3 bosminer
python3 -m pip3 install -r scripts/requirements.txt
```

### Cargo/Rust tools


```shell
cargo install svd2rust
cargo install form
rustup component add rustfmt
```


## Remote Targets

The actual mining devices are considered as *remote targets*. That means you can tell cargo to run the mining application or its tests remotely on a device that is already running an image of Braiins OS.

In order to perform the steps below you have to descent to a specific target folder (am1-s9 is the only remote target for the time being)

```shell
cd bosminer-am1-s9
```

### Authentication Notes
Authentication method "none" (no password) DOES NOT WORK.

For authentication, you MUST use either "publickey" authentication or "password" (although beware, this is not confirmed to be working from all sources).

NOTE: for the time being, the key MUST NOT have a passphrase. Therefore, only
temporary development key should be used.

### Running the Test suite Remotely
```shell
cargo test -- --hostname <HOSTNAME>
```

This runs all tests on remote machine specified by argument *--hostname*. It is possible to omit this additional parameter
by providing a configuration file *Test.toml* stored in crate root directory:

```toml
[remote]
hostname = "<HOSTNAME>"
```

It is possible to call the following command with these settings

```shell
cargo test
```

## Running the bOSminer

The miner can be run on host target or on remote one depending on backend and supported targets. Again, the *Test.toml* allows remote hostname specification so that we don't have to specify every time the hostname on the command line.

```shell
# run miner on host target (without runner)
cargo run -- --pool <POOLV2PROXY> --user <POOLUSER> [--disable-asic-boost]

# run miner on remote target (using runner written in python)
cargo run -- [--hostname <HOSTNAME>] -- --pool <POOLV2PROXY> --user <POOLUSER> [--disable-asic-boost]
```

The `--disable-asic-boost` option disables ASIC boost on S9 targets (ASIC boost is enabled by default on S9) - this is achieved by changing the number of midstates sent to chips from 4 to 1. This option does nothing on eruptor target.


## Logging

To enable more verbose logging/tracing, set `RUST_LOG` environment variable:

- enable all tracing: `RUST_LOG=trace cargo run ...`
- quiet mode, print just errors: `RUST_LOG=error cargo run ...`
