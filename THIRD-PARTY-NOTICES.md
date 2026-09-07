# Third-Party Notices

Spaceadom is built with the Rust crates and npm packages listed below.
Generated from `cargo metadata`/`cargo license` (Rust, non-dev/build
dependencies) and `npx license-checker --production` (npm). Each
package remains under its own licence — nothing in Spaceadom's own
LICENSE changes the terms below.

Total: **571** packages (569 Rust crates, 2 npm packages).

**Licences that require attribution if you redistribute a build containing
them** (you keep the notice, you don't need to ask): MIT, Apache-2.0,
BSD-2-Clause, BSD-3-Clause, ISC, Zlib, MPL-2.0. This file exists to
satisfy that requirement in one place. Licences like Unicode-3.0/CC0-1.0/
0BSD are effectively public-domain-equivalent and impose no practical
obligation.

**Notes on the less common entries:**
- **MPL-2.0** (`cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`,
  `selectors`) — weak copyleft that only attaches to modifications of the
  MPL-licensed files themselves. Spaceadom uses these crates unmodified.
- **CDLA-Permissive-2.0** (`webpki-root-certs`, `webpki-roots`) — a permissive
  data licence covering the bundled Mozilla CA root certificate list, not
  code. No obligations beyond keeping the notice.
- **Anything listing LGPL-2.1-or-later** (`r-efi`) offers it as one of three
  alternatives (`Apache-2.0 OR LGPL-2.1-or-later OR MIT`); Spaceadom relies on
  the Apache-2.0/MIT option, which is what this project is built under.
- **UNLICENSED** is not present in this list — Spaceadom's own package is
  excluded (see LICENSE for its terms).

---

## Apache-2.0 OR MIT (352)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| @tauri-apps/api | 2.11.1 | npm | [link](https://github.com/tauri-apps/tauri) |
| addr2line | 0.25.1 | crates.io | [link](https://github.com/gimli-rs/addr2line) |
| android_system_properties | 0.1.5 | crates.io | [link](https://github.com/nical/android_system_properties) |
| anyhow | 1.0.103 | crates.io | [link](https://github.com/dtolnay/anyhow) |
| arbitrary | 1.4.2 | crates.io | [link](https://github.com/rust-fuzz/arbitrary/) |
| arc-swap | 1.9.2 | crates.io | [link](https://github.com/vorner/arc-swap) |
| async-broadcast | 0.7.2 | crates.io | [link](https://github.com/smol-rs/async-broadcast) |
| async-channel | 2.5.0 | crates.io | [link](https://github.com/smol-rs/async-channel) |
| async-executor | 1.14.0 | crates.io | [link](https://github.com/smol-rs/async-executor) |
| async-io | 2.6.0 | crates.io | [link](https://github.com/smol-rs/async-io) |
| async-lock | 3.4.2 | crates.io | [link](https://github.com/smol-rs/async-lock) |
| async-process | 2.5.0 | crates.io | [link](https://github.com/smol-rs/async-process) |
| async-recursion | 1.1.1 | crates.io | [link](https://github.com/dcchut/async-recursion) |
| async-signal | 0.2.14 | crates.io | [link](https://github.com/smol-rs/async-signal) |
| async-task | 4.7.1 | crates.io | [link](https://github.com/smol-rs/async-task) |
| async-trait | 0.1.89 | crates.io | [link](https://github.com/dtolnay/async-trait) |
| atomic-waker | 1.1.2 | crates.io | [link](https://github.com/smol-rs/atomic-waker) |
| backtrace | 0.3.76 | crates.io | [link](https://github.com/rust-lang/backtrace-rs) |
| base64 | 0.21.7 | crates.io | [link](https://github.com/marshallpierce/rust-base64) |
| base64 | 0.22.1 | crates.io | [link](https://github.com/marshallpierce/rust-base64) |
| base64 | 0.23.1 | crates.io | [link](https://github.com/marshallpierce/rust-base64) |
| bit-set | 0.8.0 | crates.io | [link](https://github.com/contain-rs/bit-set) |
| bit-vec | 0.8.0 | crates.io | [link](https://github.com/contain-rs/bit-vec) |
| bitflags | 1.3.2 | crates.io | [link](https://github.com/bitflags/bitflags) |
| bitflags | 2.13.0 | crates.io | [link](https://github.com/bitflags/bitflags) |
| block-buffer | 0.10.4 | crates.io | [link](https://github.com/RustCrypto/utils) |
| blocking | 1.6.2 | crates.io | [link](https://github.com/smol-rs/blocking) |
| bs58 | 0.5.1 | crates.io | [link](https://github.com/Nullus157/bs58-rs) |
| bumpalo | 3.20.3 | crates.io | [link](https://github.com/fitzgen/bumpalo) |
| camino | 1.2.4 | crates.io | [link](https://github.com/camino-rs/camino) |
| cargo-platform | 0.1.9 | crates.io | [link](https://github.com/rust-lang/cargo) |
| cesu8 | 1.1.0 | crates.io | [link](https://github.com/emk/cesu8-rs) |
| cfg-if | 1.0.4 | crates.io | [link](https://github.com/rust-lang/cfg-if) |
| chacha20 | 0.10.1 | crates.io | [link](https://github.com/RustCrypto/stream-ciphers) |
| chrono | 0.4.45 | crates.io | [link](https://github.com/chronotope/chrono) |
| concurrent-queue | 2.5.0 | crates.io | [link](https://github.com/smol-rs/concurrent-queue) |
| cookie | 0.18.1 | crates.io | [link](https://github.com/SergioBenitez/cookie-rs) |
| core-foundation | 0.9.4 | crates.io | [link](https://github.com/servo/core-foundation-rs) |
| core-foundation | 0.10.1 | crates.io | [link](https://github.com/servo/core-foundation-rs) |
| core-foundation-sys | 0.8.7 | crates.io | [link](https://github.com/servo/core-foundation-rs) |
| core-graphics | 0.25.0 | crates.io | [link](https://github.com/servo/core-foundation-rs) |
| core-graphics-types | 0.2.0 | crates.io | [link](https://github.com/servo/core-foundation-rs) |
| cpufeatures | 0.2.17 | crates.io | [link](https://github.com/RustCrypto/utils) |
| cpufeatures | 0.3.0 | crates.io | [link](https://github.com/RustCrypto/utils) |
| crc32fast | 1.5.0 | crates.io | [link](https://github.com/srijs/rust-crc32fast) |
| crossbeam-channel | 0.5.16 | crates.io | [link](https://github.com/crossbeam-rs/crossbeam) |
| crossbeam-utils | 0.8.22 | crates.io | [link](https://github.com/crossbeam-rs/crossbeam) |
| crypto-common | 0.1.7 | crates.io | [link](https://github.com/RustCrypto/traits) |
| ctor | 0.8.0 | crates.io | [link](https://github.com/mmastrac/rust-ctor) |
| ctor-proc-macro | 0.0.7 | crates.io | [link](https://github.com/mmastrac/rust-ctor) |
| dbus | 0.9.12 | crates.io | [link](https://github.com/diwic/dbus-rs) |
| deranged | 0.5.8 | crates.io | [link](https://github.com/jhpratt/deranged) |
| derive_arbitrary | 1.4.2 | crates.io | [link](https://github.com/rust-fuzz/arbitrary) |
| digest | 0.10.7 | crates.io | [link](https://github.com/RustCrypto/traits) |
| dirs | 4.0.0 | crates.io | [link](https://github.com/soc/dirs-rs) |
| dirs | 6.0.0 | crates.io | [link](https://github.com/soc/dirs-rs) |
| dirs-sys | 0.3.7 | crates.io | [link](https://github.com/dirs-dev/dirs-sys-rs) |
| dirs-sys | 0.5.0 | crates.io | [link](https://github.com/dirs-dev/dirs-sys-rs) |
| displaydoc | 0.2.6 | crates.io | [link](https://github.com/yaahc/displaydoc) |
| dtoa | 1.0.11 | crates.io | [link](https://github.com/dtolnay/dtoa) |
| dtor | 0.3.0 | crates.io | [link](https://github.com/mmastrac/rust-ctor) |
| dtor-proc-macro | 0.0.6 | crates.io | [link](https://github.com/mmastrac/rust-ctor) |
| dyn-clone | 1.0.20 | crates.io | [link](https://github.com/dtolnay/dyn-clone) |
| embed_plist | 1.2.2 | crates.io | [link](https://github.com/nvzqz/embed-plist-rs) |
| enumflags2 | 0.7.12 | crates.io | [link](https://github.com/meithecatte/enumflags2) |
| enumflags2_derive | 0.7.12 | crates.io | [link](https://github.com/meithecatte/enumflags2) |
| equivalent | 1.0.2 | crates.io | [link](https://github.com/indexmap-rs/equivalent) |
| erased-serde | 0.4.10 | crates.io | [link](https://github.com/dtolnay/erased-serde) |
| errno | 0.3.14 | crates.io | [link](https://github.com/lambda-fairy/rust-errno) |
| event-listener | 5.4.1 | crates.io | [link](https://github.com/smol-rs/event-listener) |
| event-listener-strategy | 0.5.4 | crates.io | [link](https://github.com/smol-rs/event-listener-strategy) |
| fastrand | 2.4.1 | crates.io | [link](https://github.com/smol-rs/fastrand) |
| fdeflate | 0.3.7 | crates.io | [link](https://github.com/image-rs/fdeflate) |
| field-offset | 0.3.6 | crates.io | [link](https://github.com/Diggsey/rust-field-offset) |
| filetime | 0.2.29 | crates.io | [link](https://github.com/alexcrichton/filetime) |
| flate2 | 1.1.9 | crates.io | [link](https://github.com/rust-lang/flate2-rs) |
| fnv | 1.0.7 | crates.io | [link](https://github.com/servo/rust-fnv) |
| foreign-types | 0.5.0 | crates.io | [link](https://github.com/sfackler/foreign-types) |
| foreign-types-macros | 0.2.3 | crates.io | [link](https://github.com/sfackler/foreign-types) |
| foreign-types-shared | 0.3.1 | crates.io | [link](https://github.com/sfackler/foreign-types) |
| form_urlencoded | 1.2.2 | crates.io | [link](https://github.com/servo/rust-url) |
| futures-channel | 0.3.32 | crates.io | [link](https://github.com/rust-lang/futures-rs) |
| futures-core | 0.3.32 | crates.io | [link](https://github.com/rust-lang/futures-rs) |
| futures-executor | 0.3.32 | crates.io | [link](https://github.com/rust-lang/futures-rs) |
| futures-io | 0.3.32 | crates.io | [link](https://github.com/rust-lang/futures-rs) |
| futures-lite | 2.6.1 | crates.io | [link](https://github.com/smol-rs/futures-lite) |
| futures-macro | 0.3.32 | crates.io | [link](https://github.com/rust-lang/futures-rs) |
| futures-sink | 0.3.32 | crates.io | [link](https://github.com/rust-lang/futures-rs) |
| futures-task | 0.3.32 | crates.io | [link](https://github.com/rust-lang/futures-rs) |
| futures-util | 0.3.32 | crates.io | [link](https://github.com/rust-lang/futures-rs) |
| getrandom | 0.2.17 | crates.io | [link](https://github.com/rust-random/getrandom) |
| getrandom | 0.3.4 | crates.io | [link](https://github.com/rust-random/getrandom) |
| getrandom | 0.4.3 | crates.io | [link](https://github.com/rust-random/getrandom) |
| gimli | 0.32.3 | crates.io | [link](https://github.com/gimli-rs/gimli) |
| glob | 0.3.3 | crates.io | [link](https://github.com/rust-lang/glob) |
| hashbrown | 0.12.3 | crates.io | [link](https://github.com/rust-lang/hashbrown) |
| hashbrown | 0.17.1 | crates.io | [link](https://github.com/rust-lang/hashbrown) |
| heck | 0.4.1 | crates.io | [link](https://github.com/withoutboats/heck) |
| heck | 0.5.0 | crates.io | [link](https://github.com/withoutboats/heck) |
| hermit-abi | 0.5.2 | crates.io | [link](https://github.com/hermit-os/hermit-rs) |
| hex | 0.4.3 | crates.io | [link](https://github.com/KokaKiwi/rust-hex) |
| html5ever | 0.38.0 | crates.io | [link](https://github.com/servo/html5ever) |
| http | 1.4.2 | crates.io | [link](https://github.com/hyperium/http) |
| httparse | 1.10.1 | crates.io | [link](https://github.com/seanmonstar/httparse) |
| httpdate | 1.0.3 | crates.io | [link](https://github.com/pyfisch/httpdate) |
| iana-time-zone | 0.1.65 | crates.io | [link](https://github.com/strawlab/iana-time-zone) |
| iana-time-zone-haiku | 0.1.2 | crates.io | [link](https://github.com/strawlab/iana-time-zone) |
| ident_case | 1.0.1 | crates.io | [link](https://github.com/TedDriggs/ident_case) |
| idna | 1.1.0 | crates.io | [link](https://github.com/servo/rust-url/) |
| idna_adapter | 1.2.2 | crates.io | [link](https://github.com/hsivonen/idna_adapter) |
| image | 0.25.10 | crates.io | [link](https://github.com/image-rs/image) |
| indexmap | 1.9.3 | crates.io | [link](https://github.com/bluss/indexmap) |
| indexmap | 2.14.0 | crates.io | [link](https://github.com/indexmap-rs/indexmap) |
| ipnet | 2.12.0 | crates.io | [link](https://github.com/krisprice/ipnet) |
| itoa | 1.0.18 | crates.io | [link](https://github.com/dtolnay/itoa) |
| jni | 0.21.1 | crates.io | [link](https://github.com/jni-rs/jni-rs) |
| jni | 0.22.4 | crates.io | [link](https://github.com/jni-rs/jni-rs) |
| jni-macros | 0.22.4 | crates.io | [link](https://github.com/jni-rs/jni-rs) |
| jni-sys | 0.3.1 | crates.io | [link](https://github.com/jni-rs/jni-sys) |
| jni-sys | 0.4.1 | crates.io | [link](https://github.com/jni-rs/jni-sys) |
| jni-sys-macros | 0.4.1 | crates.io | [link](https://github.com/jni-rs/jni-sys) |
| js-sys | 0.3.103 | crates.io | [link](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys) |
| json-patch | 3.0.1 | crates.io | [link](https://github.com/idubrov/json-patch) |
| jsonptr | 0.6.3 | crates.io | [link](https://github.com/chanced/jsonptr) |
| keyboard-types | 0.7.0 | crates.io | [link](https://github.com/pyfisch/keyboard-types) |
| libappindicator | 0.9.0 | crates.io | — |
| libappindicator-sys | 0.9.0 | crates.io | — |
| libc | 0.2.186 | crates.io | [link](https://github.com/rust-lang/libc) |
| libdbus-sys | 0.2.7 | crates.io | [link](https://github.com/diwic/dbus-rs) |
| lock_api | 0.4.14 | crates.io | [link](https://github.com/Amanieu/parking_lot) |
| log | 0.4.33 | crates.io | [link](https://github.com/rust-lang/log) |
| log-mdc | 0.1.0 | crates.io | [link](https://github.com/sfackler/rust-log-mdc) |
| log4rs | 1.4.0 | crates.io | [link](https://github.com/estk/log4rs) |
| mac-notification-sys | 0.6.15 | crates.io | [link](https://github.com/h4llow3En/mac-notification-sys) |
| markup5ever | 0.38.0 | crates.io | [link](https://github.com/servo/html5ever) |
| mime | 0.3.17 | crates.io | [link](https://github.com/hyperium/mime) |
| muda | 0.19.3 | crates.io | [link](https://github.com/tauri-apps/muda) |
| ndk | 0.9.0 | crates.io | [link](https://github.com/rust-mobile/ndk) |
| ndk-sys | 0.6.0+11769913 | crates.io | [link](https://github.com/rust-mobile/ndk) |
| notify-rust | 4.18.0 | crates.io | [link](https://github.com/hoodie/notify-rust) |
| num-conv | 0.2.2 | crates.io | [link](https://github.com/jhpratt/num-conv) |
| num-traits | 0.2.19 | crates.io | [link](https://github.com/rust-num/num-traits) |
| object | 0.37.3 | crates.io | [link](https://github.com/gimli-rs/object) |
| once_cell | 1.21.4 | crates.io | [link](https://github.com/matklad/once_cell) |
| openssl-probe | 0.2.1 | crates.io | [link](https://github.com/rustls/openssl-probe) |
| ordered-stream | 0.2.0 | crates.io | [link](https://github.com/danieldg/ordered-stream) |
| osakit | 0.3.1 | crates.io | [link](https://github.com/mdevils/rust-osakit) |
| parking | 2.2.1 | crates.io | [link](https://github.com/smol-rs/parking) |
| parking_lot | 0.12.5 | crates.io | [link](https://github.com/Amanieu/parking_lot) |
| parking_lot_core | 0.9.12 | crates.io | [link](https://github.com/Amanieu/parking_lot) |
| percent-encoding | 2.3.2 | crates.io | [link](https://github.com/servo/rust-url/) |
| pin-project-lite | 0.2.17 | crates.io | [link](https://github.com/taiki-e/pin-project-lite) |
| piper | 0.2.5 | crates.io | [link](https://github.com/smol-rs/piper) |
| png | 0.17.16 | crates.io | [link](https://github.com/image-rs/image-png) |
| png | 0.18.1 | crates.io | [link](https://github.com/image-rs/image-png) |
| polling | 3.11.0 | crates.io | [link](https://github.com/smol-rs/polling) |
| powerfmt | 0.2.0 | crates.io | [link](https://github.com/jhpratt/powerfmt) |
| ppv-lite86 | 0.2.21 | crates.io | [link](https://github.com/cryptocorrosion/cryptocorrosion) |
| proc-macro-crate | 1.3.1 | crates.io | [link](https://github.com/bkchr/proc-macro-crate) |
| proc-macro-crate | 2.0.2 | crates.io | [link](https://github.com/bkchr/proc-macro-crate) |
| proc-macro-crate | 3.5.0 | crates.io | [link](https://github.com/bkchr/proc-macro-crate) |
| proc-macro-error | 1.0.4 | crates.io | [link](https://gitlab.com/CreepySkeleton/proc-macro-error) |
| proc-macro-error-attr | 1.0.4 | crates.io | [link](https://gitlab.com/CreepySkeleton/proc-macro-error) |
| proc-macro2 | 1.0.106 | crates.io | [link](https://github.com/dtolnay/proc-macro2) |
| quinn | 0.11.11 | crates.io | [link](https://github.com/quinn-rs/quinn) |
| quinn-proto | 0.11.17 | crates.io | [link](https://github.com/quinn-rs/quinn) |
| quinn-udp | 0.5.15 | crates.io | [link](https://github.com/quinn-rs/quinn) |
| quote | 1.0.46 | crates.io | [link](https://github.com/dtolnay/quote) |
| rand | 0.9.4 | crates.io | [link](https://github.com/rust-random/rand) |
| rand | 0.10.2 | crates.io | [link](https://github.com/rust-random/rand) |
| rand_chacha | 0.9.0 | crates.io | [link](https://github.com/rust-random/rand) |
| rand_core | 0.9.5 | crates.io | [link](https://github.com/rust-random/rand) |
| rand_core | 0.10.1 | crates.io | [link](https://github.com/rust-random/rand_core) |
| rand_pcg | 0.10.2 | crates.io | [link](https://github.com/rust-random/rngs) |
| ref-cast | 1.0.25 | crates.io | [link](https://github.com/dtolnay/ref-cast) |
| ref-cast-impl | 1.0.25 | crates.io | [link](https://github.com/dtolnay/ref-cast) |
| regex | 1.12.4 | crates.io | [link](https://github.com/rust-lang/regex) |
| regex-automata | 0.4.14 | crates.io | [link](https://github.com/rust-lang/regex) |
| regex-syntax | 0.8.11 | crates.io | [link](https://github.com/rust-lang/regex) |
| reqwest | 0.13.4 | crates.io | [link](https://github.com/seanmonstar/reqwest) |
| rustc-demangle | 0.1.28 | crates.io | [link](https://github.com/rust-lang/rustc-demangle) |
| rustc-hash | 2.1.3 | crates.io | [link](https://github.com/rust-lang/rustc-hash) |
| rustls-pki-types | 1.15.1 | crates.io | [link](https://github.com/rustls/pki-types) |
| rustls-platform-verifier | 0.7.0 | crates.io | [link](https://github.com/rustls/rustls-platform-verifier) |
| rustls-platform-verifier-android | 0.1.1 | crates.io | [link](https://github.com/rustls/rustls-platform-verifier) |
| rustversion | 1.0.23 | crates.io | [link](https://github.com/dtolnay/rustversion) |
| scopeguard | 1.2.0 | crates.io | [link](https://github.com/bluss/scopeguard) |
| security-framework | 3.7.0 | crates.io | [link](https://github.com/kornelski/rust-security-framework) |
| security-framework-sys | 2.17.0 | crates.io | [link](https://github.com/kornelski/rust-security-framework) |
| semver | 1.0.28 | crates.io | [link](https://github.com/dtolnay/semver) |
| serde | 1.0.228 | crates.io | [link](https://github.com/serde-rs/serde) |
| serde_core | 1.0.228 | crates.io | [link](https://github.com/serde-rs/serde) |
| serde_derive | 1.0.228 | crates.io | [link](https://github.com/serde-rs/serde) |
| serde_derive_internals | 0.29.1 | crates.io | [link](https://github.com/serde-rs/serde) |
| serde_json | 1.0.150 | crates.io | [link](https://github.com/serde-rs/json) |
| serde_repr | 0.1.20 | crates.io | [link](https://github.com/dtolnay/serde-repr) |
| serde_spanned | 0.6.9 | crates.io | [link](https://github.com/toml-rs/toml) |
| serde_spanned | 1.1.1 | crates.io | [link](https://github.com/toml-rs/toml) |
| serde_with | 3.21.0 | crates.io | [link](https://github.com/jonasbb/serde_with/) |
| serde_with_macros | 3.21.0 | crates.io | [link](https://github.com/jonasbb/serde_with/) |
| serde-untagged | 0.1.9 | crates.io | [link](https://github.com/dtolnay/serde-untagged) |
| serialize-to-javascript | 0.1.2 | crates.io | [link](https://github.com/chippers/serialize-to-javascript) |
| serialize-to-javascript-impl | 0.1.2 | crates.io | [link](https://github.com/chippers/serialize-to-javascript) |
| servo_arc | 0.4.3 | crates.io | [link](https://github.com/servo/stylo) |
| sha2 | 0.10.9 | crates.io | [link](https://github.com/RustCrypto/hashes) |
| signal-hook | 0.3.18 | crates.io | [link](https://github.com/vorner/signal-hook) |
| signal-hook-registry | 1.4.8 | crates.io | [link](https://github.com/vorner/signal-hook) |
| simd_cesu8 | 1.2.0 | crates.io | [link](https://github.com/seancroach/simd_cesu8) |
| simdutf8 | 0.1.5 | crates.io | [link](https://github.com/rusticstuff/simdutf8) |
| siphasher | 1.0.3 | crates.io | [link](https://github.com/jedisct1/rust-siphash) |
| smallvec | 1.15.2 | crates.io | [link](https://github.com/servo/rust-smallvec) |
| socket2 | 0.6.4 | crates.io | [link](https://github.com/rust-lang/socket2) |
| softbuffer | 0.4.8 | crates.io | [link](https://github.com/rust-windowing/softbuffer) |
| stable_deref_trait | 1.2.1 | crates.io | [link](https://github.com/storyyeller/stable_deref_trait) |
| string_cache | 0.9.0 | crates.io | [link](https://github.com/servo/string-cache) |
| swift-rs | 1.0.7 | crates.io | [link](https://github.com/Brendonovich/swift-rs) |
| syn | 1.0.109 | crates.io | [link](https://github.com/dtolnay/syn) |
| syn | 2.0.118 | crates.io | [link](https://github.com/dtolnay/syn) |
| system-configuration | 0.7.0 | crates.io | [link](https://github.com/mullvad/system-configuration-rs) |
| system-configuration-sys | 0.6.0 | crates.io | [link](https://github.com/mullvad/system-configuration-rs) |
| tao-macros | 0.1.3 | crates.io | [link](https://github.com/tauri-apps/tao) |
| tar | 0.4.46 | crates.io | [link](https://github.com/composefs/tar-rs) |
| tauri | 2.11.5 | crates.io | [link](https://github.com/tauri-apps/tauri) |
| tauri-codegen | 2.6.3 | crates.io | [link](https://github.com/tauri-apps/tauri) |
| tauri-macros | 2.6.3 | crates.io | [link](https://github.com/tauri-apps/tauri) |
| tauri-plugin-autostart | 2.5.1 | crates.io | [link](https://github.com/tauri-apps/plugins-workspace) |
| tauri-plugin-dialog | 2.7.1 | crates.io | [link](https://github.com/tauri-apps/plugins-workspace) |
| tauri-plugin-fs | 2.5.1 | crates.io | [link](https://github.com/tauri-apps/plugins-workspace) |
| tauri-plugin-notification | 2.3.3 | crates.io | [link](https://github.com/tauri-apps/plugins-workspace) |
| tauri-plugin-opener | 2.5.4 | crates.io | [link](https://github.com/tauri-apps/plugins-workspace) |
| tauri-plugin-shell | 2.3.5 | crates.io | [link](https://github.com/tauri-apps/plugins-workspace) |
| tauri-plugin-single-instance | 2.4.2 | crates.io | [link](https://github.com/tauri-apps/plugins-workspace) |
| tauri-plugin-updater | 2.11.0 | crates.io | [link](https://github.com/tauri-apps/plugins-workspace) |
| tauri-runtime | 2.11.3 | crates.io | [link](https://github.com/tauri-apps/tauri) |
| tauri-runtime-wry | 2.11.4 | crates.io | [link](https://github.com/tauri-apps/tauri) |
| tauri-utils | 2.9.3 | crates.io | [link](https://github.com/tauri-apps/tauri) |
| tauri-winrt-notification | 0.7.3 | crates.io | [link](https://github.com/tauri-apps/winrt-notification) |
| tempfile | 3.27.0 | crates.io | [link](https://github.com/Stebalien/tempfile) |
| tendril | 0.5.1 | crates.io | [link](https://github.com/servo/html5ever) |
| thiserror | 1.0.69 | crates.io | [link](https://github.com/dtolnay/thiserror) |
| thiserror | 2.0.18 | crates.io | [link](https://github.com/dtolnay/thiserror) |
| thiserror-impl | 1.0.69 | crates.io | [link](https://github.com/dtolnay/thiserror) |
| thiserror-impl | 2.0.18 | crates.io | [link](https://github.com/dtolnay/thiserror) |
| thread-id | 5.1.0 | crates.io | [link](https://github.com/ruuda/thread-id) |
| time | 0.3.53 | crates.io | [link](https://github.com/time-rs/time) |
| time-core | 0.1.9 | crates.io | [link](https://github.com/time-rs/time) |
| time-macros | 0.2.31 | crates.io | [link](https://github.com/time-rs/time) |
| tokio-rustls | 0.26.4 | crates.io | [link](https://github.com/rustls/tokio-rustls) |
| toml | 1.1.2+spec-1.1.0 | crates.io | [link](https://github.com/toml-rs/toml) |
| toml_datetime | 0.6.3 | crates.io | [link](https://github.com/toml-rs/toml) |
| toml_datetime | 1.1.1+spec-1.1.0 | crates.io | [link](https://github.com/toml-rs/toml) |
| toml_edit | 0.19.15 | crates.io | [link](https://github.com/toml-rs/toml) |
| toml_edit | 0.20.2 | crates.io | [link](https://github.com/toml-rs/toml) |
| toml_edit | 0.25.12+spec-1.1.0 | crates.io | [link](https://github.com/toml-rs/toml) |
| toml_parser | 1.1.2+spec-1.1.0 | crates.io | [link](https://github.com/toml-rs/toml) |
| toml_writer | 1.1.1+spec-1.1.0 | crates.io | [link](https://github.com/toml-rs/toml) |
| tray-icon | 0.24.1 | crates.io | [link](https://github.com/tauri-apps/tray-icon) |
| typeid | 1.0.3 | crates.io | [link](https://github.com/dtolnay/typeid) |
| typenum | 1.20.1 | crates.io | [link](https://github.com/paholg/typenum) |
| uname | 0.1.1 | crates.io | [link](https://github.com/icorderi/rust-uname) |
| unic-char-property | 0.9.0 | crates.io | [link](https://github.com/open-i18n/rust-unic/) |
| unic-char-range | 0.9.0 | crates.io | [link](https://github.com/open-i18n/rust-unic/) |
| unic-common | 0.9.0 | crates.io | [link](https://github.com/open-i18n/rust-unic/) |
| unic-ucd-ident | 0.9.0 | crates.io | [link](https://github.com/open-i18n/rust-unic/) |
| unic-ucd-version | 0.9.0 | crates.io | [link](https://github.com/open-i18n/rust-unic/) |
| unicode-segmentation | 1.13.3 | crates.io | [link](https://github.com/unicode-rs/unicode-segmentation) |
| unicode-xid | 0.2.6 | crates.io | [link](https://github.com/unicode-rs/unicode-xid) |
| ureq | 3.4.0 | crates.io | [link](https://github.com/algesten/ureq) |
| ureq-proto | 0.6.1 | crates.io | [link](https://github.com/algesten/ureq-proto) |
| url | 2.5.8 | crates.io | [link](https://github.com/servo/rust-url) |
| utf8_iter | 1.0.4 | crates.io | [link](https://github.com/hsivonen/utf8_iter) |
| utf8-zero | 0.8.1 | crates.io | [link](https://github.com/algesten/utf8-zero) |
| uuid | 1.23.4 | crates.io | [link](https://github.com/uuid-rs/uuid) |
| wasm-bindgen | 0.2.126 | crates.io | [link](https://github.com/wasm-bindgen/wasm-bindgen) |
| wasm-bindgen-futures | 0.4.76 | crates.io | [link](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures) |
| wasm-bindgen-macro | 0.2.126 | crates.io | [link](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro) |
| wasm-bindgen-macro-support | 0.2.126 | crates.io | [link](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro-support) |
| wasm-bindgen-shared | 0.2.126 | crates.io | [link](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared) |
| wasm-streams | 0.5.0 | crates.io | [link](https://github.com/MattiasBuelens/wasm-streams/) |
| web_atoms | 0.2.5 | crates.io | [link](https://github.com/servo/html5ever) |
| web-sys | 0.3.103 | crates.io | [link](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys) |
| web-time | 1.1.0 | crates.io | [link](https://github.com/daxpedda/web-time) |
| winapi | 0.3.9 | crates.io | [link](https://github.com/retep998/winapi-rs) |
| winapi-i686-pc-windows-gnu | 0.4.0 | crates.io | [link](https://github.com/retep998/winapi-rs) |
| winapi-x86_64-pc-windows-gnu | 0.4.0 | crates.io | [link](https://github.com/retep998/winapi-rs) |
| window-vibrancy | 0.6.0 | crates.io | [link](https://github.com/tauri-apps/tauri-plugin-vibrancy) |
| windows | 0.58.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows | 0.61.3 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_aarch64_gnullvm | 0.42.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_aarch64_gnullvm | 0.48.5 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_aarch64_gnullvm | 0.52.6 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_aarch64_gnullvm | 0.53.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_aarch64_msvc | 0.42.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_aarch64_msvc | 0.48.5 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_aarch64_msvc | 0.52.6 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_aarch64_msvc | 0.53.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_gnu | 0.42.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_gnu | 0.48.5 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_gnu | 0.52.6 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_gnu | 0.53.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_gnullvm | 0.52.6 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_gnullvm | 0.53.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_msvc | 0.42.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_msvc | 0.48.5 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_msvc | 0.52.6 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_i686_msvc | 0.53.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnu | 0.42.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnu | 0.48.5 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnu | 0.52.6 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnu | 0.53.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnullvm | 0.42.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnullvm | 0.48.5 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnullvm | 0.52.6 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_gnullvm | 0.53.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_msvc | 0.42.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_msvc | 0.48.5 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_msvc | 0.52.6 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows_x86_64_msvc | 0.53.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-collections | 0.2.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-core | 0.58.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-core | 0.61.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-core | 0.62.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-future | 0.2.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-implement | 0.58.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-implement | 0.60.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-interface | 0.58.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-interface | 0.59.3 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-link | 0.1.3 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-link | 0.2.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-numerics | 0.2.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-registry | 0.6.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-result | 0.2.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-result | 0.3.4 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-result | 0.4.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-strings | 0.1.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-strings | 0.4.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-strings | 0.5.1 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.45.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.48.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.52.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.59.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.60.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.61.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-targets | 0.42.2 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-targets | 0.48.5 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-targets | 0.52.6 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-targets | 0.53.5 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-threading | 0.1.0 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| windows-version | 0.1.7 | crates.io | [link](https://github.com/microsoft/windows-rs) |
| wry | 0.55.1 | crates.io | [link](https://github.com/tauri-apps/wry) |
| xattr | 1.6.1 | crates.io | [link](https://github.com/Stebalien/xattr) |
| zeroize | 1.9.0 | crates.io | [link](https://github.com/RustCrypto/utils) |

## MIT (127)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| atk | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| atk-sys | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| auto-launch | 0.5.0 | crates.io | [link](https://github.com/zzzgydi/auto-launch.git) |
| block2 | 0.6.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| bytes | 1.12.1 | crates.io | [link](https://github.com/tokio-rs/bytes) |
| cairo-rs | 0.18.5 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| cairo-sys-rs | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| cargo_metadata | 0.19.2 | crates.io | [link](https://github.com/oli-obk/cargo_metadata) |
| cfb | 0.7.3 | crates.io | [link](https://github.com/mdsteele/rust-cfb) |
| combine | 4.6.7 | crates.io | [link](https://github.com/Marwes/combine) |
| darling | 0.23.0 | crates.io | [link](https://github.com/TedDriggs/darling) |
| darling_core | 0.23.0 | crates.io | [link](https://github.com/TedDriggs/darling) |
| darling_macro | 0.23.0 | crates.io | [link](https://github.com/TedDriggs/darling) |
| derive_more | 2.1.1 | crates.io | [link](https://github.com/JelteF/derive_more) |
| derive_more-impl | 2.1.1 | crates.io | [link](https://github.com/JelteF/derive_more) |
| dlopen2 | 0.8.2 | crates.io | [link](https://github.com/OpenByteDev/dlopen2) |
| dlopen2_derive | 0.4.3 | crates.io | [link](https://github.com/OpenByteDev/dlopen2) |
| dom_query | 0.27.0 | crates.io | [link](https://github.com/niklak/dom_query) |
| endi | 1.1.1 | crates.io | [link](https://github.com/zeenix/endi) |
| gdk | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| gdk-pixbuf | 0.18.5 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| gdk-pixbuf-sys | 0.18.0 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| gdk-sys | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| gdkwayland-sys | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| gdkx11 | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| gdkx11-sys | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| generic-array | 0.14.7 | crates.io | [link](https://github.com/fizyk20/generic-array.git) |
| gio | 0.18.4 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| gio-sys | 0.18.1 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| glib | 0.18.5 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| glib-macros | 0.18.5 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| glib-sys | 0.18.1 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| gobject-sys | 0.18.0 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| gtk | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| gtk-sys | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| gtk3-macros | 0.18.2 | crates.io | [link](https://github.com/gtk-rs/gtk3-rs) |
| hostname | 0.4.2 | crates.io | [link](https://github.com/djc/hostname) |
| http-body | 1.0.1 | crates.io | [link](https://github.com/hyperium/http-body) |
| http-body-util | 0.1.3 | crates.io | [link](https://github.com/hyperium/http-body) |
| hyper | 1.10.1 | crates.io | [link](https://github.com/hyperium/hyper) |
| hyper-util | 0.1.20 | crates.io | [link](https://github.com/hyperium/hyper-util) |
| ico | 0.5.0 | crates.io | [link](https://github.com/mdsteele/rust-ico) |
| infer | 0.19.0 | crates.io | [link](https://github.com/bojand/infer) |
| is-docker | 0.2.0 | crates.io | [link](https://github.com/TheLarkInn/is-docker) |
| is-wsl | 0.4.0 | crates.io | [link](https://github.com/TheLarkInn/is-wsl) |
| javascriptcore-rs | 1.1.2 | crates.io | [link](https://github.com/tauri-apps/javascriptcore-rs) |
| javascriptcore-rs-sys | 1.1.1 | crates.io | [link](https://github.com/tauri-apps/javascriptcore-rs) |
| libredox | 0.1.18 | crates.io | [link](https://gitlab.redox-os.org/redox-os/libredox.git) |
| memoffset | 0.9.1 | crates.io | [link](https://github.com/Gilnaa/memoffset) |
| minisign-verify | 0.2.5 | crates.io | [link](https://github.com/jedisct1/rust-minisign-verify) |
| mio | 1.2.1 | crates.io | [link](https://github.com/tokio-rs/mio) |
| new_debug_unreachable | 1.0.6 | crates.io | [link](https://github.com/mbrubeck/rust-debug-unreachable) |
| nix | 0.31.3 | crates.io | [link](https://github.com/nix-rust/nix) |
| objc2 | 0.6.4 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-encode | 4.1.0 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-foundation | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| open | 5.3.6 | crates.io | [link](https://github.com/Byron/open-rs) |
| os_info | 3.15.0 | crates.io | [link](https://github.com/stanislav-tkach/os_info) |
| os_pipe | 1.2.3 | crates.io | [link](https://github.com/oconnor663/os_pipe.rs) |
| pango | 0.18.3 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| pango-sys | 0.18.0 | crates.io | [link](https://github.com/gtk-rs/gtk-rs-core) |
| phf | 0.13.1 | crates.io | [link](https://github.com/rust-phf/rust-phf) |
| phf_generator | 0.13.1 | crates.io | [link](https://github.com/rust-phf/rust-phf) |
| phf_macros | 0.13.1 | crates.io | [link](https://github.com/rust-phf/rust-phf) |
| phf_shared | 0.13.1 | crates.io | [link](https://github.com/rust-phf/rust-phf) |
| plist | 1.10.0 | crates.io | [link](https://github.com/ebarnard/rust-plist/) |
| precomputed-hash | 0.1.1 | crates.io | [link](https://github.com/emilio/precomputed-hash) |
| quick-xml | 0.41.0 | crates.io | [link](https://github.com/tafia/quick-xml) |
| redox_syscall | 0.5.18 | crates.io | [link](https://gitlab.redox-os.org/redox-os/syscall) |
| redox_users | 0.4.6 | crates.io | [link](https://gitlab.redox-os.org/redox-os/users) |
| redox_users | 0.5.2 | crates.io | [link](https://gitlab.redox-os.org/redox-os/users) |
| rfd | 0.16.0 | crates.io | [link](https://github.com/PolyMeilex/rfd) |
| schannel | 0.1.29 | crates.io | [link](https://github.com/steffengy/schannel-rs) |
| schemars | 0.8.22 | crates.io | [link](https://github.com/GREsau/schemars) |
| schemars | 0.9.0 | crates.io | [link](https://github.com/GREsau/schemars) |
| schemars | 1.2.1 | crates.io | [link](https://github.com/GREsau/schemars) |
| schemars_derive | 0.8.22 | crates.io | [link](https://github.com/GREsau/schemars) |
| sentry | 0.49.1 | crates.io | [link](https://github.com/getsentry/sentry-rust) |
| sentry-backtrace | 0.49.1 | crates.io | [link](https://github.com/getsentry/sentry-rust) |
| sentry-contexts | 0.49.1 | crates.io | [link](https://github.com/getsentry/sentry-rust) |
| sentry-core | 0.49.1 | crates.io | [link](https://github.com/getsentry/sentry-rust) |
| sentry-log | 0.49.1 | crates.io | [link](https://github.com/getsentry/sentry-rust) |
| sentry-tracing | 0.49.1 | crates.io | [link](https://github.com/getsentry/sentry-rust) |
| sentry-types | 0.49.1 | crates.io | [link](https://github.com/getsentry/sentry-rust) |
| shared_child | 1.1.1 | crates.io | [link](https://github.com/oconnor663/shared_child.rs) |
| sigchld | 0.2.4 | crates.io | [link](https://github.com/oconnor663/sigchld.rs) |
| simd-adler32 | 0.3.9 | crates.io | [link](https://github.com/mcountryman/simd-adler32) |
| slab | 0.4.12 | crates.io | [link](https://github.com/tokio-rs/slab) |
| soup3 | 0.5.0 | crates.io | [link](https://gitlab.gnome.org/World/Rust/soup3-rs) |
| soup3-sys | 0.5.0 | crates.io | [link](https://gitlab.gnome.org/World/Rust/soup3-rs) |
| strsim | 0.11.1 | crates.io | [link](https://github.com/rapidfuzz/strsim-rs) |
| synstructure | 0.13.2 | crates.io | [link](https://github.com/mystor/synstructure) |
| tokio | 1.52.3 | crates.io | [link](https://github.com/tokio-rs/tokio) |
| tokio-macros | 2.7.0 | crates.io | [link](https://github.com/tokio-rs/tokio) |
| tokio-util | 0.7.18 | crates.io | [link](https://github.com/tokio-rs/tokio) |
| tower | 0.5.3 | crates.io | [link](https://github.com/tower-rs/tower) |
| tower-http | 0.6.11 | crates.io | [link](https://github.com/tower-rs/tower-http) |
| tower-layer | 0.3.3 | crates.io | [link](https://github.com/tower-rs/tower) |
| tower-service | 0.3.3 | crates.io | [link](https://github.com/tower-rs/tower) |
| tracing | 0.1.44 | crates.io | [link](https://github.com/tokio-rs/tracing) |
| tracing-attributes | 0.1.31 | crates.io | [link](https://github.com/tokio-rs/tracing) |
| tracing-core | 0.1.36 | crates.io | [link](https://github.com/tokio-rs/tracing) |
| tracing-subscriber | 0.3.23 | crates.io | [link](https://github.com/tokio-rs/tracing) |
| try-lock | 0.2.5 | crates.io | [link](https://github.com/seanmonstar/try-lock) |
| uds_windows | 1.2.1 | crates.io | [link](https://github.com/haraldh/rust_uds_windows) |
| urlpattern | 0.3.0 | crates.io | [link](https://github.com/denoland/rust-urlpattern) |
| valuable | 0.1.1 | crates.io | [link](https://github.com/tokio-rs/valuable) |
| want | 0.3.1 | crates.io | [link](https://github.com/seanmonstar/want) |
| webkit2gtk | 2.0.2 | crates.io | [link](https://github.com/tauri-apps/webkit2gtk-rs) |
| webkit2gtk-sys | 2.0.2 | crates.io | [link](https://github.com/tauri-apps/webkit2gtk-rs) |
| webview2-com | 0.38.2 | crates.io | [link](https://github.com/wravery/webview2-rs) |
| webview2-com-macros | 0.8.1 | crates.io | [link](https://github.com/wravery/webview2-rs) |
| webview2-com-sys | 0.38.2 | crates.io | [link](https://github.com/wravery/webview2-rs) |
| winnow | 0.5.40 | crates.io | [link](https://github.com/winnow-rs/winnow) |
| winnow | 1.0.3 | crates.io | [link](https://github.com/winnow-rs/winnow) |
| winreg | 0.10.1 | crates.io | [link](https://github.com/gentoo90/winreg-rs) |
| winreg | 0.52.0 | crates.io | [link](https://github.com/gentoo90/winreg-rs) |
| x11 | 2.21.0 | crates.io | [link](https://github.com/AltF02/x11-rs.git) |
| x11-dl | 2.21.0 | crates.io | [link](https://github.com/AltF02/x11-rs.git) |
| zbus | 5.17.0 | crates.io | [link](https://github.com/z-galaxy/zbus/) |
| zbus_macros | 5.17.0 | crates.io | [link](https://github.com/z-galaxy/zbus/) |
| zbus_names | 4.3.3 | crates.io | [link](https://github.com/z-galaxy/zbus/) |
| zip | 4.6.1 | crates.io | [link](https://github.com/zip-rs/zip2.git) |
| zmij | 1.0.21 | crates.io | [link](https://github.com/dtolnay/zmij) |
| zvariant | 5.13.0 | crates.io | [link](https://github.com/z-galaxy/zbus/) |
| zvariant_derive | 5.13.0 | crates.io | [link](https://github.com/z-galaxy/zbus/) |
| zvariant_utils | 3.5.0 | crates.io | [link](https://github.com/z-galaxy/zbus/) |

## Apache-2.0 OR MIT OR Zlib (22)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| bytemuck | 1.25.0 | crates.io | [link](https://github.com/Lokathor/bytemuck) |
| dispatch2 | 0.3.1 | crates.io | [link](https://github.com/madsmtm/objc2) |
| lru-slab | 0.1.2 | crates.io | [link](https://github.com/Ralith/lru-slab) |
| miniz_oxide | 0.8.9 | crates.io | [link](https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide) |
| objc2-app-kit | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-cloud-kit | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-core-data | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-core-foundation | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-core-graphics | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-core-image | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-core-location | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-core-text | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-exception-helper | 0.1.1 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-io-surface | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-osa-kit | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-quartz-core | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-ui-kit | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-user-notifications | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| objc2-web-kit | 0.3.2 | crates.io | [link](https://github.com/madsmtm/objc2) |
| raw-window-handle | 0.6.2 | crates.io | [link](https://github.com/rust-windowing/raw-window-handle) |
| tinyvec | 1.11.0 | crates.io | [link](https://github.com/Lokathor/tinyvec) |
| tinyvec_macros | 0.1.1 | crates.io | [link](https://github.com/Soveu/tinyvec_macros) |

## Unicode-3.0 (18)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| icu_collections | 2.2.0 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| icu_locale_core | 2.2.0 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| icu_normalizer | 2.2.0 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| icu_normalizer_data | 2.2.0 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| icu_properties | 2.2.0 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| icu_properties_data | 2.2.0 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| icu_provider | 2.2.0 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| litemap | 0.8.2 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| potential_utf | 0.1.5 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| tinystr | 0.8.3 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| writeable | 0.6.3 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| yoke | 0.8.3 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| yoke-derive | 0.8.2 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| zerofrom | 0.1.8 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| zerofrom-derive | 0.1.7 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| zerotrie | 0.2.4 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| zerovec | 0.11.6 | crates.io | [link](https://github.com/unicode-org/icu4x) |
| zerovec-derive | 0.11.3 | crates.io | [link](https://github.com/unicode-org/icu4x) |

## MIT OR Unlicense (7)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| aho-corasick | 1.1.4 | crates.io | [link](https://github.com/BurntSushi/aho-corasick) |
| byteorder | 1.5.0 | crates.io | [link](https://github.com/BurntSushi/byteorder) |
| byteorder-lite | 0.1.0 | crates.io | [link](https://github.com/image-rs/byteorder-lite) |
| memchr | 2.8.3 | crates.io | [link](https://github.com/BurntSushi/memchr) |
| same-file | 1.0.6 | crates.io | [link](https://github.com/BurntSushi/same-file) |
| walkdir | 2.5.0 | crates.io | [link](https://github.com/BurntSushi/walkdir) |
| winapi-util | 0.1.11 | crates.io | [link](https://github.com/BurntSushi/winapi-util) |

## Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT (5)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| linux-raw-sys | 0.12.1 | crates.io | [link](https://github.com/sunfishcode/linux-raw-sys) |
| rustix | 1.1.4 | crates.io | [link](https://github.com/bytecodealliance/rustix) |
| wasi | 0.11.1+wasi-snapshot-preview1 | crates.io | [link](https://github.com/bytecodealliance/wasi) |
| wasip2 | 1.0.4+wasi-0.2.12 | crates.io | [link](https://github.com/bytecodealliance/wasi-rs) |
| wit-bindgen | 0.57.1 | crates.io | [link](https://github.com/bytecodealliance/wit-bindgen) |

## MPL-2.0 (5)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| cssparser | 0.36.0 | crates.io | [link](https://github.com/servo/rust-cssparser) |
| cssparser-macros | 0.6.1 | crates.io | [link](https://github.com/servo/rust-cssparser) |
| dtoa-short | 0.3.5 | crates.io | [link](https://github.com/upsuper/dtoa-short) |
| option-ext | 0.2.0 | crates.io | [link](https://github.com/soc/option-ext.git) |
| selectors | 0.36.1 | crates.io | [link](https://github.com/servo/stylo) |

## Apache-2.0 (3)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| debugid | 0.8.0 | crates.io | [link](https://github.com/getsentry/rust-debugid) |
| sync_wrapper | 1.0.2 | crates.io | [link](https://github.com/Actyx/sync_wrapper) |
| tao | 0.35.3 | crates.io | [link](https://github.com/tauri-apps/tao) |

## Apache-2.0 OR ISC OR MIT (3)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| hyper-rustls | 0.27.9 | crates.io | [link](https://github.com/rustls/hyper-rustls) |
| rustls | 0.23.43 | crates.io | [link](https://github.com/rustls/rustls) |
| rustls-native-certs | 0.8.4 | crates.io | [link](https://github.com/rustls/rustls-native-certs) |

## BSD-3-Clause (3)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| alloc-no-stdlib | 2.0.4 | crates.io | [link](https://github.com/dropbox/rust-alloc-no-stdlib) |
| alloc-stdlib | 0.2.4 | crates.io | [link](https://github.com/dropbox/rust-alloc-no-stdlib) |
| subtle | 2.6.1 | crates.io | [link](https://github.com/dalek-cryptography/subtle) |

## ISC (3)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| libloading | 0.7.4 | crates.io | [link](https://github.com/nagisa/rust_libloading/) |
| rustls-webpki | 0.103.15 | crates.io | [link](https://github.com/rustls/webpki) |
| untrusted | 0.9.0 | crates.io | [link](https://github.com/briansmith/untrusted) |

## Apache-2.0 OR BSD-2-Clause OR MIT (2)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| zerocopy | 0.8.54 | crates.io | [link](https://github.com/google/zerocopy) |
| zerocopy-derive | 0.8.54 | crates.io | [link](https://github.com/google/zerocopy) |

## Apache-2.0 OR BSD-3-Clause (2)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| moxcms | 0.8.1 | crates.io | [link](https://github.com/awxkee/moxcms.git) |
| pxfm | 0.1.30 | crates.io | [link](https://github.com/awxkee/pxfm) |

## Apache-2.0 OR BSD-3-Clause OR MIT (2)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| num_enum | 0.7.6 | crates.io | [link](https://github.com/illicitonion/num_enum) |
| num_enum_derive | 0.7.6 | crates.io | [link](https://github.com/illicitonion/num_enum) |

## Apache-2.0 OR LGPL-2.1-or-later OR MIT (2)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| r-efi | 5.3.0 | crates.io | [link](https://github.com/r-efi/r-efi) |
| r-efi | 6.0.0 | crates.io | [link](https://github.com/r-efi/r-efi) |

## CDLA-Permissive-2.0 (2)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| webpki-root-certs | 1.0.9 | crates.io | [link](https://github.com/rustls/webpki-roots) |
| webpki-roots | 1.0.9 | crates.io | [link](https://github.com/rustls/webpki-roots) |

## (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0) AND (Apache-2.0 OR ISC) AND Apache-2.0 AND BSD-3-Clause AND ISC AND MIT (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| aws-lc-sys | 0.44.0 | crates.io | [link](https://github.com/aws/aws-lc-rs) |

## (Apache-2.0 OR ISC) AND ISC (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| aws-lc-rs | 1.18.0 | crates.io | [link](https://github.com/aws/aws-lc-rs) |

## (Apache-2.0 OR MIT) AND BSD-3-Clause (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| encoding_rs | 0.8.35 | crates.io | [link](https://github.com/hsivonen/encoding_rs) |

## (Apache-2.0 OR MIT) AND Unicode-3.0 (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| unicode-ident | 1.0.24 | crates.io | [link](https://github.com/dtolnay/unicode-ident) |

## 0BSD (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| mock_instant | 0.6.1 | crates.io | [link](https://github.com/museun/mock_instant) |

## 0BSD OR Apache-2.0 OR MIT (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| adler2 | 2.0.1 | crates.io | [link](https://github.com/oyvindln/adler2) |

## Apache-2.0 AND ISC (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| ring | 0.17.14 | crates.io | [link](https://github.com/briansmith/ring) |

## Apache-2.0 AND MIT (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| dpi | 0.1.2 | crates.io | [link](https://github.com/rust-windowing/winit) |

## Apache-2.0 OR CC0-1.0 OR MIT-0 (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| dunce | 1.0.5 | crates.io | [link](https://gitlab.com/kornelski/dunce) |

## BSD-3-Clause AND MIT (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| brotli | 8.0.4 | crates.io | [link](https://github.com/dropbox/rust-brotli) |

## BSD-3-Clause OR MIT (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| brotli-decompressor | 5.0.3 | crates.io | [link](https://github.com/dropbox/rust-brotli-decompressor) |

## MIT OR Apache-2.0 (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| @tauri-apps/plugin-opener | 2.5.4 | npm | [link](https://github.com/tauri-apps/plugins-workspace) |

## Zlib (1)

| Package | Version | Ecosystem | Source |
| --- | --- | --- | --- |
| foldhash | 0.2.0 | crates.io | [link](https://github.com/orlp/foldhash) |

