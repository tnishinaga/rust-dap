# Raspberry Pi Pico 2 port

Raspberry Pi Pico 2（RP2350）用の CMSIS-DAP ファームウェアです。SWD と JTAG
はデフォルトで PIO を使い、`bitbang` feature を指定すると CPU による GPIO
制御に切り替わります。SWJ は bitbang で SWD/JTAG を実行時に切り替えます。

ビルドはこのディレクトリで実行してください。

```sh
# SWD（PIO）
cargo build --release --features swd

# JTAG（PIO）
cargo build --release --no-default-features --features jtag

# SWD（bitbang）
cargo build --release --no-default-features --features swd,bitbang

# SWJ
cargo build --release --no-default-features --features swj
```
