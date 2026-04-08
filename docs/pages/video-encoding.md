# Video encoding

### For Linux and Windows

You can download `pianopro-cli` from [releases](https://github.com/PolyMeilex/PianoPro/releases)

### For macOS

To encode video you need to install `rust` and `ffmpeg`.

Then compile the PianoPro CLI: `make build-recorder`

It will compile `pianopro-cli`, from now on it is used as a command line tool

To encode a test.mid file run `./target/release/pianopro-cli ./test.mid`

Video will be outputted to `./out` directory
